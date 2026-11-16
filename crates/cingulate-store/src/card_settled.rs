use rusqlite::{params, Connection};
use serde::Serialize;

use crate::{ConsentStore, ElicitationRecord, Result, State};

pub const CARD_SETTLED_READERS: [&str; 5] = ["aft", "plexus", "basal", "broca", "prefrontal-core"];
pub const CARD_SETTLED_PENDING_CAP: i64 = 16_384;

/// A feed pointer only: card text and answer contents never enter the bus body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardSettled {
    pub elicitation_id: String,
    pub reader: String,
    pub feed: String,
    pub seq: i64,
}

impl CardSettled {
    pub fn msg_id(&self) -> String {
        // One message per reader per settlement. Reopened cards settle again at
        // a new feed position; retries of that settlement retain the same id.
        format!("{}:{}:{}", self.elicitation_id, self.reader, self.seq)
    }

    pub fn body(&self) -> Vec<u8> {
        #[derive(Serialize)]
        struct Body<'a> {
            elicitation_id: &'a str,
            feed: &'a str,
            seq: i64,
        }
        serde_json::to_vec(&Body {
            elicitation_id: &self.elicitation_id,
            feed: &self.feed,
            seq: self.seq,
        })
        .expect("feed pointer serialization")
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CardSettledCounts {
    pub pending: u64,
    pub published: u64,
    pub dropped: u64,
}

pub(crate) fn enqueue(conn: &Connection, record: &ElicitationRecord) -> Result<()> {
    let seq: i64 = conn.query_row(
        "SELECT cursor FROM elicitation_answers WHERE elicitation_id=?1",
        [&record.elicitation_id],
        |row| row.get(0),
    )?;
    // requester is the stored daemon-stamped principal, not request.requester_ref
    // or owner (which also includes session coordinates).
    let filer = record.requester.strip_prefix("reserved:");
    for reader in CARD_SETTLED_READERS {
        let feed = if reader == "prefrontal-core" {
            "outcomes"
        } else if filer == Some(reader) && record.state != State::Withdrawn {
            "answers"
        } else {
            continue;
        };
        conn.execute(
            "INSERT INTO card_settled_outbox(elicitation_id,reader,feed,seq) VALUES (?1,?2,?3,?4)",
            params![record.elicitation_id, reader, feed, seq],
        )?;
        cap_reader(conn, reader, CARD_SETTLED_PENDING_CAP)?;
    }
    Ok(())
}

fn cap_reader(conn: &Connection, reader: &str, cap: i64) -> Result<()> {
    // An outage retains at most 16,384 rows per reader (81,920 total). A hint
    // says only that a feed moved to at least seq N. Readers reread from their
    // own cursor, so a later hint covers earlier dropped hints to that reader.
    // Never drop the newest hint, or let a busy reader evict another's hints.
    let dropped = conn.execute(
        "DELETE FROM card_settled_outbox WHERE reader=?1 AND seq IN \
         (SELECT seq FROM card_settled_outbox WHERE reader=?1 ORDER BY seq DESC LIMIT -1 OFFSET ?2)",
        params![reader, cap],
    )?;
    if dropped > 0 {
        conn.execute(
            "UPDATE card_settled_counters SET dropped=dropped+?2 WHERE reader=?1",
            params![reader, dropped as i64],
        )?;
    }
    Ok(())
}

impl ConsentStore {
    /// A reader's oldest pointers, independent of other readers' backlogs.
    pub fn card_settled_pending_for_reader(
        &self,
        reader: &str,
        limit: usize,
    ) -> Result<Vec<CardSettled>> {
        self.read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT elicitation_id,reader,feed,seq FROM card_settled_outbox \
                 WHERE reader=?1 ORDER BY seq LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![reader, limit.min(256) as i64], |row| {
                Ok(CardSettled {
                    elicitation_id: row.get(0)?,
                    reader: row.get(1)?,
                    feed: row.get(2)?,
                    seq: row.get(3)?,
                })
            })?;
            Ok(rows.collect::<std::result::Result<_, _>>()?)
        })
    }

    pub fn card_settled_pending(&self, limit: usize) -> Result<Vec<CardSettled>> {
        self.read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT elicitation_id,reader,feed,seq FROM card_settled_outbox ORDER BY seq,reader LIMIT ?1",
            )?;
            let rows = stmt.query_map([limit.min(256) as i64], |row| {
                Ok(CardSettled {
                    elicitation_id: row.get(0)?,
                    reader: row.get(1)?,
                    feed: row.get(2)?,
                    seq: row.get(3)?,
                })
            })?;
            Ok(rows.collect::<std::result::Result<_, _>>()?)
        })
    }

    /// The caller must have received the server's publish acknowledgement.
    pub fn card_settled_published(&self, event: &CardSettled) -> Result<()> {
        self.transaction(|tx| {
            let deleted = tx.execute(
                "DELETE FROM card_settled_outbox WHERE elicitation_id=?1 AND reader=?2 AND seq=?3",
                params![event.elicitation_id, event.reader, event.seq],
            )?;
            // Overflow may have evicted a row while its publish was in flight.
            if deleted > 0 {
                tx.execute(
                    "UPDATE card_settled_counters SET published=published+1 WHERE reader=?1",
                    [&event.reader],
                )?;
            }
            Ok(())
        })
    }

    pub fn card_settled_counts(&self) -> Result<[CardSettledCounts; 5]> {
        self.read(|conn| {
            let mut counts = [CardSettledCounts::default(); 5];
            for (index, reader) in CARD_SETTLED_READERS.iter().enumerate() {
                counts[index] = conn.query_row(
                    "SELECT (SELECT COUNT(*) FROM card_settled_outbox WHERE reader=?1),published,dropped \
                     FROM card_settled_counters WHERE reader=?1",
                    [reader],
                    |row| Ok(CardSettledCounts { pending: row.get(0)?, published: row.get(1)?, dropped: row.get(2)? }),
                )?;
            }
            Ok(counts)
        })
    }
}
