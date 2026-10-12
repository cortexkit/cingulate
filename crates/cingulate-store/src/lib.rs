#![forbid(unsafe_code)]

//! SQLite consent card persistence. Identity authorization and notifications
//! belong to the caller; mutations return events only after their commit.

use std::fmt;

use cingulate_decode::{ConsentRequest, Refusal};
use cortexkit_lease::LeaseError;
use cortexkit_store::{open_sqlite, Migration, SqliteStore, StoreError};
pub use cortexkit_store_types::{sqlite_store_path, Isolation, StorageBackend, StorageDescriptor};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

mod access;
mod record;
pub use access::{
    AccessAnswer, AccessIdentity, AccessMutation, AccessRecord, AnswerAttempt, Notice,
    ScopeIdentity,
};
pub use record::{ElicitationRecord, State, Subject, LATE_ANSWER_PURPOSE};

pub const RETENTION_MS: i64 = 86_400_000;
pub const MIGRATION_NAMESPACE: &str = "cingulate";
pub const MIGRATIONS: [Migration; 5] = [
    Migration {
        version: 1,
        statements: include_str!("../migrations/001_elicitation.sql"),
    },
    Migration {
        version: 2,
        statements: include_str!("../migrations/002_id_seq.sql"),
    },
    Migration {
        version: 3,
        statements: include_str!("../migrations/003_pending_revision.sql"),
    },
    Migration {
        version: 4,
        statements: include_str!("../migrations/004_access_asks.sql"),
    },
    Migration {
        version: 5,
        statements: include_str!("../migrations/005_shared_notice_outbox.sql"),
    },
];

#[derive(Debug)]
pub enum Error {
    Store(StoreError),
    Sqlite(rusqlite::Error),
    Json(serde_json::Error),
    Refused(Refusal),
}

impl Error {
    /// Only lease contention should enter the module's bounded open retry.
    pub fn is_lease_held(&self) -> bool {
        matches!(
            self,
            Self::Store(StoreError::Lease(LeaseError::Held { .. }))
        )
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::Refused(refusal) => refusal.code,
            _ => "elicitation_store_error",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(e) => e.fmt(f),
            Self::Sqlite(e) => e.fmt(f),
            Self::Json(e) => e.fmt(f),
            Self::Refused(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for Error {}

impl From<StoreError> for Error {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}
impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}
impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

type Result<T> = std::result::Result<T, Error>;

fn refuse(code: &'static str, message: &str) -> Error {
    Error::Refused(Refusal::new(code, message))
}

/// Supplied only after the module authorizes its daemon route stamp and resolves
/// the subject. A request body must never construct these identity fields.
#[derive(Debug, Clone)]
pub struct RequestIdentity {
    /// The module's JSON encoding of `[principal, bind_session]`.
    pub owner: String,
    pub requester: String,
    pub subject: Subject,
    /// A resolver-provided name, as distinct from a raw-id fallback. Only a
    /// successful resolution may replace a pending card's existing label.
    pub resolved_name: Option<String>,
    /// The agent session the daemon stamped on the requester's route, if any.
    /// A late answer on a detached card is addressed to this session.
    pub asker: Option<ScopeIdentity>,
}

#[derive(Debug, Clone)]
pub struct CreateResult {
    pub record: ElicitationRecord,
    /// False means an in-place refresh: wake waiters, but send no delivery.
    pub created: bool,
    /// Committed retractions, to send in this order before a new delivery.
    pub settlements: Vec<ElicitationRecord>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AnswerOutcome {
    NotFound,
    Answered(ElicitationRecord),
    AlreadyAnswered(ElicitationRecord),
    Conflict(ElicitationRecord),
}

#[derive(Debug, Clone)]
pub struct Mutation<T> {
    pub value: T,
    /// Every entry represents one newly committed answer-log row. Send one
    /// retraction and wake waiters per entry, after this method returns.
    pub settlements: Vec<ElicitationRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Feed {
    pub records: Vec<Value>,
    pub cursor: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continuation: Option<i64>,
}

// Reserve space for the result envelope, cursors and pending-list continuation.
// Record bytes include each array separator; the complete reply stays below 3 MiB.
pub const PAGE_RECORD_BYTES: usize = 3 * 1024 * 1024 - 512;

/// Large package source bodies are read through get, not repeated in card feeds.
pub fn omit_package_bodies(value: &mut Value) {
    if let Some(package) = value.get_mut("package").and_then(Value::as_object_mut) {
        package.remove("script");
        package.remove("manifest_json");
    }
}

/// Owns the shared store's SQLite connection and lifetime single-writer lease.
///
/// The module authorizes and validates each op before calling these methods.
/// Before serving get/feed/pending/answer/withdraw/await reads it calls `sweep`
/// and delivers its committed retractions. Request deliberately does NOT sweep
/// unrelated cards; a refused request must have no expiry side effects.
pub struct ConsentStore {
    db: SqliteStore,
}

impl fmt::Debug for ConsentStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConsentStore")
            .field("epoch", &self.db.epoch())
            .finish_non_exhaustive()
    }
}

/// Use the supervisor's descriptor as-is. Only an absent descriptor selects the
/// conventional fallback; an invalid or unsupported descriptor never falls back.
pub fn storage_descriptor(
    descriptor: Option<&StorageDescriptor>,
    data_home: &str,
) -> StorageDescriptor {
    descriptor.cloned().unwrap_or_else(|| StorageDescriptor {
        module_id: "cingulate".into(),
        storage_namespace: "main".into(),
        isolation: Isolation::Module,
        backend: StorageBackend::Sqlite {
            path: sqlite_store_path(data_home, "ck-cingulate"),
        },
    })
}

impl ConsentStore {
    /// Open exactly the descriptor path, with the shared store's WAL pragmas and
    /// lease. Settle downtime expiries before returning. The caller must await
    /// delivery of the returned retractions before marking the module ready.
    pub fn open(
        descriptor: &StorageDescriptor,
        now_ms: i64,
    ) -> Result<(Self, Vec<ElicitationRecord>)> {
        let db = open_sqlite(descriptor)?;
        let outcome = db.migrate(MIGRATION_NAMESPACE, &MIGRATIONS)?;
        if outcome.store_ahead() {
            return Err(Error::Store(StoreError::Migration(format!(
                "cingulate store version {} is ahead of this binary's migration {}",
                outcome.recorded, outcome.chain_max
            ))));
        }
        let store = Self { db };
        let settlements = store.sweep(now_ms)?;
        Ok((store, settlements))
    }

    fn read<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        self.db.with_conn(|conn| Ok(f(conn)))?
    }

    fn transaction<T>(&self, f: impl FnOnce(&Transaction<'_>) -> Result<T>) -> Result<T> {
        self.read(|conn| {
            let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
            let value = f(&tx)?;
            tx.commit()?;
            Ok(value)
        })
    }

    /// Insert or refresh one already-accepted request. A due dedup match is
    /// expired, and changed content withdraws the old card; replacement and id
    /// minting share that settlement's transaction and roll back together.
    pub fn create(
        &self,
        identity: &RequestIdentity,
        mut request: ConsentRequest,
        now_ms: i64,
    ) -> Result<CreateResult> {
        // Transient schemas are never part of storage or dedup identity, even
        // when a caller constructed the already-validated request directly.
        request.requested_schema = None;
        self.transaction(|tx| {
            let matched: Option<String> = match &request.dedup_key {
                Some(key) => tx
                    .query_row(
                        "SELECT elicitation_id FROM elicitation_records \
                     WHERE owner=?1 AND dedup_key=?2 AND state='pending'",
                        params![identity.owner, key],
                        |row| row.get(0),
                    )
                    .optional()?,
                None => None,
            };
            let mut settlements = Vec::new();
            if let Some(id) = matched {
                let mut old = load(tx, &id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                if old.expires_at_ms <= now_ms {
                    expire(tx, &mut old)?;
                    settlements.push(old);
                } else if same_content(&old, &request, &identity.subject) {
                    old.request = request;
                    old.expires_at_ms = now_ms.saturating_add(old.request.expires_in_ms);
                    if let Some(name) = &identity.resolved_name {
                        old.subject.label = name.clone();
                    }
                    // Raising the same card again means the requester holds a
                    // new call for it, so the card is held again: clear the
                    // detach mark, or an answer delivered in that call would
                    // also be sent as a late-answer notice.
                    old.detached = false;
                    save(tx, &old)?;
                    return Ok(CreateResult {
                        record: old,
                        created: false,
                        settlements,
                    });
                } else {
                    terminal(tx, &mut old, State::Withdrawn, None, now_ms)?;
                    settlements.push(old);
                }
            }
            let seq: i64 = tx.query_row(
                "UPDATE cingulate_id_seq SET value=value+1 \
                 WHERE singleton=1 AND value<9223372036854775807 RETURNING value",
                [],
                |row| row.get(0),
            )?;
            let mut subject = identity.subject.clone();
            if let Some(name) = &identity.resolved_name {
                subject.label = name.clone();
            }
            let record = ElicitationRecord {
                elicitation_id: format!("el_{seq}"),
                expires_at_ms: now_ms.saturating_add(request.expires_in_ms),
                request,
                created_at_ms: now_ms,
                subject,
                requester: identity.requester.clone(),
                state: State::Pending,
                answered_at_ms: None,
                answered_choice_id: None,
                execution: None,
                detached: false,
                asker: identity.asker.clone(),
            };
            tx.execute(
                "INSERT INTO elicitation_records \
                 (elicitation_id,owner,dedup_key,record_json,state,expires_at_ms) \
                 VALUES (?1,?2,?3,?4,'pending',?5)",
                params![
                    record.elicitation_id,
                    identity.owner,
                    record.request.dedup_key,
                    serde_json::to_string(&record)?,
                    record.expires_at_ms
                ],
            )?;
            Ok(CreateResult {
                record,
                created: true,
                settlements,
            })
        })
    }

    /// The owner remains queryable for acknowledged and tombstoned rows, letting
    /// the caller distinguish a missing owner match from a no-longer-readable card.
    pub fn owner(&self, id: &str) -> Result<Option<String>> {
        self.read(|conn| {
            Ok(conn
                .query_row(
                    "SELECT owner FROM elicitation_records WHERE elicitation_id=?1",
                    [id],
                    |row| row.get(0),
                )
                .optional()?)
        })
    }

    /// Read a retained, unacknowledged card. Sweep first to settle due cards.
    pub fn get(&self, id: &str, now_ms: i64) -> Result<Option<ElicitationRecord>> {
        self.read(|conn| visible(conn, id, now_ms))
    }

    pub fn get_owned(&self, owner: &str, id: &str, now_ms: i64) -> Result<ElicitationRecord> {
        self.read(|conn| owned(conn, owner, id, now_ms))
    }

    /// Return the durable polling counter that tracks pending-card changes.
    /// Triggers update it in the same transaction as each change.
    pub fn pending_revision(&self) -> Result<i64> {
        self.read(|conn| {
            Ok(conn.query_row(
                "SELECT value FROM cingulate_pending_revision WHERE singleton=1",
                [],
                |row| row.get(0),
            )?)
        })
    }

    /// Pending cards are ordered for display, not by insertion or feed order.
    pub fn pending(&self) -> Result<Vec<ElicitationRecord>> {
        self.read(|conn| {
            let ids = ids(
                conn,
                "SELECT elicitation_id FROM elicitation_records \
                WHERE state='pending' ORDER BY expires_at_ms,elicitation_id",
                [],
            )?;
            ids.iter()
                .map(|id| {
                    load(conn, id)?
                        .ok_or_else(|| Error::Sqlite(rusqlite::Error::QueryReturnedNoRows))
                })
                .collect()
        })
    }

    /// Answer one card atomically. The module checks operator authorization and
    /// params, sweeps globally, then calls this. Expiry is rechecked inside the
    /// transaction, before choice and console checks, so time can never grant.
    pub fn answer(
        &self,
        id: &str,
        choice: &str,
        console_machine_id: Option<&str>,
        now_ms: i64,
    ) -> Result<Mutation<AnswerOutcome>> {
        self.transaction(|tx| {
            let Some(mut record) = load(tx, id)? else {
                return Ok(Mutation {
                    value: AnswerOutcome::NotFound,
                    settlements: vec![],
                });
            };
            if !retained(&record, now_ms) {
                return Ok(Mutation {
                    value: AnswerOutcome::NotFound,
                    settlements: vec![],
                });
            }
            let mut settlements = Vec::new();
            if record.state == State::Pending && record.expires_at_ms <= now_ms {
                expire(tx, &mut record)?;
                settlements.push(record.clone());
            }
            let value = if record.state != State::Pending {
                if record.state == State::Answered
                    && record.answered_choice_id.as_deref() == Some(choice)
                {
                    AnswerOutcome::AlreadyAnswered(record)
                } else {
                    AnswerOutcome::Conflict(record)
                }
            } else {
                if !record
                    .request
                    .options
                    .iter()
                    .any(|option| option.id == choice)
                {
                    return Err(refuse(
                        "elicitation_invalid_choice",
                        "choice must name an option",
                    ));
                }
                if let Some(required) = record.request.at_console.as_deref() {
                    if console_machine_id != Some(required) {
                        return Err(refuse(
                            "user_not_at_console",
                            "answering machine must match at_console",
                        ));
                    }
                }
                terminal(
                    tx,
                    &mut record,
                    State::Answered,
                    Some(choice.to_owned()),
                    now_ms,
                )?;
                settlements.push(record.clone());
                AnswerOutcome::Answered(record)
            };
            Ok(Mutation { value, settlements })
        })
    }

    pub fn withdraw(
        &self,
        owner: &str,
        id: &str,
        now_ms: i64,
    ) -> Result<Mutation<ElicitationRecord>> {
        self.transaction(|tx| {
            let mut record = owned(tx, owner, id, now_ms)?;
            let mut settlements = Vec::new();
            if record.state == State::Pending {
                if record.expires_at_ms <= now_ms {
                    expire(tx, &mut record)?;
                } else {
                    terminal(tx, &mut record, State::Withdrawn, None, now_ms)?;
                }
                settlements.push(record.clone());
            }
            Ok(Mutation {
                value: record,
                settlements,
            })
        })
    }

    /// Record that the requester stopped holding the agent's call for this card.
    /// Only settlements committed after this mark queue a `consent_answered`
    /// notice. Because the mark and every settlement are serialized store
    /// transactions, each card is either settled while held (no notice) or
    /// settled after detach (one notice). A settled card is returned unchanged,
    /// a repeat returns the current record, and a repeat never undoes the mark;
    /// only an in-place dedup refresh in `create` clears it.
    pub fn detach(
        &self,
        owner: &str,
        id: &str,
        now_ms: i64,
    ) -> Result<Mutation<ElicitationRecord>> {
        self.transaction(|tx| {
            let mut record = owned(tx, owner, id, now_ms)?;
            if record.request.kind != "consent" {
                return Err(refuse(
                    "elicitation_requester_refused",
                    "consent.detach is admitted only for kind consent",
                ));
            }
            let mut settlements = Vec::new();
            if record.state == State::Pending {
                if record.expires_at_ms <= now_ms {
                    // The card was already due, so it settled while still held.
                    expire(tx, &mut record)?;
                    settlements.push(record.clone());
                } else if !record.detached {
                    record.detached = true;
                    save(tx, &record)?;
                }
            }
            Ok(Mutation {
                value: record,
                settlements,
            })
        })
    }

    pub fn report_execution(
        &self,
        owner: &str,
        id: &str,
        execution: &str,
        now_ms: i64,
    ) -> Result<ElicitationRecord> {
        self.transaction(|tx| {
            let mut record = owned(tx, owner, id, now_ms)?;
            if record.request.kind == "flow_decision"
                || !matches!(
                    execution,
                    "executed" | "approved_not_executed" | "outcome_unknown"
                )
            {
                return Err(refuse(
                    "elicitation_invalid_execution",
                    "invalid execution outcome",
                ));
            }
            if record.state != State::Answered {
                return Err(refuse(
                    "elicitation_store_error",
                    "execution requires an answered card",
                ));
            }
            record.execution = Some(execution.into());
            save(tx, &record)?;
            Ok(record)
        })
    }

    /// Prune first, then settle every pending due card, including downtime
    /// expiries. Returned records represent only this transaction's settlements.
    pub fn sweep(&self, now_ms: i64) -> Result<Vec<ElicitationRecord>> {
        self.transaction(|tx| {
            prune(tx, now_ms)?;
            let ids = ids(
                tx,
                "SELECT elicitation_id FROM elicitation_records \
                WHERE state='pending' AND expires_at_ms<=?1 ORDER BY expires_at_ms,elicitation_id",
                [now_ms],
            )?;
            let mut settlements = Vec::new();
            for id in ids {
                let mut record = load(tx, &id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                expire(tx, &mut record)?;
                settlements.push(record);
            }
            Ok(settlements)
        })
    }

    /// Each feed's rows and global high-water are one consistent read snapshot.
    pub fn answers(&self, owner: &str, since: i64, now_ms: i64) -> Result<Feed> {
        self.feed(Some(owner), since, now_ms)
    }

    /// Caller restricts this feed to reserved:prefrontal-core. Owner acks do not
    /// hide outcomes, but retention deletes acknowledged outcomes after 24 hours.
    pub fn outcomes(&self, since: i64, now_ms: i64) -> Result<Feed> {
        self.feed(None, since, now_ms)
    }

    fn feed(&self, owner: Option<&str>, since: i64, now_ms: i64) -> Result<Feed> {
        if since < 0 {
            return Err(refuse(
                "elicitation_invalid_request",
                "since must be nonnegative",
            ));
        }
        self.read(|conn| {
            let tx = conn.unchecked_transaction()?;
            let mut stmt = tx.prepare(
                "SELECT r.elicitation_id,a.settled_at_ms,r.record_json,a.cursor \
                 FROM elicitation_answers a JOIN elicitation_records r USING(elicitation_id) \
                 WHERE a.cursor>?1 AND (?2 IS NULL OR \
                 (a.owner=?2 AND r.acknowledged=0 AND r.state IN ('answered','expired'))) \
                 ORDER BY a.cursor",
            )?;
            let rows = stmt.query_map(params![since, owner], |row| Ok((
                 row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, Option<String>>(2)?, row.get::<_, i64>(3)?,
            )))?.collect::<std::result::Result<Vec<_>, _>>()?;
            drop(stmt);
            let mut cursor = tx.query_row(
                "SELECT COALESCE((SELECT seq FROM sqlite_sequence WHERE name='elicitation_answers'),0)",
                [], |row| row.get(0),
            )?;
            let mut records = Vec::new();
            let mut bytes = 0;
            let mut last_cursor = since;
            let mut continuation = None;
            for (id, at, raw, row_cursor) in rows {
                let value = match raw.filter(|_| now_ms.saturating_sub(at) < RETENTION_MS) {
                    Some(raw) => {
                        let record: ElicitationRecord = serde_json::from_str(&raw)?;
                        let mut value = record.requester_wire(None);
                        if owner.is_none() {
                            value.as_object_mut().expect("requester record object").remove("binding");
                        }
                        value
                    }
                    None => json!({"elicitation_id":id,"state":"expired","settled_at":at}),
                };
                // Both feeds keep package bodies. Their only readers are the card's
                // requester (answers) and prefrontal-core (outcomes), which check an answered
                // card against the one that was sent. One card is at most the 1.25 MiB request
                // cap, so it always fits a page; only phone-facing lists drop the bodies.
                let size = serde_json::to_vec(&value)?.len() + 1;
                if bytes + size > PAGE_RECORD_BYTES {
                    if records.is_empty() {
                        return Err(refuse("elicitation_invalid_request", "card projection exceeds page budget"));
                    }
                    // A page cursor may acknowledge only the rows actually returned.
                    cursor = last_cursor;
                    continuation = Some(last_cursor);
                    break;
                }
                bytes += size;
                last_cursor = row_cursor;
                records.push(value);
            }
            tx.commit()?;
            Ok(Feed { records, cursor, continuation })
        })
    }

    pub fn ack(&self, owner: &str, cursor: i64) -> Result<()> {
        if cursor < 0 {
            return Err(refuse(
                "elicitation_invalid_request",
                "cursor must be nonnegative",
            ));
        }
        self.transaction(|tx| {
            tx.execute(
                "UPDATE elicitation_records SET acknowledged=1 WHERE owner=?1 \
                 AND elicitation_id IN (SELECT elicitation_id FROM elicitation_answers \
                 WHERE owner=?1 AND cursor<=?2)",
                params![owner, cursor],
            )?;
            Ok(())
        })
    }
}

fn load(conn: &Connection, id: &str) -> Result<Option<ElicitationRecord>> {
    let raw: Option<Option<String>> = conn
        .query_row(
            "SELECT record_json FROM elicitation_records WHERE elicitation_id=?1",
            [id],
            |row| row.get(0),
        )
        .optional()?;
    raw.flatten()
        .map(|raw| serde_json::from_str(&raw).map_err(Error::from))
        .transpose()
}

fn retained(record: &ElicitationRecord, now_ms: i64) -> bool {
    record
        .answered_at_ms
        .is_none_or(|at| now_ms.saturating_sub(at) < RETENTION_MS)
}

fn visible(conn: &Connection, id: &str, now_ms: i64) -> Result<Option<ElicitationRecord>> {
    let acknowledged: Option<bool> = conn
        .query_row(
            "SELECT acknowledged FROM elicitation_records WHERE elicitation_id=?1",
            [id],
            |row| row.get(0),
        )
        .optional()?;
    if acknowledged == Some(true) {
        return Ok(None);
    }
    Ok(load(conn, id)?.filter(|record| retained(record, now_ms)))
}

fn owned(conn: &Connection, owner: &str, id: &str, now_ms: i64) -> Result<ElicitationRecord> {
    let stored_owner: Option<String> = conn
        .query_row(
            "SELECT owner FROM elicitation_records WHERE elicitation_id=?1",
            [id],
            |row| row.get(0),
        )
        .optional()?;
    if stored_owner.as_deref() != Some(owner) {
        return Err(refuse(
            "elicitation_not_owner",
            "elicitation has no matching owner",
        ));
    }
    visible(conn, id, now_ms)?
        .ok_or_else(|| refuse("elicitation_not_found", "elicitation is no longer retained"))
}

fn ids<P: rusqlite::Params>(conn: &Connection, sql: &str, params: P) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params, |row| row.get(0))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

fn same_content(old: &ElicitationRecord, request: &ConsentRequest, subject: &Subject) -> bool {
    let mut previous = old.request.clone();
    let mut next = request.clone();
    previous.dedup_key = None;
    next.dedup_key = None;
    // Reply correlation can change on a refresh without replacing the card.
    previous.requester_ref = None;
    next.requester_ref = None;
    previous.expires_in_ms = 0;
    next.expires_in_ms = 0;
    previous.requested_schema = None;
    next.requested_schema = None;
    previous == next
        && old.subject.agent_id == subject.agent_id
        && old.subject.head_agent_id == subject.head_agent_id
}

fn save(conn: &Connection, record: &ElicitationRecord) -> Result<()> {
    conn.execute(
        "UPDATE elicitation_records SET record_json=?2,state=?3,expires_at_ms=?4,settled_at_ms=?5 \
         WHERE elicitation_id=?1",
        params![
            record.elicitation_id,
            serde_json::to_string(record)?,
            record.state.as_str(),
            record.expires_at_ms,
            record.answered_at_ms
        ],
    )?;
    Ok(())
}

/// Called only on a pending record inside the inserting/settling transaction.
/// A unique answer id makes an inconsistent double settlement fail and roll back.
fn terminal(
    conn: &Connection,
    record: &mut ElicitationRecord,
    state: State,
    choice: Option<String>,
    at: i64,
) -> Result<()> {
    record.state = state;
    record.answered_choice_id = choice;
    record.answered_at_ms = Some(at);
    save(conn, record)?;
    conn.execute(
        "INSERT INTO elicitation_answers(elicitation_id,owner,settled_at_ms) \
         SELECT elicitation_id,owner,settled_at_ms FROM elicitation_records WHERE elicitation_id=?1",
        [&record.elicitation_id],
    )?;
    if record.detached {
        // A card settled after its requester stopped holding the call: queue the
        // late answer for the agent's session in this same transaction. The
        // (elicitation_id, purpose) key keeps it to one notice per card.
        if let Some(body) = record.late_answer_notice() {
            conn.execute(
                "INSERT OR IGNORE INTO access_notices(elicitation_id,purpose,body_json) \
                 VALUES (?1,?2,?3)",
                params![record.elicitation_id, LATE_ANSWER_PURPOSE, body],
            )?;
        }
    }
    Ok(())
}

fn expire(conn: &Connection, record: &mut ElicitationRecord) -> Result<()> {
    let choice = (record.request.kind == "flow_decision").then(|| record.request.default.clone());
    terminal(conn, record, State::Expired, choice, record.expires_at_ms)
}

fn prune(conn: &Connection, now_ms: i64) -> Result<()> {
    conn.execute(
        "UPDATE elicitation_records SET record_json=NULL WHERE settled_at_ms<=?1",
        [now_ms.saturating_sub(RETENTION_MS)],
    )?;
    // Delete answers explicitly: do not rely on foreign key enforcement, which
    // an out-of-band SQLite reader may disable when inspecting this database.
    conn.execute(
        "DELETE FROM elicitation_answers WHERE elicitation_id IN \
         (SELECT elicitation_id FROM elicitation_records WHERE acknowledged=1 AND record_json IS NULL)", [],
    )?;
    conn.execute(
        "DELETE FROM elicitation_records WHERE acknowledged=1 AND record_json IS NULL",
        [],
    )?;
    Ok(())
}
