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
/// How many change-log rows the store keeps for consent.watch_pending. A watch
/// cursor older than the retained rows cannot be replayed and gets a reread.
pub const CHANGE_LOG_ROWS: i64 = 4096;
pub const MIGRATIONS: [Migration; 7] = [
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
    Migration {
        version: 6,
        statements: include_str!("../migrations/006_optional_expiry.sql"),
    },
    Migration {
        version: 7,
        statements: include_str!("../migrations/007_change_log.sql"),
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
    change_log_rows: i64,
}

/// One card that changed after a watch position: its id and the version of its
/// latest change. Earlier changes to the same card are folded into this one,
/// because a watch item always carries the card's current projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub version: i64,
    pub elicitation_id: String,
}

/// The change log as one consistent read, for consent.watch_pending.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangePage {
    /// Names this store file, so a cursor from another store is recognized.
    pub instance: String,
    /// The newest version written; a cursor beyond it is unknown.
    pub head: i64,
    /// Versions at or below this were compacted away. A cursor below it cannot
    /// be replayed in full.
    pub pruned_through: i64,
    /// How many distinct cards changed after the requested version.
    pub backlog: i64,
    /// Up to the requested number of those cards, oldest latest change first.
    pub changes: Vec<Change>,
}

pub struct ExecutionPage {
    pub records: Vec<ElicitationRecord>,
    pub after: Option<String>,
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
        let store = Self {
            db,
            change_log_rows: CHANGE_LOG_ROWS,
        };
        let settlements = store.sweep(now_ms)?;
        Ok((store, settlements))
    }

    /// Change how many change-log rows each sweep keeps. Tests use a small
    /// value to make a watch cursor stale without writing thousands of rows.
    pub fn set_change_log_rows(&mut self, rows: i64) {
        self.change_log_rows = rows.max(0);
    }

    /// Rows this connection has written since it opened, counting trigger
    /// writes. The module compares it after each store use to wake watches
    /// only when something was written.
    pub fn write_count(&self) -> u64 {
        self.db
            .with_conn(|conn| Ok(conn.total_changes()))
            .unwrap_or_default()
    }

    /// Read the change log after `version` in one snapshot: the log's identity
    /// and bounds, the number of changed cards, and the first `limit` of them
    /// ordered by their latest change.
    pub fn changes_after(&self, version: i64, limit: usize) -> Result<ChangePage> {
        self.read(|conn| {
            let tx = conn.unchecked_transaction()?;
            let (instance, pruned_through): (String, i64) = tx.query_row(
                "SELECT instance,pruned_through FROM cingulate_change_log WHERE singleton=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            let head: i64 = tx.query_row(
                "SELECT COALESCE((SELECT seq FROM sqlite_sequence WHERE name='cingulate_changes'),0)",
                [],
                |row| row.get(0),
            )?;
            let backlog: i64 = tx.query_row(
                "SELECT COUNT(DISTINCT elicitation_id) FROM cingulate_changes WHERE version>?1",
                [version],
                |row| row.get(0),
            )?;
            let mut stmt = tx.prepare(
                "SELECT MAX(version),elicitation_id FROM cingulate_changes WHERE version>?1 \
                 GROUP BY elicitation_id ORDER BY 1 LIMIT ?2",
            )?;
            let changes = stmt
                .query_map(params![version, limit as i64], |row| {
                    Ok(Change {
                        version: row.get(0)?,
                        elicitation_id: row.get(1)?,
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            drop(stmt);
            tx.commit()?;
            Ok(ChangePage {
                instance,
                head,
                pruned_through,
                backlog,
                changes,
            })
        })
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
                if old.expires_at_ms.is_some_and(|expiry| expiry <= now_ms) {
                    expire(tx, &mut old)?;
                    settlements.push(old);
                } else if same_content(&old, &request, &identity.subject) {
                    old.request = request;
                    old.expires_at_ms = old
                        .request
                        .expires_in_ms
                        .map(|ttl| now_ms.saturating_add(ttl));
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
                expires_at_ms: request.expires_in_ms.map(|ttl| now_ms.saturating_add(ttl)),
                request,
                created_at_ms: now_ms,
                subject,
                requester: identity.requester.clone(),
                state: State::Pending,
                answered_at_ms: None,
                answered_choice_id: None,
                execution: None,
                execution_reason: None,
                detached: false,
                late: false,
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

    /// Read a retained phone card even after the requester acknowledges it.
    /// Requester `get` and `get_owned` reads still hide acknowledged records.
    pub fn get_phone(&self, id: &str, now_ms: i64) -> Result<Option<ElicitationRecord>> {
        self.read(|conn| Ok(load(conn, id)?.filter(|record| retained(record, now_ms))))
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
                WHERE state='pending' ORDER BY json_extract(record_json,'$.createdAtMs'),elicitation_id",
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
            if record.state == State::Pending
                && record.expires_at_ms.is_some_and(|expiry| expiry <= now_ms)
            {
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
                if record.expires_at_ms.is_some_and(|expiry| expiry <= now_ms) {
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
                if record.expires_at_ms.is_some_and(|expiry| expiry <= now_ms) {
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
        reason: Option<&str>,
        now_ms: i64,
    ) -> Result<ElicitationRecord> {
        self.transaction(|tx| {
            let mut record = owned_retained(tx, owner, id, now_ms)?;
            if reason.is_some_and(|reason| {
                execution != "approved_not_executed"
                    || reason.is_empty()
                    || reason.len() > 64
                    || !reason.as_bytes()[0].is_ascii_lowercase()
                    || !reason.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
            }) {
                return Err(refuse("elicitation_invalid_execution", "reason requires approved_not_executed and a lowercase snake_case code of 1 to 64 bytes"));
            }
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
            record.execution_reason = reason.map(str::to_owned);
            save(tx, &record)?;
            Ok(record)
        })
    }

    /// Inspect at most `limit` stored rows, returning eligible retained answers.
    /// Advance by the scanned id even for ineligible rows to bound work and avoid starvation.
    pub fn execution_candidates(
        &self,
        after: Option<&str>,
        limit: usize,
        now_ms: i64,
    ) -> Result<ExecutionPage> {
        self.read(|conn| {
            let ids = ids(
                conn,
                "SELECT elicitation_id FROM elicitation_records \
                 WHERE elicitation_id>COALESCE(?1,'') ORDER BY elicitation_id LIMIT ?2",
                params![after, limit as i64],
            )?;
            let mut records = Vec::new();
            for id in &ids {
                if let Some(record) = load(conn, id)?.filter(|record| {
                    retained(record, now_ms)
                        && record.request.kind == "consent"
                        && record.state == State::Answered
                        && record.asker.is_some()
                        && record.execution.is_none()
                }) {
                    records.push(record);
                }
            }
            Ok(ExecutionPage {
                records,
                after: ids.last().cloned(),
            })
        })
    }

    /// Recheck eligibility and install the backstop atomically with requester reports.
    pub fn execution_session_ended(&self, id: &str, now_ms: i64) -> Result<()> {
        self.transaction(|tx| {
            let Some(mut record) = load(tx, id)?.filter(|record| retained(record, now_ms)) else {
                return Ok(());
            };
            if record.request.kind == "consent"
                && record.state == State::Answered
                && record.asker.is_some()
                && record.execution.is_none()
            {
                record.execution = Some("approved_not_executed".into());
                record.execution_reason = Some("session_ended".into());
                save(tx, &record)?;
            }
            Ok(())
        })
    }

    /// Prune first, then settle every pending due card, including downtime
    /// expiries. Returned records represent only this transaction's settlements.
    pub fn sweep(&self, now_ms: i64) -> Result<Vec<ElicitationRecord>> {
        self.transaction(|tx| {
            prune(tx, now_ms, self.change_log_rows)?;
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
                        if owner.is_none() {
                            record.outcome_wire()
                        } else {
                            record.requester_wire(None)
                        }
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
    check_owner(conn, owner, id)?;
    visible(conn, id, now_ms)?
        .ok_or_else(|| refuse("elicitation_not_found", "elicitation is no longer retained"))
}

fn owned_retained(
    conn: &Connection,
    owner: &str,
    id: &str,
    now_ms: i64,
) -> Result<ElicitationRecord> {
    check_owner(conn, owner, id)?;
    load(conn, id)?
        .filter(|record| retained(record, now_ms))
        .ok_or_else(|| refuse("elicitation_not_found", "elicitation is no longer retained"))
}

fn check_owner(conn: &Connection, owner: &str, id: &str) -> Result<()> {
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
    Ok(())
}

/// Shared exponential retry delay for maintenance work; attempts are zero-based.
pub fn maintenance_backoff_ms(attempts: u32) -> i64 {
    (250_i64.saturating_mul(1_i64 << attempts.min(8))).min(60_000)
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
    previous.expires_in_ms = None;
    next.expires_in_ms = None;
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
    record.late = record.detached;
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
    terminal(
        conn,
        record,
        State::Expired,
        None,
        record.expires_at_ms.expect("only deadline cards expire"),
    )
}

fn prune(conn: &Connection, now_ms: i64, change_log_rows: i64) -> Result<()> {
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
    // Keep the newest change-log rows. Record the compaction boundary before
    // deleting, so a watch cursor inside the deleted range is detected as
    // stale and answered with a reread rather than a replay with gaps. Both
    // statements touch no row when nothing is due, so a routine sweep writes
    // nothing and does not wake open watches.
    conn.execute(
        "UPDATE cingulate_change_log SET pruned_through=MAX(pruned_through, \
         COALESCE((SELECT seq FROM sqlite_sequence WHERE name='cingulate_changes'),0)-?1) \
         WHERE singleton=1 AND pruned_through< \
         COALESCE((SELECT seq FROM sqlite_sequence WHERE name='cingulate_changes'),0)-?1",
        [change_log_rows],
    )?;
    conn.execute(
        "DELETE FROM cingulate_changes WHERE version<=\
         (SELECT pruned_through FROM cingulate_change_log WHERE singleton=1)",
        [],
    )?;
    Ok(())
}
