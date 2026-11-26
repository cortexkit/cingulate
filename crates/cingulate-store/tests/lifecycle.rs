use std::{
    collections::HashSet,
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Barrier,
    },
};

use cingulate_decode::ConsentRequest;
use cingulate_store::{
    storage_descriptor, AnswerOutcome, ConsentStore, ElicitationRecord, Isolation, RequestIdentity,
    State, StorageBackend, StorageDescriptor, MIGRATION_NAMESPACE, RETENTION_MS,
};
use rusqlite::{params, types::ValueRef, Connection, OptionalExtension};
use serde_json::{json, Value};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[path = "lifecycle/prefrontal.rs"]
mod prefrontal;

struct Scratch {
    root: PathBuf,
    descriptor: StorageDescriptor,
}

impl Scratch {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "cingulate-store-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&root).unwrap();
        let descriptor = StorageDescriptor {
            module_id: "cingulate".into(),
            storage_namespace: "test".into(),
            isolation: Isolation::Module,
            backend: StorageBackend::Sqlite {
                path: root.join("chosen.db").to_str().unwrap().into(),
            },
        };
        Self { root, descriptor }
    }

    fn sql(&self) -> Connection {
        let StorageBackend::Sqlite { path } = &self.descriptor.backend else {
            unreachable!()
        };
        Connection::open(path).unwrap()
    }

    fn open(&self, now: i64) -> (ConsentStore, Vec<ElicitationRecord>) {
        ConsentStore::open(&self.descriptor, now).unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn request() -> ConsentRequest {
    ConsentRequest::decode(
        serde_json::from_str(include_str!(
            "../../../test-vectors/elicitation-request-v1/valid/app-grant-once.json"
        ))
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn card_settled_outbox_exact_readers_sequences_and_idempotence() {
    let scratch = Scratch::new();
    let (store, _) = scratch.open(100);
    for (index, reader) in ["aft", "plexus", "basal", "broca", "cerebellum"]
        .iter()
        .enumerate()
    {
        let owner = identity(&format!("reserved:{reader}"), "stamped-session");
        let mut req = request();
        req.requester_ref = Some("reserved:broca private text".into());
        let id = store
            .create(&owner, req, 100)
            .unwrap()
            .record
            .elicitation_id;
        store.answer(&id, "allow_once", None, 101).unwrap();
        store.answer(&id, "allow_once", None, 102).unwrap();
        store.withdraw(&owner.owner, &id, 103).unwrap();
        let rows: Vec<_> = store
            .card_settled_pending(64)
            .unwrap()
            .into_iter()
            .filter(|row| row.elicitation_id == id)
            .collect();
        assert_eq!(rows.len(), if *reader == "cerebellum" { 1 } else { 2 });
        for row in rows {
            assert_eq!(
                row.seq,
                index as i64 + 1,
                "global answer cursor, not a reader-local count"
            );
            assert_eq!(row.msg_id(), format!("{id}:{}:{}", row.reader, row.seq));
            let body: Value = serde_json::from_slice(&row.body()).unwrap();
            assert_eq!(
                body,
                json!({"elicitation_id": id, "feed": if row.reader == "prefrontal" { "outcomes" } else { "answers" }, "seq": index + 1})
            );
            assert!(row.reader == "prefrontal" || row.reader == *reader);
        }
        assert_eq!(
            store.answers(&owner.owner, 0, 104).unwrap().cursor,
            index as i64 + 1
        );
    }
    drop(store);
    let (store, _) = scratch.open(104);
    assert_eq!(
        store.card_settled_pending(64).unwrap().len(),
        9,
        "pending survives reopen"
    );
}

#[test]
fn card_settled_outbox_rolls_back_with_failed_settlement() {
    let scratch = Scratch::new();
    let (store, _) = scratch.open(100);
    let id = store
        .create(&identity("reserved:aft", "s"), request(), 100)
        .unwrap()
        .record
        .elicitation_id;
    scratch.sql().execute_batch("CREATE TRIGGER fail_after_settle BEFORE INSERT ON card_settled_outbox WHEN NEW.reader='prefrontal' BEGIN SELECT RAISE(ABORT,'injected rollback'); END;").unwrap();
    assert!(store.answer(&id, "allow_once", None, 101).is_err());
    assert_eq!(store.get(&id, 102).unwrap().unwrap().state, State::Pending);
    assert!(
        store.card_settled_pending(64).unwrap().is_empty(),
        "including the first reader's inserted row"
    );
    assert!(store.outcomes(0, 102).unwrap().records.is_empty());
    scratch
        .sql()
        .execute_batch("DROP TRIGGER fail_after_settle")
        .unwrap();
    store.answer(&id, "allow_once", None, 103).unwrap();
    assert_eq!(store.card_settled_pending(64).unwrap().len(), 2);
}

#[test]
fn card_settled_outbox_expiry_and_withdrawal_match_feed_visibility() {
    let scratch = Scratch::new();
    let (store, _) = scratch.open(100);
    let owner = identity("reserved:broca", "s");
    let id = store
        .create(&owner, request(), 100)
        .unwrap()
        .record
        .elicitation_id;
    store.withdraw(&owner.owner, &id, 101).unwrap();
    let rows = store.card_settled_pending(64).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].reader, "prefrontal");
    let mut req = request();
    req.expires_in_ms = Some(10);
    let expired = store
        .create(&owner, req, 102)
        .unwrap()
        .record
        .elicitation_id;
    store.sweep(112).unwrap();
    let rows = store.card_settled_pending(64).unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows.iter()
            .filter(|row| row.elicitation_id == expired && row.seq == 2)
            .count(),
        2
    );
    assert_eq!(
        store.answers(&owner.owner, 0, 113).unwrap().records.len(),
        1
    );
    assert_eq!(store.outcomes(0, 113).unwrap().records.len(), 2);
}

#[test]
fn card_settled_outbox_ack_deletes_once_and_counters_survive_reopen() {
    let scratch = Scratch::new();
    let (store, _) = scratch.open(100);
    let id = store
        .create(&identity("reserved:aft", "s"), request(), 100)
        .unwrap()
        .record
        .elicitation_id;
    store.answer(&id, "allow_once", None, 101).unwrap();
    let row = store.card_settled_pending(64).unwrap().remove(0);
    assert_eq!(row.reader, "aft");
    store.card_settled_published(&row).unwrap();
    store.card_settled_published(&row).unwrap();
    store.answer(&id, "allow_once", None, 102).unwrap();
    drop(store);
    let (store, _) = scratch.open(103);
    let counts = store.card_settled_counts().unwrap();
    assert_eq!(counts[0].pending, 0);
    assert_eq!(counts[0].published, 1);
    assert_eq!(counts[4].pending, 1);
    assert_eq!(counts[4].published, 0);
}

#[test]
fn card_settled_outbox_cap_is_per_reader_and_preserves_newest() {
    use cingulate_store::CARD_SETTLED_PENDING_CAP;
    let scratch = Scratch::new();
    let (store, _) = scratch.open(100);
    scratch
        .sql()
        .execute(
            "WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<?1) \
         INSERT INTO card_settled_outbox SELECT 'old-'||x,'prefrontal','outcomes',x FROM n",
            [CARD_SETTLED_PENDING_CAP],
        )
        .unwrap();
    scratch
        .sql()
        .execute(
            "INSERT INTO card_settled_outbox VALUES ('other-reader','broca','answers',1)",
            [],
        )
        .unwrap();
    // The next real feed position follows the synthetic backlog.
    let owner = identity("reserved:aft", "s");
    let id = store
        .create(&owner, request(), 100)
        .unwrap()
        .record
        .elicitation_id;
    scratch
        .sql()
        .execute(
            "UPDATE sqlite_sequence SET seq=?1 WHERE name='elicitation_answers'",
            [CARD_SETTLED_PENDING_CAP],
        )
        .unwrap();
    store.answer(&id, "allow_once", None, 101).unwrap();
    let conn = scratch.sql();
    let counts = store.card_settled_counts().unwrap();
    assert_eq!(counts[4].pending, CARD_SETTLED_PENDING_CAP as u64);
    assert_eq!(counts[4].dropped, 1);
    assert_eq!(counts[3].pending, 1, "busy core cannot evict broca");
    assert_eq!(counts[0].pending, 1);
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM card_settled_outbox WHERE elicitation_id='old-1'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row(
            "SELECT seq FROM card_settled_outbox WHERE elicitation_id=?1 AND reader='prefrontal'",
            [&id],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        CARD_SETTLED_PENDING_CAP + 1
    );
    drop(store);
    let (store, _) = scratch.open(102);
    assert_eq!(store.card_settled_counts().unwrap()[4].dropped, 1);
}

fn identity(principal: &str, session: &str) -> RequestIdentity {
    RequestIdentity {
        owner: json!([principal, session]).to_string(),
        requester: principal.into(),
        subject: cingulate_store::Subject {
            agent_id: None,
            head_agent_id: String::new(),
            label: "an unverified local process".into(),
            via_worker: false,
            federated: None,
        },
        resolved_name: None,
        asker: None,
        head: None,
    }
}

fn flow_decision() -> ConsentRequest {
    let mut value = serde_json::to_value(request()).unwrap();
    value["kind"] = json!("flow_decision");
    value["flow_decision"] = json!({"arbitrary":"decision"});
    value["options"] = json!([
        {"id":"proceed","label":"Proceed","effect":"choose"},
        {"id":"decline","label":"Decline","effect":"decline"},
    ]);
    value["default"] = json!("decline");
    ConsentRequest::decode(value).unwrap()
}

fn rows(conn: &Connection, table: &str) -> Vec<Vec<Value>> {
    let mut stmt = conn
        .prepare(&format!("SELECT * FROM {table} ORDER BY 1"))
        .unwrap();
    let columns = stmt.column_count();
    stmt.query_map([], |row| {
        Ok((0..columns)
            .map(|i| match row.get_ref(i).unwrap() {
                ValueRef::Null => Value::Null,
                ValueRef::Integer(n) => json!(n),
                ValueRef::Real(n) => json!(n),
                ValueRef::Text(s) => json!(std::str::from_utf8(s).unwrap()),
                ValueRef::Blob(_) => panic!("no blobs in domain tables"),
            })
            .collect())
    })
    .unwrap()
    .collect::<rusqlite::Result<_>>()
    .unwrap()
}

#[derive(Debug, PartialEq)]
struct Snapshot {
    records: Vec<Vec<Value>>,
    answers: Vec<Vec<Value>>,
    id_seq: Vec<Vec<Value>>,
    pending_revision: Vec<Vec<Value>>,
}

fn snapshot(scratch: &Scratch) -> Snapshot {
    let conn = scratch.sql();
    Snapshot {
        records: rows(&conn, "elicitation_records"),
        answers: rows(&conn, "elicitation_answers"),
        id_seq: rows(&conn, "cingulate_id_seq"),
        pending_revision: rows(&conn, "cingulate_pending_revision"),
    }
}

fn assert_settlement(scratch: &Scratch, record: &ElicitationRecord, count: i64) {
    let conn = scratch.sql();
    let (state, record_at, answer_at, raw): (String, i64, i64, String) = conn
        .query_row(
            "SELECT r.state,r.settled_at_ms,a.settled_at_ms,r.record_json \
         FROM elicitation_records r JOIN elicitation_answers a USING(elicitation_id) \
         WHERE elicitation_id=?1",
            [&record.elicitation_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(state, record.state.as_str());
    assert_eq!(Some(record_at), record.answered_at_ms);
    assert_eq!(Some(answer_at), record.answered_at_ms);
    assert_eq!(
        serde_json::from_str::<ElicitationRecord>(&raw).unwrap(),
        *record
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM elicitation_answers", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        count
    );
    assert_eq!(
        conn.query_row(
            "SELECT SUM(notice_delivered) FROM elicitation_records",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}

#[test]
fn descriptor_path_wal_and_migrations_are_authoritative() {
    let scratch = Scratch::new();
    let data_home = scratch.root.join("unused-data-home");
    let descriptor = storage_descriptor(Some(&scratch.descriptor), data_home.to_str().unwrap());
    assert_eq!(descriptor, scratch.descriptor);
    let (store, settlements) = ConsentStore::open(&descriptor, 100).unwrap();
    assert!(settlements.is_empty());
    assert!(!data_home.exists());
    let conn = scratch.sql();
    assert_eq!(
        conn.pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0))
            .unwrap(),
        "wal"
    );
    assert_eq!(
        conn.query_row(
            "SELECT GROUP_CONCAT(version, ',') FROM cortexkit_schema_version WHERE namespace=?1",
            [MIGRATION_NAMESPACE],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        "1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18"
    );
    let answer_schema: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE name='elicitation_answers'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(answer_schema.contains("cursor INTEGER PRIMARY KEY AUTOINCREMENT"));
    assert_eq!(
        rows(&conn, "cingulate_id_seq"),
        vec![vec![json!(1), json!(0)]]
    );
    assert!(
        conn.execute("INSERT INTO cingulate_id_seq VALUES(2,0)", [])
            .is_err(),
        "counter is single-row"
    );
    drop(conn);
    drop(store);
    let (store, _) = scratch.open(100);
    assert_eq!(
        rows(&scratch.sql(), "cingulate_id_seq"),
        vec![vec![json!(1), json!(0)]]
    );
    drop(store);
}

#[test]
fn fallback_uses_supplied_scratch_data_home_and_never_overrides_errors() {
    let scratch = Scratch::new();
    let fallback = storage_descriptor(None, scratch.root.to_str().unwrap());
    assert_eq!(fallback.module_id, "cingulate");
    assert_eq!(
        fallback.backend,
        StorageBackend::Sqlite {
            path: format!(
                "{}/cortexkit/ck-cingulate/store.db",
                scratch.root.to_str().unwrap()
            )
        }
    );
    let (store, _) = ConsentStore::open(&fallback, 1).unwrap();
    assert!(scratch
        .root
        .join("cortexkit/ck-cingulate/store.db")
        .is_file());
    drop(store);
    let bad = StorageDescriptor {
        backend: StorageBackend::Postgres {
            dsn: "unavailable".into(),
            database: "none".into(),
        },
        ..scratch.descriptor.clone()
    };
    let selected = storage_descriptor(Some(&bad), scratch.root.to_str().unwrap());
    assert_eq!(selected, bad);
    let error = ConsentStore::open(&selected, 1).unwrap_err();
    assert!(!error.is_lease_held());
    assert!(error.to_string().contains("not supported"));
}

#[test]
fn only_live_lease_contention_is_retryable_and_release_allows_reopen() {
    let scratch = Scratch::new();
    let (held, _) = scratch.open(0);
    assert!(ConsentStore::open(&scratch.descriptor, 0)
        .unwrap_err()
        .is_lease_held());
    drop(held);
    let (reopened, _) = scratch.open(0);
    drop(reopened);
}

#[test]
fn newer_store_is_refused_before_serving_or_sweeping() {
    let scratch = Scratch::new();
    let (store, _) = scratch.open(0);
    store
        .create(&identity("direct", "s"), request(), 0)
        .unwrap();
    drop(store);
    scratch
        .sql()
        .execute(
            "INSERT INTO cortexkit_schema_version VALUES(?1,19,0)",
            [MIGRATION_NAMESPACE],
        )
        .unwrap();
    let before = snapshot(&scratch);
    let error = ConsentStore::open(&scratch.descriptor, i64::MAX).unwrap_err();
    assert!(error.to_string().contains("version 19"));
    assert!(error.to_string().contains("migration 18"));
    assert!(!error.is_lease_held());
    assert_eq!(snapshot(&scratch), before);
}

#[test]
fn durable_ids_do_not_reuse_any_value_after_settlement_prune_and_restart() {
    let scratch = Scratch::new();
    let owner = identity("direct", "s");
    let (store, _) = scratch.open(100);
    let mut seen = HashSet::new();
    for i in 0..4 {
        let created = store.create(&owner, request(), 100).unwrap();
        assert_eq!(created.record.elicitation_id, format!("el_{}", i + 1));
        assert!(seen.insert(created.record.elicitation_id.clone()));
        store
            .withdraw(&owner.owner, &created.record.elicitation_id, 101)
            .unwrap();
    }
    store.ack(&owner.owner, 4).unwrap();
    store.sweep(101 + RETENTION_MS).unwrap();
    assert!(rows(&scratch.sql(), "elicitation_records").is_empty());
    drop(store);
    let (store, _) = scratch.open(200 + RETENTION_MS);
    let new = store.create(&owner, request(), 200 + RETENTION_MS).unwrap();
    assert_eq!(new.record.elicitation_id, "el_5");
    assert!(!seen.contains(&new.record.elicitation_id));
    assert!(new.record.elicitation_id.starts_with("el_"));
    assert_eq!(
        store
            .answer("el_1", "allow_once", None, 200 + RETENTION_MS)
            .unwrap()
            .value,
        AnswerOutcome::NotFound
    );
    assert_eq!(
        store
            .get(&new.record.elicitation_id, 200 + RETENTION_MS)
            .unwrap()
            .unwrap()
            .state,
        State::Pending
    );
}

#[test]
fn first_terminal_state_wins_and_retries_add_no_log_rows() {
    let scratch = Scratch::new();
    let (store, _) = scratch.open(100);
    let owner = identity("direct", "s");
    let id = store
        .create(&owner, request(), 100)
        .unwrap()
        .record
        .elicitation_id;
    let answered = store.answer(&id, "allow_once", None, 101).unwrap();
    let AnswerOutcome::Answered(record) = answered.value else {
        panic!("fresh answer")
    };
    assert_eq!(answered.settlements, vec![record.clone()]);
    assert_eq!(record.answered_choice_id.as_deref(), Some("allow_once"));
    assert_settlement(&scratch, &record, 1);
    let before = snapshot(&scratch);
    let repeated = store.answer(&id, "allow_once", None, 102).unwrap();
    assert_eq!(
        repeated.value,
        AnswerOutcome::AlreadyAnswered(record.clone())
    );
    assert!(repeated.settlements.is_empty());
    let conflict = store.answer(&id, "deny", None, 103).unwrap();
    assert_eq!(conflict.value, AnswerOutcome::Conflict(record.clone()));
    assert!(conflict.settlements.is_empty());
    let withdrawn = store.withdraw(&owner.owner, &id, 104).unwrap();
    assert_eq!(withdrawn.value, record);
    assert!(withdrawn.settlements.is_empty());
    assert_eq!(snapshot(&scratch), before);
    let id2 = store
        .create(&owner, request(), 105)
        .unwrap()
        .record
        .elicitation_id;
    let withdrawn = store.withdraw(&owner.owner, &id2, 106).unwrap();
    assert_eq!(withdrawn.value.state, State::Withdrawn);
    assert_eq!(withdrawn.value.answered_choice_id, None);
    assert_settlement(&scratch, &withdrawn.value, 2);
    let before = snapshot(&scratch);
    assert!(store
        .withdraw(&owner.owner, &id2, 107)
        .unwrap()
        .settlements
        .is_empty());
    assert_eq!(
        store.answer(&id2, "allow_once", None, 107).unwrap().value,
        AnswerOutcome::Conflict(withdrawn.value)
    );
    assert_eq!(snapshot(&scratch), before);
}

#[test]
fn concurrent_answer_and_withdraw_commit_exactly_one_terminal_state() {
    let scratch = Scratch::new();
    let (store, _) = scratch.open(100);
    let owner = identity("direct", "s");
    let id = store
        .create(&owner, request(), 100)
        .unwrap()
        .record
        .elicitation_id;
    let store = Arc::new(store);
    let barrier = Arc::new(Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|i| {
            let store = store.clone();
            let barrier = barrier.clone();
            let id = id.clone();
            let owner = owner.owner.clone();
            std::thread::spawn(move || {
                barrier.wait();
                if i % 2 == 0 {
                    store
                        .answer(&id, "allow_once", None, 101)
                        .unwrap()
                        .settlements
                        .len()
                } else {
                    store.withdraw(&owner, &id, 101).unwrap().settlements.len()
                }
            })
        })
        .collect();
    assert_eq!(
        threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .sum::<usize>(),
        1
    );
    let record = store.get(&id, 102).unwrap().unwrap();
    assert!(matches!(record.state, State::Answered | State::Withdrawn));
    assert_settlement(&scratch, &record, 1);
}

#[test]
fn expiry_never_records_a_choice_for_any_kind() {
    let scratch = Scratch::new();
    let (store, _) = scratch.open(100);
    let owner = identity("direct", "s");
    let mut consent = request();
    consent.expires_in_ms = Some(5);
    consent.at_console = Some("m1".into());
    let record = store.create(&owner, consent, 100).unwrap().record;
    let result = store
        .answer(&record.elicitation_id, "not-an-option", None, 105)
        .unwrap();
    let AnswerOutcome::Conflict(expired) = result.value else {
        panic!("due answer conflicts")
    };
    assert_eq!(expired.state, State::Expired);
    assert_eq!(expired.answered_choice_id, None);
    assert_eq!(expired.answered_at_ms, Some(105));
    assert_eq!(result.settlements, vec![expired.clone()]);
    assert_settlement(&scratch, &expired, 1);
    let mut flow = flow_decision();
    flow.expires_in_ms = Some(2);
    let flow = store.create(&owner, flow, 106).unwrap().record;
    let result = store
        .answer(&flow.elicitation_id, "decline", None, 108)
        .unwrap();
    let AnswerOutcome::Conflict(expired) = result.value else {
        panic!("default is not a repeat answer")
    };
    assert_eq!(expired.answered_at_ms, Some(108));
    assert_eq!(expired.answered_choice_id, None);
    assert_settlement(&scratch, &expired, 2);
    let mut req = request();
    req.expires_in_ms = Some(1);
    let id = store
        .create(&owner, req, 109)
        .unwrap()
        .record
        .elicitation_id;
    let result = store.withdraw(&owner.owner, &id, 115).unwrap();
    assert_eq!(result.value.state, State::Expired);
    assert_eq!(result.value.answered_at_ms, Some(110));
    assert_settlement(&scratch, &result.value, 3);
}

#[test]
fn open_sweep_settles_downtime_expiry_exactly_once_for_all_kinds() {
    let scratch = Scratch::new();
    let (store, _) = scratch.open(10);
    let owner = identity("direct", "s");
    let mut requests = vec![request(), flow_decision()];
    let mut install = request();
    install.kind = "flow_install".into();
    install.flow_install = Some(serde_json::from_value(json!({"not_a_card":true})).unwrap());
    requests.push(install);
    for mut request in requests {
        request.expires_in_ms = Some(50);
        store.create(&owner, request, 10).unwrap();
    }
    drop(store);
    let (store, settlements) = scratch.open(1000);
    assert_eq!(settlements.len(), 3);
    for record in &settlements {
        assert_eq!(record.state, State::Expired);
        assert_eq!(record.answered_at_ms, Some(60));
        assert_eq!(record.answered_choice_id, None);
        assert_settlement(&scratch, record, 3);
    }
    assert!(store.pending().unwrap().is_empty());
    assert!(store.sweep(1000).unwrap().is_empty());
    drop(store);
    let (store, settlements) = scratch.open(1001);
    assert!(settlements.is_empty());
    assert_eq!(store.outcomes(0, 1001).unwrap().records.len(), 3);
}

#[test]
fn pending_choice_console_and_owner_refusals_write_nothing() {
    let scratch = Scratch::new();
    let (store, _) = scratch.open(100);
    let owner = identity("direct", "s");
    let mut req = request();
    req.at_console = Some("m1".into());
    let record = store.create(&owner, req, 100).unwrap().record;
    let before = snapshot(&scratch);
    assert_eq!(
        store
            .answer(&record.elicitation_id, "invalid", Some("m1"), 101)
            .unwrap_err()
            .code(),
        "elicitation_invalid_choice"
    );
    for machine in [None, Some("m2")] {
        assert_eq!(
            store
                .answer(&record.elicitation_id, "allow_once", machine, 101)
                .unwrap_err()
                .code(),
            "user_not_at_console"
        );
    }
    for id in ["never-stored", record.elicitation_id.as_str()] {
        assert_eq!(
            store.get_owned("wrong-owner", id, 101).unwrap_err().code(),
            "elicitation_not_owner"
        );
        assert_eq!(
            store.withdraw("wrong-owner", id, 101).unwrap_err().code(),
            "elicitation_not_owner"
        );
        assert_eq!(
            store
                .report_execution("wrong-owner", id, "executed", None, 101)
                .unwrap_err()
                .code(),
            "elicitation_not_owner"
        );
    }
    assert_eq!(snapshot(&scratch), before);
    assert!(matches!(
        store
            .answer(&record.elicitation_id, "allow_once", Some("m1"), 101)
            .unwrap()
            .value,
        AnswerOutcome::Answered(_)
    ));
}

#[test]
fn execution_updates_only_an_answered_non_decision_card_without_logging() {
    let scratch = Scratch::new();
    let (store, _) = scratch.open(100);
    let owner = identity("direct", "s");
    let id = store
        .create(&owner, request(), 100)
        .unwrap()
        .record
        .elicitation_id;
    let before = snapshot(&scratch);
    assert_eq!(
        store
            .report_execution(&owner.owner, &id, "not_run", None, 101)
            .unwrap_err()
            .code(),
        "elicitation_invalid_execution"
    );
    assert_eq!(
        store
            .report_execution(&owner.owner, &id, "executed", None, 101)
            .unwrap_err()
            .code(),
        "elicitation_store_error"
    );
    assert_eq!(snapshot(&scratch), before);
    store.answer(&id, "allow_once", None, 102).unwrap();
    let log = rows(&scratch.sql(), "elicitation_answers");
    for execution in ["executed", "approved_not_executed", "outcome_unknown"] {
        let record = store
            .report_execution(&owner.owner, &id, execution, None, 103)
            .unwrap();
        assert_eq!(record.execution.as_deref(), Some(execution));
        assert_eq!(store.get(&id, 103).unwrap().unwrap(), record);
        assert_eq!(rows(&scratch.sql(), "elicitation_answers"), log);
    }
    let id = store
        .create(&owner, flow_decision(), 104)
        .unwrap()
        .record
        .elicitation_id;
    store.answer(&id, "proceed", None, 105).unwrap();
    let before = snapshot(&scratch);
    assert_eq!(
        store
            .report_execution(&owner.owner, &id, "executed", None, 106)
            .unwrap_err()
            .code(),
        "elicitation_invalid_execution"
    );
    assert_eq!(snapshot(&scratch), before);
}

#[test]
fn dedup_refresh_ignores_expiry_schema_and_resolver_label_across_restart() {
    let scratch = Scratch::new();
    let mut owner = identity("reserved:plexus", "s");
    owner.subject.agent_id = Some("agent-alf".into());
    owner.subject.head_agent_id = "agent-alf".into();
    owner.subject.label = "agent-alf".into();
    let (store, _) = scratch.open(100);
    let mut req = request();
    req.dedup_key = Some("key".into());
    req.expires_in_ms = Some(60_000);
    let first = store.create(&owner, req.clone(), 100).unwrap();
    assert!(first.created);
    assert!(first.settlements.is_empty());
    let second = store.create(&owner, req.clone(), 100).unwrap();
    assert!(!second.created);
    assert_eq!(second.record, first.record);
    drop(store);
    let (store, _) = scratch.open(200);
    req.expires_in_ms = Some(120_000);
    req.requested_schema = Some(json!({"transient":"not stored"}));
    owner.resolved_name = Some("ALF".into());
    let refreshed = store.create(&owner, req.clone(), 200).unwrap();
    assert!(!refreshed.created);
    assert_eq!(refreshed.record.elicitation_id, first.record.elicitation_id);
    assert_eq!(refreshed.record.created_at_ms, 100);
    assert_eq!(refreshed.record.expires_at_ms, Some(120_200));
    assert_eq!(refreshed.record.request.expires_in_ms, Some(120_000));
    assert_eq!(refreshed.record.subject.label, "ALF");
    assert_eq!(refreshed.record.phone(None)["expiresInMs"], 120_000);
    assert!(refreshed.settlements.is_empty());
    owner.resolved_name = None;
    let refreshed = store.create(&owner, req, 300).unwrap();
    assert_eq!(refreshed.record.subject.label, "ALF");
    assert_eq!(refreshed.record.elicitation_id, first.record.elicitation_id);
    assert!(rows(&scratch.sql(), "elicitation_answers").is_empty());
    let raw: String = scratch
        .sql()
        .query_row("SELECT record_json FROM elicitation_records", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(!raw.contains("requestedSchema"));
    assert!(!raw.contains("requested_schema"));
}

#[test]
fn dedup_content_changes_withdraw_then_insert_new_content_atomically() {
    let cases: Vec<(&str, Value)> = vec![
        ("scope", json!({"changed":true})),
        ("args_digest", json!("changed")),
        ("binding", json!({"k":2})),
        ("at_console", json!("m1")),
        ("title", json!("Changed title")),
        ("preview", json!({"label":"Preview","text":"changed"})),
    ];
    for (field, value) in cases {
        let scratch = Scratch::new();
        let (store, _) = scratch.open(100);
        let owner = identity("direct", "s");
        let mut req = request();
        req.dedup_key = Some("key".into());
        let first = store.create(&owner, req.clone(), 100).unwrap().record;
        let mut body = serde_json::to_value(req).unwrap();
        body[field] = value;
        let changed = store
            .create(&owner, ConsentRequest::decode(body).unwrap(), 101)
            .unwrap();
        assert!(changed.created, "{field}");
        assert_ne!(
            changed.record.elicitation_id, first.elicitation_id,
            "{field}"
        );
        assert_eq!(changed.settlements.len(), 1, "{field}");
        assert_eq!(changed.settlements[0].state, State::Withdrawn, "{field}");
        assert_eq!(
            changed.settlements[0].elicitation_id, first.elicitation_id,
            "{field}"
        );
        assert_eq!(changed.settlements[0].answered_at_ms, Some(101));
        assert_settlement(&scratch, &changed.settlements[0], 1);
        assert!(matches!(
            store
                .answer(&first.elicitation_id, "allow_once", None, 102)
                .unwrap()
                .value,
            AnswerOutcome::Conflict(_)
        ));
        assert_eq!(
            store
                .get(&changed.record.elicitation_id, 102)
                .unwrap()
                .unwrap()
                .state,
            State::Pending
        );
    }
}

#[test]
fn dedup_flow_body_and_subject_id_changes_replace_even_when_labels_match() {
    for field in ["flow_install", "flow_decision", "agent_id", "head_agent_id"] {
        let scratch = Scratch::new();
        let (store, _) = scratch.open(100);
        let mut owner = identity("reserved:plexus", "s");
        let mut req = if field == "flow_decision" {
            flow_decision()
        } else {
            request()
        };
        if field == "flow_install" {
            req.kind = "flow_install".into();
            req.flow_install = Some(serde_json::from_value(json!({"old":true})).unwrap());
        }
        req.dedup_key = Some("key".into());
        let old = store.create(&owner, req.clone(), 100).unwrap().record;
        match field {
            "flow_install" => {
                req.flow_install = Some(serde_json::from_value(json!({"new":true})).unwrap())
            }
            "flow_decision" => {
                req.flow_decision = Some(serde_json::from_value(json!({"new":true})).unwrap())
            }
            "agent_id" => owner.subject.agent_id = Some("new-agent".into()),
            "head_agent_id" => owner.subject.head_agent_id = "new-head".into(),
            _ => unreachable!(),
        }
        let new = store.create(&owner, req, 101).unwrap();
        assert!(new.created, "{field}");
        assert_ne!(old.elicitation_id, new.record.elicitation_id, "{field}");
        assert_eq!(new.settlements[0].state, State::Withdrawn);
    }
}

#[test]
fn dedup_matches_only_pending_owner_key_and_no_key_always_inserts() {
    let scratch = Scratch::new();
    let (store, _) = scratch.open(100);
    let a = identity("direct", "session-a");
    let b = identity("direct", "session-b");
    let mut req = request();
    req.dedup_key = Some("key".into());
    let first = store.create(&a, req.clone(), 100).unwrap().record;
    let other = store.create(&b, req.clone(), 100).unwrap();
    assert!(other.created);
    assert_ne!(other.record.elicitation_id, first.elicitation_id);
    store
        .answer(&first.elicitation_id, "allow_once", None, 101)
        .unwrap();
    let settled = store.get(&first.elicitation_id, 101).unwrap().unwrap();
    let new = store.create(&a, req, 102).unwrap();
    assert!(new.created);
    assert!(new.settlements.is_empty());
    assert_ne!(new.record.elicitation_id, first.elicitation_id);
    assert_eq!(
        store.get(&first.elicitation_id, 102).unwrap().unwrap(),
        settled
    );
    let first = store.create(&a, request(), 103).unwrap();
    let second = store.create(&a, request(), 103).unwrap();
    assert_ne!(first.record.elicitation_id, second.record.elicitation_id);
    assert!(first.created && second.created);
}

#[test]
fn due_dedup_match_expires_and_replaces_but_request_never_sweeps_unrelated_cards() {
    let scratch = Scratch::new();
    let (store, _) = scratch.open(100);
    let owner = identity("direct", "s");
    let mut req = request();
    req.expires_in_ms = Some(10);
    let unrelated = store.create(&owner, req.clone(), 100).unwrap().record;
    req.dedup_key = Some("key".into());
    let due = store.create(&owner, req.clone(), 100).unwrap().record;
    let new = store.create(&owner, req, 110).unwrap();
    assert!(new.created);
    assert_ne!(new.record.elicitation_id, due.elicitation_id);
    assert_eq!(new.settlements.len(), 1);
    assert_eq!(new.settlements[0].elicitation_id, due.elicitation_id);
    assert_eq!(new.settlements[0].state, State::Expired);
    assert_eq!(new.settlements[0].answered_at_ms, Some(110));
    assert_eq!(new.record.created_at_ms, 110);
    assert_settlement(&scratch, &new.settlements[0], 1);
    assert_eq!(
        scratch
            .sql()
            .query_row(
                "SELECT state FROM elicitation_records WHERE elicitation_id=?1",
                [unrelated.elicitation_id],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
        "pending"
    );
}

#[test]
fn failing_replacement_insert_rolls_back_settlement_counter_and_old_record() {
    let scratch = Scratch::new();
    let (store, _) = scratch.open(100);
    let owner = identity("direct", "s");
    let mut req = request();
    req.dedup_key = Some("key".into());
    req.expires_in_ms = Some(10);
    let old = store.create(&owner, req.clone(), 100).unwrap().record;
    scratch.sql().execute_batch("CREATE TRIGGER fail_insert BEFORE INSERT ON elicitation_records BEGIN SELECT RAISE(ABORT,'injected insert failure'); END;").unwrap();
    let before = snapshot(&scratch);
    for now in [101, 110] {
        let mut changed = req.clone();
        changed.title = "changed".into();
        assert!(store
            .create(&owner, changed, now)
            .unwrap_err()
            .to_string()
            .contains("injected insert failure"));
        assert_eq!(
            snapshot(&scratch),
            before,
            "rollback includes old settlement and id increment"
        );
    }
    scratch
        .sql()
        .execute_batch("DROP TRIGGER fail_insert")
        .unwrap();
    let new = store.create(&owner, req, 110).unwrap();
    assert_eq!(new.record.elicitation_id, "el_2");
    assert_eq!(new.settlements[0].elicitation_id, old.elicitation_id);
}

#[test]
fn failing_answer_log_insert_rolls_back_record_and_all_sweep_work() {
    let scratch = Scratch::new();
    let (store, _) = scratch.open(100);
    let owner = identity("direct", "s");
    let mut req = request();
    req.expires_in_ms = Some(10);
    let id = store
        .create(&owner, req.clone(), 100)
        .unwrap()
        .record
        .elicitation_id;
    store.create(&owner, req, 100).unwrap();
    scratch.sql().execute_batch("CREATE TRIGGER fail_log BEFORE INSERT ON elicitation_answers BEGIN SELECT RAISE(ABORT,'injected log failure'); END;").unwrap();
    let before = snapshot(&scratch);
    assert!(store.answer(&id, "allow_once", None, 101).is_err());
    assert_eq!(snapshot(&scratch), before);
    assert!(store.sweep(110).is_err());
    assert_eq!(snapshot(&scratch), before);
    scratch
        .sql()
        .execute_batch("DROP TRIGGER fail_log")
        .unwrap();
    assert_eq!(store.sweep(110).unwrap().len(), 2);
}

#[test]
fn answers_and_outcomes_order_filter_and_global_high_water_survive_ack_and_restart() {
    let scratch = Scratch::new();
    let (store, _) = scratch.open(100);
    let owners = [
        identity("direct", "s"),
        identity("reserved:basal", "s"),
        identity("reserved:plexus", "s"),
    ];
    let mut req = request();
    req.binding = Some(json!({"private":true}));
    let a = store.create(&owners[0], req.clone(), 100).unwrap().record;
    let mut expiring = flow_decision();
    expiring.expires_in_ms = Some(1);
    let b = store.create(&owners[1], expiring, 100).unwrap().record;
    let c = store.create(&owners[2], req, 100).unwrap().record;
    store
        .answer(&a.elicitation_id, "allow_once", None, 100)
        .unwrap();
    store.sweep(101).unwrap();
    store
        .withdraw(&owners[2].owner, &c.elicitation_id, 101)
        .unwrap();
    let outcomes = store.outcomes(0, 101).unwrap();
    assert_eq!(outcomes.cursor, 3);
    assert_eq!(
        outcomes
            .records
            .iter()
            .map(|row| row["elicitation_id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec![
            a.elicitation_id.as_str(),
            b.elicitation_id.as_str(),
            c.elicitation_id.as_str()
        ]
    );
    for record in &outcomes.records {
        assert!(record.get("requester_label").is_some());
        assert!(record.get("binding").is_none());
    }
    assert_eq!(
        store.answers(&owners[0].owner, 0, 101).unwrap().records[0]["binding"],
        json!({"private":true})
    );
    assert!(
        store
            .answers(&owners[2].owner, 0, 101)
            .unwrap()
            .records
            .is_empty(),
        "withdrawals excluded from owner feed"
    );
    assert_eq!(store.answers("no-owner", 0, 101).unwrap().cursor, 3);
    assert!(store.outcomes(3, 101).unwrap().records.is_empty());
    for owner in &owners {
        store.ack(&owner.owner, 3).unwrap();
    }
    assert!(store.get(&a.elicitation_id, 101).unwrap().is_none());
    assert_eq!(
        store
            .get_owned(&owners[0].owner, &a.elicitation_id, 101)
            .unwrap_err()
            .code(),
        "elicitation_not_found"
    );
    assert!(store
        .answers(&owners[0].owner, 0, 101)
        .unwrap()
        .records
        .is_empty());
    assert_eq!(store.outcomes(0, 101).unwrap(), outcomes);
    drop(store);
    let (store, settlements) = scratch.open(102);
    assert!(settlements.is_empty());
    assert_eq!(store.outcomes(0, 102).unwrap(), outcomes);
    store.sweep(101 + RETENTION_MS).unwrap();
    assert_eq!(store.outcomes(3, 101 + RETENTION_MS).unwrap().cursor, 3);
    assert!(store
        .outcomes(0, 101 + RETENTION_MS)
        .unwrap()
        .records
        .is_empty());
    drop(store);
    let (store, _) = scratch.open(102 + RETENTION_MS);
    let empty = store.outcomes(3, 102 + RETENTION_MS).unwrap();
    assert!(empty.records.is_empty());
    assert_eq!(empty.cursor, 3);
    let next = store
        .create(&owners[0], request(), 102 + RETENTION_MS)
        .unwrap()
        .record;
    store
        .answer(&next.elicitation_id, "allow_once", None, 103 + RETENTION_MS)
        .unwrap();
    let feed = store.outcomes(3, 103 + RETENTION_MS).unwrap();
    assert_eq!(feed.cursor, 4);
    assert_eq!(feed.records.len(), 1);
    assert_eq!(feed.records[0]["elicitation_id"], next.elicitation_id);
    assert!(store
        .outcomes(feed.cursor, 103 + RETENTION_MS)
        .unwrap()
        .records
        .is_empty());
}

#[test]
fn ack_is_cumulative_owner_scoped_and_includes_withdrawals_only_up_to_cursor() {
    let scratch = Scratch::new();
    let (store, _) = scratch.open(100);
    let a = identity("direct", "a");
    let b = identity("direct", "b");
    let first = store.create(&a, request(), 100).unwrap().record;
    let second = store.create(&b, request(), 100).unwrap().record;
    let third = store.create(&a, request(), 100).unwrap().record;
    store
        .withdraw(&a.owner, &first.elicitation_id, 101)
        .unwrap();
    store
        .answer(&second.elicitation_id, "allow_once", None, 101)
        .unwrap();
    store
        .answer(&third.elicitation_id, "allow_once", None, 101)
        .unwrap();
    store.ack(&a.owner, 2).unwrap();
    assert!(store.get(&first.elicitation_id, 101).unwrap().is_none());
    assert!(store.get(&second.elicitation_id, 101).unwrap().is_some());
    assert!(store.get(&third.elicitation_id, 101).unwrap().is_some());
    let before = snapshot(&scratch);
    assert_eq!(
        store.ack(&a.owner, -1).unwrap_err().code(),
        "elicitation_invalid_request"
    );
    assert_eq!(
        store.answers(&a.owner, -1, 101).unwrap_err().code(),
        "elicitation_invalid_request"
    );
    assert_eq!(
        store.outcomes(-1, 101).unwrap_err().code(),
        "elicitation_invalid_request"
    );
    assert_eq!(snapshot(&scratch), before);
}

#[test]
fn retention_exact_bound_builds_tombstones_at_read_time_without_changing_the_log() {
    let scratch = Scratch::new();
    let (store, _) = scratch.open(100);
    let owner = identity("direct", "s");
    let answered = store.create(&owner, request(), 100).unwrap().record;
    let withdrawn = store.create(&owner, request(), 100).unwrap().record;
    store
        .answer(&answered.elicitation_id, "allow_once", None, 101)
        .unwrap();
    store
        .withdraw(&owner.owner, &withdrawn.elicitation_id, 101)
        .unwrap();
    let log = rows(&scratch.sql(), "elicitation_answers");
    store.sweep(100 + RETENTION_MS).unwrap();
    assert!(store
        .answers(&owner.owner, 0, 100 + RETENTION_MS)
        .unwrap()
        .records[0]
        .get("title")
        .is_some());
    assert!(store
        .get(&answered.elicitation_id, 100 + RETENTION_MS)
        .unwrap()
        .is_some());
    store.sweep(101 + RETENTION_MS).unwrap();
    assert_eq!(
        scratch
            .sql()
            .query_row(
                "SELECT COUNT(*) FROM elicitation_records WHERE record_json IS NULL",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        2
    );
    let answer_tombstone =
        json!({"elicitation_id":answered.elicitation_id,"state":"expired","settled_at":101});
    let withdraw_tombstone =
        json!({"elicitation_id":withdrawn.elicitation_id,"state":"expired","settled_at":101});
    assert_eq!(
        store
            .answers(&owner.owner, 0, 101 + RETENTION_MS)
            .unwrap()
            .records,
        vec![answer_tombstone.clone()]
    );
    assert_eq!(
        store.outcomes(0, 101 + RETENTION_MS).unwrap().records,
        vec![answer_tombstone, withdraw_tombstone]
    );
    assert_eq!(rows(&scratch.sql(), "elicitation_answers"), log);
    assert!(store
        .get(&answered.elicitation_id, 101 + RETENTION_MS)
        .unwrap()
        .is_none());
    let before = snapshot(&scratch);
    let result = store
        .answer(
            &answered.elicitation_id,
            "allow_once",
            None,
            101 + RETENTION_MS,
        )
        .unwrap();
    assert_eq!(result.value, AnswerOutcome::NotFound);
    assert!(result.settlements.is_empty());
    assert_eq!(snapshot(&scratch), before);
    drop(store);
    let (store, settlements) = scratch.open(102 + RETENTION_MS);
    assert!(settlements.is_empty());
    assert_eq!(
        store
            .answers(&owner.owner, 0, 102 + RETENTION_MS)
            .unwrap()
            .records
            .len(),
        1
    );
}

#[test]
fn prune_deletes_acknowledged_answers_explicitly_before_record_deletion() {
    let scratch = Scratch::new();
    let (store, _) = scratch.open(100);
    let owner = identity("direct", "s");
    let record = store.create(&owner, request(), 100).unwrap().record;
    store
        .answer(&record.elicitation_id, "allow_once", None, 101)
        .unwrap();
    store.ack(&owner.owner, 1).unwrap();
    // The trigger observes the order even though the shared-store connection
    // normally enables foreign keys: cascade must never do this deletion for us.
    scratch.sql().execute_batch("CREATE TRIGGER require_explicit_answer_delete BEFORE DELETE ON elicitation_records WHEN EXISTS (SELECT 1 FROM elicitation_answers WHERE elicitation_id=OLD.elicitation_id) BEGIN SELECT RAISE(ABORT,'answer must be deleted first'); END;").unwrap();
    assert!(store.sweep(101 + RETENTION_MS).unwrap().is_empty());
    assert!(rows(&scratch.sql(), "elicitation_records").is_empty());
    assert!(rows(&scratch.sql(), "elicitation_answers").is_empty());
    assert_eq!(store.outcomes(0, 101 + RETENTION_MS).unwrap().cursor, 1);
}

#[test]
fn prune_delete_failure_rolls_back_nulling_and_explicit_answer_delete_together() {
    let scratch = Scratch::new();
    let (store, _) = scratch.open(100);
    let owner = identity("direct", "s");
    let record = store.create(&owner, request(), 100).unwrap().record;
    store
        .answer(&record.elicitation_id, "allow_once", None, 101)
        .unwrap();
    store.ack(&owner.owner, 1).unwrap();
    scratch.sql().execute_batch("CREATE TRIGGER fail_prune BEFORE DELETE ON elicitation_records BEGIN SELECT RAISE(ABORT,'injected prune failure'); END;").unwrap();
    let before = snapshot(&scratch);
    assert!(store
        .sweep(101 + RETENTION_MS)
        .unwrap_err()
        .to_string()
        .contains("injected prune failure"));
    assert_eq!(snapshot(&scratch), before);
    scratch
        .sql()
        .execute_batch("DROP TRIGGER fail_prune")
        .unwrap();
    store.sweep(101 + RETENTION_MS).unwrap();
    assert!(rows(&scratch.sql(), "elicitation_answers").is_empty());
}

#[test]
fn pending_order_and_saturating_expiry_are_durable() {
    let scratch = Scratch::new();
    let (store, _) = scratch.open(100);
    let owner = identity("direct", "s");
    let mut longer = request();
    longer.expires_in_ms = Some(200);
    let a = store.create(&owner, longer, 100).unwrap().record;
    let mut shorter = request();
    shorter.expires_in_ms = Some(100);
    let b = store.create(&owner, shorter.clone(), 100).unwrap().record;
    let c = store.create(&owner, shorter, 100).unwrap().record;
    assert_eq!(
        store
            .pending()
            .unwrap()
            .iter()
            .map(|r| &r.elicitation_id)
            .collect::<Vec<_>>(),
        vec![&a.elicitation_id, &b.elicitation_id, &c.elicitation_id]
    );
    let near_limit = store
        .create(&owner, request(), i64::MAX - 1)
        .unwrap()
        .record;
    assert_eq!(near_limit.expires_at_ms, Some(i64::MAX));
    assert!(matches!(
        store
            .answer(&near_limit.elicitation_id, "allow_once", None, i64::MAX)
            .unwrap()
            .value,
        AnswerOutcome::Conflict(_)
    ));
}

#[test]
fn record_projections_preserve_opaque_content_but_do_not_leak_binding_to_phone() {
    let scratch = Scratch::new();
    let (store, _) = scratch.open(100);
    let mut owner = identity("reserved:plexus", "s");
    owner.subject.agent_id = Some("worker".into());
    owner.subject.head_agent_id = "worker".into();
    owner.subject.label = "WORKER".into();
    let mut req = request();
    req.scope = Some(json!({"MixedCase":{"inside_key":true}}));
    req.binding = Some(json!({"nestedOpaque":{"KeepCase":1}}));
    req.at_console = Some("m1".into());
    req.preview = Some(cingulate_decode::Preview {
        label: "Preview".into(),
        text: "Some text".into(),
    });
    req.options[0].detail = Some("More detail".into());
    let record = store.create(&owner, req, 100).unwrap().record;
    let phone = record.phone(Some(i64::MAX));
    assert_eq!(phone["requesterLabel"], "reserved:plexus");
    assert_eq!(phone["subject"]["headAgentId"], "worker");
    assert_eq!(phone["atConsole"], "m1");
    assert_eq!(phone["blockUntilMs"], record.expires_at_ms.unwrap());
    assert_eq!(phone["scope"], json!({"MixedCase":{"inside_key":true}}));
    assert_eq!(
        phone["preview"],
        json!({"label":"Preview","text":"Some text"})
    );
    assert_eq!(phone["options"][0]["detail"], "More detail");
    for key in [
        "binding",
        "request",
        "requester",
        "elicitation_id",
        "requestedSchema",
        "requested_schema",
    ] {
        assert!(phone.get(key).is_none(), "{key}");
    }
    let requester = record.requester_wire(Some(101));
    assert_eq!(requester["requester_label"], "reserved:plexus");
    assert_eq!(requester["subject"]["head_agent_id"], "worker");
    assert_eq!(requester["binding"], json!({"nestedOpaque":{"KeepCase":1}}));
    assert_eq!(requester["at_console"], "m1");
    assert_eq!(requester["block_until_ms"], 101);
    assert!(requester.get("elicitationId").is_none());
    assert!(requester.get("requester").is_none());
    let result = store
        .withdraw(&owner.owner, &record.elicitation_id, 102)
        .unwrap();
    assert!(result.value.phone(Some(103)).get("blockUntilMs").is_none());
    assert!(result
        .value
        .requester_wire(None)
        .get("block_until_ms")
        .is_none());
    // Binding and console are durable inside record_json, not extra columns.
    let raw: String = scratch
        .sql()
        .query_row(
            "SELECT record_json FROM elicitation_records WHERE elicitation_id=?1",
            params![record.elicitation_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<ElicitationRecord>(&raw).unwrap(),
        result.value
    );
}

#[test]
fn pending_revision_migrates_existing_cards_without_rewriting_them() {
    let scratch = Scratch::new();
    let owner = identity("direct", "s");
    let (store, _) = scratch.open(100);
    let record = store.create(&owner, request(), 100).unwrap().record;
    drop(store);
    let before = rows(&scratch.sql(), "elicitation_records");
    scratch
        .sql()
        .execute_batch(
            "DROP TABLE card_settled_outbox;
        DROP TABLE card_settled_counters;
        ALTER TABLE elicitation_answers DROP COLUMN sealed_inputs_json;
        ALTER TABLE elicitation_answers DROP COLUMN sealed_answer_sha256;
        DROP TRIGGER cingulate_change_card_insert;
        DROP TRIGGER cingulate_change_card_update;
         DROP TRIGGER cingulate_change_card_delete;
         DROP TABLE sandbox_escape_requests;
         ALTER TABLE elicitation_records DROP COLUMN sandbox_escape;
         DROP TABLE broker_action_requests;
         ALTER TABLE elicitation_records DROP COLUMN broker_action;
         DROP TABLE config_plan_audit;
        ALTER TABLE elicitation_records DROP COLUMN config_plan;
        DROP TABLE manifest_binding_requests;
        ALTER TABLE elicitation_records DROP COLUMN manifest_binding;
        DROP TABLE secret_use_requests;
        ALTER TABLE elicitation_records DROP COLUMN secret_use;
        DROP TABLE cingulate_changes;
        DROP TABLE cingulate_change_log;
        DROP TRIGGER access_insert_revision;
        DROP TRIGGER access_update_revision;
        DROP TABLE access_notices;
        DROP TABLE access_asks;
        DELETE FROM cortexkit_schema_version WHERE namespace='cingulate' AND version>=5;
        DELETE FROM cortexkit_schema_version WHERE namespace='cingulate' AND version=4;
        DROP TRIGGER cingulate_pending_insert;
        DROP TRIGGER cingulate_pending_update;
        DROP TRIGGER cingulate_pending_delete;
        DROP TABLE cingulate_pending_revision;
        DELETE FROM cortexkit_schema_version WHERE namespace='cingulate' AND version=3;",
        )
        .unwrap();
    let (store, settlements) = scratch.open(100);
    assert!(settlements.is_empty());
    assert_eq!(rows(&scratch.sql(), "elicitation_records"), before);
    assert_eq!(store.pending_revision().unwrap(), 0);
    store
        .answer(&record.elicitation_id, "allow_once", None, 101)
        .unwrap();
    assert_eq!(store.pending_revision().unwrap(), 1);
    drop(store);
    let (store, _) = scratch.open(102);
    assert_eq!(store.pending_revision().unwrap(), 1);
}

#[test]
fn pending_revision_exhaustion_rolls_back_instead_of_reusing_a_revision() {
    let scratch = Scratch::new();
    let (store, _) = scratch.open(100);
    scratch
        .sql()
        .execute("UPDATE cingulate_pending_revision SET value=?1", [i64::MAX])
        .unwrap();
    let before = snapshot(&scratch);
    assert!(store
        .create(&identity("direct", "s"), request(), 100)
        .is_err());
    assert_eq!(snapshot(&scratch), before);
    assert_eq!(store.pending_revision().unwrap(), i64::MAX);
}

#[test]
fn access_migration_applies_to_version_three_without_changing_existing_cards() {
    let scratch = Scratch::new();
    let old = cortexkit_store::open_sqlite(&scratch.descriptor).unwrap();
    old.migrate(MIGRATION_NAMESPACE, &cingulate_store::MIGRATIONS[..3])
        .unwrap();
    let legacy = ElicitationRecord {
        elicitation_id: "el_1".into(),
        request: request(),
        created_at_ms: 100,
        expires_at_ms: Some(50_000),
        subject: identity("direct", "s").subject,
        requester: "direct".into(),
        state: State::Pending,
        answered_at_ms: None,
        answered_device: None,
        answered_surface: None,
        secret_use_answer: None,
        manifest_binding_answer: None,
        broker_action_answer: None,
        input_supplied: false,
        sealed_inputs: None,
        answer_seq: None,
        statement: None,
        signature_b64url: None,
        unverified: None,
        notice_round: 0,
        answered_choice_id: None,
        execution: None,
        execution_reason: None,
        detached: false,
        late: false,
        asker: None,
        head: None,
    };
    old.with_conn(|conn| {
        conn.execute("INSERT INTO elicitation_records(elicitation_id,owner,record_json,state,expires_at_ms) VALUES (?1,?2,?3,'pending',50000)",params![legacy.elicitation_id,identity("direct","s").owner,serde_json::to_string(&legacy).unwrap()])?;
        conn.execute("UPDATE cingulate_id_seq SET value=1",[])?;
        Ok(())
    }).unwrap();
    drop(old);
    let mut before = rows(&scratch.sql(), "elicitation_records");
    for row in &mut before {
        // The later kind-visibility columns default to false for consent.
        row.extend((0..5).map(|_| json!(0)));
    }
    let (store, settlements) = scratch.open(100);
    assert!(settlements.is_empty());
    assert_eq!(rows(&scratch.sql(), "elicitation_records"), before);
    let identity = cingulate_store::AccessIdentity {
        principal: "reserved:aft".into(),
        scope_owner: Some("reserved:prefrontal-core".into()),
        asker: cingulate_store::ScopeIdentity {
            scope_ref: "head".into(),
            scope_epoch: 1,
            agent_id: Some("agent".into()),
        },
        scope_kind: "head".into(),
        harness: "test".into(),
        head: None,
    };
    let access = cingulate_decode::AccessRequest::decode(json!({"kind":"tool_permission","call_key":"key","tool":"read","operation":"read","paths":["/tmp/file"],"reason":"Read file","args_hash":"hash","pattern":"/tmp/file"})).unwrap();
    let record = store
        .access_create(identity.clone(), access.clone(), 100)
        .unwrap()
        .record;
    assert_eq!(record.elicitation_id, "acc_2");
    drop(store);
    let (store, _) = scratch.open(200);
    assert_eq!(
        store
            .access_create(identity, access, 200)
            .unwrap()
            .record
            .elicitation_id,
        "acc_2"
    );
    // The legacy pending card has no stamped scope, so opening the upgraded
    // store gives it its one person notice; the access ask keeps its own.
    let mut notices: Vec<_> = store
        .access_notices(200)
        .unwrap()
        .into_iter()
        .map(|notice| (notice.elicitation_id, notice.purpose))
        .collect();
    notices.sort();
    assert_eq!(
        notices,
        vec![
            ("acc_2".to_string(), "user_ask".to_string()),
            ("el_1".to_string(), "escalated".to_string()),
        ]
    );
}

#[test]
fn development_local_operator_permission_answer_requires_explicit_authority() {
    let scratch = Scratch::new();
    let (store, _) = scratch.open(100);
    let identity = cingulate_store::AccessIdentity {
        principal: "reserved:aft".into(),
        scope_owner: Some("reserved:prefrontal-core".into()),
        asker: cingulate_store::ScopeIdentity {
            scope_ref: "head".into(),
            scope_epoch: 1,
            agent_id: Some("agent".into()),
        },
        scope_kind: "head".into(),
        harness: "test".into(),
        head: None,
    };
    let request = cingulate_decode::AccessRequest::decode(json!({
        "kind":"tool_permission", "call_key":"local-user", "tool":"read",
        "operation":"read", "paths":["/scratch/file"], "reason":"Read file",
        "args_hash":"hash", "pattern":"/scratch/file"
    }))
    .unwrap();
    let id = store
        .access_create(identity, request, 100)
        .unwrap()
        .record
        .elicitation_id;
    let answer = || cingulate_store::AccessAnswer {
        via: None,
        surface: "user".into(),
        principal: "direct".into(),
        answer: "allow_once".into(),
        note: None,
        answered_by: None,
    };
    let refused = store
        .access_answer_from_operator(&id, answer(), None, false, 101)
        .unwrap()
        .unwrap();
    assert_eq!(refused.code.as_deref(), Some("access_wrong_leg"));
    assert!(!refused.changed);
    assert_eq!(refused.record.state, "pending:user");
    let accepted = store
        .access_answer_from_operator(&id, answer(), None, true, 102)
        .unwrap()
        .unwrap();
    assert_eq!(accepted.code, None);
    assert!(accepted.changed);
    assert_eq!(accepted.record.state, "settled");
    assert_eq!(accepted.record.outcome.as_deref(), Some("allow_once"));
    for (attempt, result) in accepted
        .record
        .attempts
        .iter()
        .zip(["access_wrong_leg", "accepted"])
    {
        assert_eq!(attempt.principal, "direct");
        assert_eq!(attempt.surface, "user");
        assert_eq!(attempt.device, None);
        assert_eq!(attempt.result, result);
    }
    assert_eq!(accepted.record.attempts.len(), 2);
}

#[test]
fn no_deadline_survives_future_sweep_refresh_restart_and_late_answer() {
    let scratch = Scratch::new();
    let mut owner = identity("direct", "s");
    owner.asker = Some(cingulate_store::ScopeIdentity {
        scope_ref: "head".into(),
        scope_epoch: 1,
        agent_id: Some("agent".into()),
    });
    let (store, _) = scratch.open(100);
    let mut req = request();
    req.expires_in_ms = None;
    req.dedup_key = Some("no-deadline".into());
    let original = store.create(&owner, req.clone(), 100).unwrap().record;
    assert_eq!(original.expires_at_ms, None);
    let expiry: Option<i64> = scratch
        .sql()
        .query_row(
            "SELECT expires_at_ms FROM elicitation_records WHERE elicitation_id=?1",
            [&original.elicitation_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(expiry, None);
    let future = 100 + 100 * RETENTION_MS;
    assert!(store.sweep(future).unwrap().is_empty());
    assert_eq!(store.pending().unwrap(), vec![original.clone()]);
    let refreshed = store.create(&owner, req.clone(), future).unwrap();
    assert!(!refreshed.created);
    assert_eq!(refreshed.record, original);
    let phone = original.phone(Some(future + 120_000));
    assert!(phone.get("expiresAtMs").is_none());
    assert!(phone.get("expiresInMs").is_none());
    assert_eq!(phone["blockUntilMs"], future + 120_000);
    let requester = original.requester_wire(None);
    assert!(requester.get("expires_at_ms").is_none());
    assert!(requester.get("expires_in_ms").is_none());
    assert!(requester.get("block_until_ms").is_none());
    store
        .detach(&owner.owner, &original.elicitation_id, future)
        .unwrap();
    drop(store);
    let (store, events) = scratch.open(future + 1);
    assert!(events.is_empty());
    let answered = store
        .answer(&original.elicitation_id, "allow_once", None, future + 2)
        .unwrap();
    assert!(matches!(answered.value, AnswerOutcome::Answered(_)));
    assert_eq!(answered.settlements.len(), 1);
    assert!(answered.settlements[0].late);
    let notices = store.access_notices(future + 2).unwrap();
    assert_eq!(
        notices
            .iter()
            .map(|notice| notice.purpose.as_str())
            .collect::<Vec<_>>(),
        vec!["settled", "consent_answered"]
    );
    let other = store.create(&owner, req, future + 3).unwrap().record;
    let withdrawn = store
        .withdraw(&owner.owner, &other.elicitation_id, future + RETENTION_MS)
        .unwrap();
    assert_eq!(withdrawn.value.state, State::Withdrawn);
    assert!(withdrawn.value.answered_choice_id.is_none());
}

#[test]
fn optional_expiry_migration_preserves_all_existing_rows_and_cursors() {
    let scratch = Scratch::new();
    let old = cortexkit_store::open_sqlite(&scratch.descriptor).unwrap();
    old.migrate(MIGRATION_NAMESPACE, &cingulate_store::MIGRATIONS[..5])
        .unwrap();
    old.with_conn(|conn| {
        for (index, state) in ["pending", "answered", "expired", "withdrawn"].iter().enumerate() {
            let id = format!("el_{}", index + 1);
            let mut record = json!({
                "elicitationId":id, "request":request(), "createdAtMs":100,
                "expiresAtMs":60100, "subject":identity("direct", "s").subject,
                "state":state, "requester":"direct"
            });
            if index > 0 {
                record["answeredAtMs"] = json!(101);
            }
            conn.execute("INSERT INTO elicitation_records(elicitation_id,owner,dedup_key,record_json,state,expires_at_ms,settled_at_ms,acknowledged,notice_delivered) VALUES (?1,?2,?3,?4,?5,60100,?6,?7,?8)",
                params![id, identity("direct", "s").owner, format!("key-{index}"), record.to_string(), state, (index > 0).then_some(101), i64::from(index == 3), i64::from(index == 2)])?;
            if index > 0 {
                conn.execute("INSERT INTO elicitation_answers(cursor,elicitation_id,owner,settled_at_ms) VALUES (?1,?2,?3,101)", params![index as i64, id, identity("direct", "s").owner])?;
            }
        }
        conn.execute("UPDATE sqlite_sequence SET seq=99 WHERE name='elicitation_answers'", [])?;
        conn.execute("UPDATE cingulate_id_seq SET value=4", [])?;
        conn.execute("INSERT INTO access_notices(elicitation_id,purpose,body_json,attempts,retry_at_ms) VALUES ('el_2','consent_answered','{}',3,500)", [])?;
        Ok(())
    }).unwrap();
    drop(old);
    let mut before = snapshot(&scratch);
    for row in &mut before.answers {
        row.push(Value::Null);
        row.push(Value::Null);
    }
    for row in &mut before.records {
        row.extend((0..5).map(|_| json!(0)));
    }
    let notices = rows(&scratch.sql(), "access_notices");
    let (store, events) = scratch.open(102);
    assert!(events.is_empty());
    assert_eq!(snapshot(&scratch), before);
    // The one pending card has no stamped scope; opening the upgraded store
    // queues its person notice once, ahead of the rows already there.
    let mut expected = vec![vec![
        json!("el_1"),
        json!("escalated"),
        json!(
            r#"{"elicitation_id":"el_1","purpose":"escalated","fields":{"card_kind":"consent","subject":"an unverified local process","expires_at_ms":60100,"phone_only":false}}"#
        ),
        json!("pending"),
        json!(0),
        json!(0),
        Value::Null,
    ]];
    expected.extend(notices);
    // Migration 12 adds round zero without changing any earlier outbox value.
    for row in &mut expected {
        row.push(json!(0));
    }
    assert_eq!(rows(&scratch.sql(), "access_notices"), expected);
    assert!(rows(&scratch.sql(), "pragma_foreign_key_check").is_empty());
    let info = rows(&scratch.sql(), "pragma_table_info('elicitation_records')");
    assert_eq!(
        info.iter().find(|row| row[1] == "expires_at_ms").unwrap()[3],
        0
    );
    let record = store
        .get_owned(&identity("direct", "s").owner, "el_1", 102)
        .unwrap();
    assert_eq!(record.expires_at_ms, Some(60100));
    assert_eq!(
        store
            .withdraw(&identity("direct", "s").owner, "el_1", 103)
            .unwrap()
            .value
            .state,
        State::Withdrawn
    );
    assert_eq!(store.outcomes(99, 103).unwrap().cursor, 100);
    let revision = store.pending_revision().unwrap();
    let mut req = request();
    req.expires_in_ms = None;
    let created = store
        .create(&identity("direct", "s"), req, 104)
        .unwrap()
        .record;
    assert_eq!(created.elicitation_id, "el_5");
    assert_eq!(store.pending_revision().unwrap(), revision + 1);
    assert_eq!(created.expires_at_ms, None);
}

#[test]
fn optional_expiry_migration_keeps_an_empty_answer_logs_high_water() {
    let scratch = Scratch::new();
    let old = cortexkit_store::open_sqlite(&scratch.descriptor).unwrap();
    old.migrate(MIGRATION_NAMESPACE, &cingulate_store::MIGRATIONS[..5])
        .unwrap();
    old.with_conn(|conn| {
        conn.execute(
            "INSERT INTO sqlite_sequence(name,seq) VALUES ('elicitation_answers',99)",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    drop(old);
    let (store, _) = scratch.open(100);
    assert_eq!(store.outcomes(0, 100).unwrap().cursor, 99);
    let owner = identity("direct", "s");
    let id = store
        .create(&owner, request(), 101)
        .unwrap()
        .record
        .elicitation_id;
    store.withdraw(&owner.owner, &id, 102).unwrap();
    assert_eq!(store.outcomes(99, 102).unwrap().cursor, 100);
}

fn scoped_identity() -> RequestIdentity {
    RequestIdentity {
        asker: Some(cingulate_store::ScopeIdentity {
            scope_ref: "sc_asker".into(),
            scope_epoch: 3,
            agent_id: None,
        }),
        ..identity("direct", "s")
    }
}

#[test]
fn person_notice_subject_is_bounded_by_characters_and_keeps_attested_project() {
    let scratch = Scratch::new();
    let (store, _) = scratch.open(100);
    for (label, expected) in [
        (
            "ALF in project Test".to_owned(),
            "ALF in project Test".to_owned(),
        ),
        ("猫".repeat(200), "猫".repeat(200)),
        ("猫".repeat(201), format!("{}…", "猫".repeat(199))),
    ] {
        let mut owner = scoped_identity();
        owner.subject.label = label;
        let record = store.create(&owner, request(), 100).unwrap().record;
        let notice = store
            .access_notices(100)
            .unwrap()
            .into_iter()
            .find(|notice| notice.elicitation_id == record.elicitation_id)
            .unwrap();
        assert_eq!(notice.body["fields"]["subject"], expected);
        assert!(expected.chars().count() <= 200);
    }
}

#[test]
fn person_notice_backfill_survives_two_starts_and_excludes_unanswerable_cards() {
    let scratch = Scratch::new();
    let (store, _) = scratch.open(100);
    let owner = scoped_identity();
    let pending = store.create(&owner, request(), 100).unwrap().record;
    let answered = store.create(&owner, request(), 100).unwrap().record;
    store
        .answer(&answered.elicitation_id, "deny", None, 101)
        .unwrap();
    let mut console = request();
    console.at_console = Some("machine".into());
    let console = store.create(&owner, console, 100).unwrap().record;
    assert!(!store
        .access_notices(101)
        .unwrap()
        .iter()
        .any(|notice| notice.elicitation_id == console.elicitation_id));
    drop(store);
    let db = scratch.sql();
    db.execute("DELETE FROM access_notices", []).unwrap();
    let mut legacy = serde_json::to_value(&pending).unwrap();
    legacy.as_object_mut().unwrap().remove("asker");
    legacy.as_object_mut().unwrap().remove("head");
    let legacy_owner = json!([
        "reserved:cerebellum",
        json!(["reserved:prefrontal-core", "sc_asker", 3]).to_string()
    ])
    .to_string();
    db.execute(
        "UPDATE elicitation_records SET record_json=?2,owner=?3 WHERE elicitation_id=?1",
        params![pending.elicitation_id, legacy.to_string(), legacy_owner],
    )
    .unwrap();
    let (store, _) = scratch.open(102);
    let notices = store.access_notices(102).unwrap();
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].elicitation_id, pending.elicitation_id);
    assert_eq!(notices[0].purpose, "escalated");
    assert_eq!(
        notices[0].body["asker"],
        json!({"scope_ref":"sc_asker","scope_epoch":3})
    );
    assert!(notices[0].body.get("head").is_none());
    let bytes = notices[0].body_json.clone();
    store.access_notice_result(&notices[0], "ok", 102).unwrap();
    drop(store);
    let (store, _) = scratch.open(103);
    assert!(store.access_notices(103).unwrap().is_empty());
    let rows: (i64, String) = db
        .query_row("SELECT count(*),body_json FROM access_notices", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    assert_eq!(rows, (1, bytes));
}

#[test]
fn person_notice_failure_rolls_back_filing_and_answer() {
    let scratch = Scratch::new();
    let (store, _) = scratch.open(100);
    let db = scratch.sql();
    db.execute_batch("CREATE TRIGGER reject_notice BEFORE INSERT ON access_notices BEGIN SELECT RAISE(ABORT, 'notice rejected'); END;").unwrap();
    assert!(store.create(&scoped_identity(), request(), 100).is_err());
    assert!(store.pending().unwrap().is_empty());
    db.execute_batch("DROP TRIGGER reject_notice").unwrap();
    let id = store
        .create(&scoped_identity(), request(), 100)
        .unwrap()
        .record
        .elicitation_id;
    db.execute_batch("CREATE TRIGGER reject_notice BEFORE INSERT ON access_notices WHEN NEW.purpose='settled' BEGIN SELECT RAISE(ABORT, 'notice rejected'); END;").unwrap();
    assert!(store.answer(&id, "deny", None, 101).is_err());
    assert_eq!(store.get(&id, 101).unwrap().unwrap().state, State::Pending);
    assert_eq!(store.access_notices(101).unwrap()[0].purpose, "escalated");
}

#[test]
fn execution_backstop_installs_only_on_retained_generic_answers() {
    let scratch = Scratch::new();
    let (store, _) = scratch.open(100);
    let owner = scoped_identity();
    let id = store
        .create(&owner, request(), 100)
        .unwrap()
        .record
        .elicitation_id;
    store.execution_session_ended(&id, 101).unwrap();
    assert!(store.get(&id, 101).unwrap().unwrap().execution.is_none());
    store.answer(&id, "allow_once", None, 102).unwrap();
    let feed = store.answers(&owner.owner, 0, 103).unwrap();
    store.ack(&owner.owner, feed.cursor).unwrap();
    assert_eq!(
        store
            .execution_candidates(None, 16, 103)
            .unwrap()
            .records
            .len(),
        1
    );
    store.execution_session_ended(&id, 103).unwrap();
    let record = store.get_phone(&id, 103).unwrap().unwrap();
    assert_eq!(record.execution.as_deref(), Some("approved_not_executed"));
    assert_eq!(record.execution_reason.as_deref(), Some("session_ended"));
    assert_eq!(store.outcomes(0, 103).unwrap().cursor, feed.cursor);
    assert!(store
        .execution_candidates(None, 16, 104)
        .unwrap()
        .records
        .is_empty());
    let decision = store
        .create(&owner, flow_decision(), 104)
        .unwrap()
        .record
        .elicitation_id;
    store.answer(&decision, "proceed", None, 105).unwrap();
    store.execution_session_ended(&decision, 106).unwrap();
    assert!(store
        .get(&decision, 106)
        .unwrap()
        .unwrap()
        .execution
        .is_none());
    let pruned = store
        .create(&owner, request(), 107)
        .unwrap()
        .record
        .elicitation_id;
    store.answer(&pruned, "allow_once", None, 108).unwrap();
    store
        .execution_session_ended(&pruned, 108 + RETENTION_MS)
        .unwrap();
    assert!(store
        .execution_candidates(None, 16, 108 + RETENTION_MS)
        .unwrap()
        .records
        .is_empty());
    store.sweep(108 + RETENTION_MS).unwrap();
    store
        .execution_session_ended(&pruned, 108 + RETENTION_MS)
        .unwrap();
    assert!(store
        .get_phone(&pruned, 108 + RETENTION_MS)
        .unwrap()
        .is_none());
}

#[test]
fn execution_backstop_never_overwrites_a_requester_report() {
    let scratch = Scratch::new();
    let (store, _) = scratch.open(100);
    let owner = scoped_identity();
    let id = store
        .create(&owner, request(), 100)
        .unwrap()
        .record
        .elicitation_id;
    store.answer(&id, "allow_once", None, 102).unwrap();
    // The liveness probe can be in flight when the requester reports its effect.
    assert_eq!(
        store
            .execution_candidates(None, 16, 103)
            .unwrap()
            .records
            .len(),
        1
    );
    let reported = store
        .report_execution(&owner.owner, &id, "executed", None, 103)
        .unwrap();
    store.execution_session_ended(&id, 104).unwrap();
    assert_eq!(store.get_phone(&id, 104).unwrap().unwrap(), reported);
}

#[test]
fn requester_report_replaces_session_ended_backstop() {
    let scratch = Scratch::new();
    let (store, _) = scratch.open(100);
    let owner = scoped_identity();
    let id = store
        .create(&owner, request(), 100)
        .unwrap()
        .record
        .elicitation_id;
    store.answer(&id, "allow_once", None, 102).unwrap();
    store.execution_session_ended(&id, 103).unwrap();
    let backstop = store.get_phone(&id, 103).unwrap().unwrap();
    assert_eq!(backstop.execution.as_deref(), Some("approved_not_executed"));
    assert_eq!(backstop.execution_reason.as_deref(), Some("session_ended"));
    let reported = store
        .report_execution(&owner.owner, &id, "executed", None, 104)
        .unwrap();
    assert_eq!(reported.execution.as_deref(), Some("executed"));
    assert!(reported.execution_reason.is_none());
    store.execution_session_ended(&id, 105).unwrap();
    assert_eq!(store.get_phone(&id, 105).unwrap().unwrap(), reported);
}

#[test]
fn requester_report_racing_session_ended_backstop_keeps_exactly_one_report() {
    let scratch = Scratch::new();
    let (store, _) = scratch.open(100);
    let store = Arc::new(store);
    let owner = scoped_identity();
    for _ in 0..24 {
        let id = store
            .create(&owner, request(), 100)
            .unwrap()
            .record
            .elicitation_id;
        store.answer(&id, "allow_once", None, 102).unwrap();
        let cursor = store.outcomes(0, 103).unwrap().cursor;
        let barrier = Arc::new(Barrier::new(2));
        let backstop = {
            let store = store.clone();
            let barrier = barrier.clone();
            let id = id.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.execution_session_ended(&id, 103).unwrap();
            })
        };
        barrier.wait();
        let report = store
            .report_execution(&owner.owner, &id, "executed", None, 103)
            .unwrap();
        backstop.join().unwrap();
        let final_record = store.get_phone(&id, 104).unwrap().unwrap();
        assert_eq!(report, final_record);
        assert_eq!(final_record.execution.as_deref(), Some("executed"));
        assert!(final_record.execution_reason.is_none());
        assert_eq!(store.outcomes(0, 104).unwrap().cursor, cursor);
    }
}

#[test]
fn execution_scan_bounds_ineligible_rows_and_reaches_later_answers() {
    let scratch = Scratch::new();
    let (store, _) = scratch.open(100);
    let owner = scoped_identity();
    for _ in 0..39 {
        store.create(&owner, request(), 100).unwrap();
    }
    let id = store
        .create(&owner, request(), 100)
        .unwrap()
        .record
        .elicitation_id;
    store.answer(&id, "allow_once", None, 102).unwrap();
    let mut after = None;
    let mut found = Vec::new();
    for scanned in 0..=40 {
        let page = store
            .execution_candidates(after.as_deref(), 1, 103)
            .unwrap();
        assert!(page.records.len() <= 1);
        found.extend(page.records.into_iter().map(|r| r.elicitation_id));
        if scanned == 40 {
            assert!(page.after.is_none());
        } else {
            assert!(page.after.is_some());
        }
        after = page.after;
    }
    assert_eq!(found, vec![id]);
}

#[test]
fn change_log_records_every_card_change_and_compacts_with_a_stale_boundary() {
    let scratch = Scratch::new();
    let (mut store, _) = scratch.open(0);
    let start = store.changes_after(0, 64).unwrap();
    assert_eq!((start.head, start.pruned_through, start.backlog), (0, 0, 0));
    assert_eq!(start.instance.len(), 32);
    let first = store
        .create(&identity("direct", "s"), request(), 0)
        .unwrap()
        .record
        .elicitation_id;
    let second = store
        .create(&identity("direct", "s"), request(), 0)
        .unwrap()
        .record
        .elicitation_id;
    store.answer(&first, "allow_once", None, 1).unwrap();
    // Each card is listed once, at its latest change, ordered by that change.
    let page = store.changes_after(0, 64).unwrap();
    assert_eq!(page.head, 3);
    assert_eq!(page.backlog, 2);
    let listed: Vec<_> = page
        .changes
        .iter()
        .map(|change| (change.version, change.elicitation_id.clone()))
        .collect();
    assert_eq!(listed, [(2, second.clone()), (3, first.clone())]);
    assert_eq!(store.changes_after(2, 64).unwrap().backlog, 1);
    assert_eq!(store.changes_after(3, 64).unwrap().changes, []);

    // A sweep with nothing due writes no row, so it wakes no watch.
    let writes = store.write_count();
    store.sweep(2).unwrap();
    assert_eq!(store.write_count(), writes);

    // Compaction keeps the newest row and moves the stale boundary to the
    // highest version it deleted.
    store.set_change_log_rows(1);
    store.sweep(2).unwrap();
    let page = store.changes_after(0, 64).unwrap();
    assert_eq!((page.head, page.pruned_through), (3, 2));
    assert_eq!(page.changes.len(), 1);
    assert!(store.write_count() > writes);

    // Retention pruning is itself a change for each pruned card; the same
    // sweep then expires the second card.
    store.sweep(1 + RETENTION_MS).unwrap();
    let page = store.changes_after(3, 64).unwrap();
    let listed: Vec<_> = page
        .changes
        .iter()
        .map(|change| (change.version, change.elicitation_id.clone()))
        .collect();
    assert_eq!(listed, [(4, first), (5, second)]);
    assert_eq!(page.instance, start.instance);
}

#[test]
fn secret_use_migration_preserves_v8_rows_cursors_and_private_removals() {
    let conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    for migration in &cingulate_store::MIGRATIONS[..8] {
        conn.execute_batch(migration.statements).unwrap();
    }
    conn.execute("INSERT INTO elicitation_records(elicitation_id,owner,record_json,state,rich_ui_device) VALUES('old','owner','{}','answered','device')", []).unwrap();
    conn.execute("INSERT INTO elicitation_answers(cursor,elicitation_id,owner,settled_at_ms) VALUES(42,'old','owner',100)", []).unwrap();
    conn.execute_batch(cingulate_store::MIGRATIONS[8].statements)
        .unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT cursor FROM elicitation_answers WHERE elicitation_id='old'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        42
    );
    assert_eq!(
        conn.query_row(
            "SELECT rich_ui_device,secret_use FROM elicitation_records WHERE elicitation_id='old'",
            [],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?))
        )
        .unwrap(),
        ("device".into(), false)
    );
    conn.execute("INSERT INTO elicitation_records(elicitation_id,owner,record_json,state,secret_use) VALUES('secret','owner','{}','pending',1)", []).unwrap();
    conn.execute("INSERT INTO elicitation_answers(elicitation_id,owner,settled_at_ms) VALUES('secret','owner',200)", []).unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT cursor FROM elicitation_answers WHERE elicitation_id='secret'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        43
    );
    conn.execute(
        "UPDATE elicitation_records SET record_json=NULL WHERE elicitation_id='secret'",
        [],
    )
    .unwrap();
    conn.execute(
        "DELETE FROM elicitation_records WHERE elicitation_id='secret'",
        [],
    )
    .unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM cingulate_changes WHERE elicitation_id='secret' AND secret_use=1",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        3
    );
}

#[test]
fn retirement_migrates_v9_without_device_columns_or_revoked_state() {
    for retain_answers in [false, true] {
        let scratch = Scratch::new();
        let old = cortexkit_store::open_sqlite(&scratch.descriptor).unwrap();
        old.migrate(MIGRATION_NAMESPACE, &cingulate_store::MIGRATIONS[..9])
            .unwrap();
        let conn = scratch.sql();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        conn.execute("INSERT INTO elicitation_records(elicitation_id,owner,dedup_key,record_json,state,settled_at_ms,acknowledged,notice_delivered,secret_use) VALUES('old','owner','key','{}','answered',100,1,1,1)", []).unwrap();
        conn.execute("INSERT INTO elicitation_answers(cursor,elicitation_id,owner,settled_at_ms) VALUES(42,'old','owner',100)", []).unwrap();
        conn.execute(
            "INSERT INTO secret_use_requests(request_id,elicitation_id) VALUES('request','old')",
            [],
        )
        .unwrap();
        if !retain_answers {
            conn.execute("DELETE FROM elicitation_answers", []).unwrap();
        }
        let before_records = rows(&conn, "elicitation_records");
        let before_answers = rows(&conn, "elicitation_answers");
        let revision: i64 = conn
            .query_row("SELECT value FROM cingulate_pending_revision", [], |r| {
                r.get(0)
            })
            .unwrap();
        let changes: i64 = conn
            .query_row("SELECT MAX(version) FROM cingulate_changes", [], |r| {
                r.get(0)
            })
            .unwrap();
        let schema_version = || {
            conn.query_row(
                "SELECT MAX(version) FROM cortexkit_schema_version WHERE namespace='cingulate'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap()
        };
        assert_eq!(schema_version(), 9);
        old.migrate(MIGRATION_NAMESPACE, &cingulate_store::MIGRATIONS[..10])
            .unwrap();
        assert_eq!(schema_version(), 10);
        old.migrate(MIGRATION_NAMESPACE, &cingulate_store::MIGRATIONS[..10])
            .unwrap();
        assert_eq!(schema_version(), 10);
        for table in ["elicitation_records", "cingulate_changes"] {
            let names: Vec<String> = conn
                .prepare(&format!("PRAGMA table_info({table})"))
                .unwrap()
                .query_map([], |r| r.get(1))
                .unwrap()
                .map(Result::unwrap)
                .collect();
            assert!(!names.iter().any(|name| name == "rich_ui_device"));
        }
        let mut expected_records = before_records;
        for row in &mut expected_records {
            // The retired device column preceded secret_use in schema version 9.
            row.remove(row.len() - 2);
        }
        assert_eq!(rows(&conn, "elicitation_records"), expected_records);
        assert_eq!(rows(&conn, "elicitation_answers"), before_answers);
        assert_eq!(
            conn.query_row("SELECT value FROM cingulate_pending_revision", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            revision
        );
        assert_eq!(
            conn.query_row("SELECT MAX(version) FROM cingulate_changes", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            changes
        );
        assert!(conn
            .query_row(
                "SELECT secret_use FROM cingulate_changes WHERE elicitation_id='old'",
                [],
                |r| r.get::<_, bool>(0)
            )
            .unwrap());
        assert_eq!(
            conn.query_row(
                "SELECT elicitation_id FROM secret_use_requests WHERE request_id='request'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "old"
        );
        assert!(conn
            .execute(
                "UPDATE elicitation_records SET state='revoked' WHERE elicitation_id='old'",
                []
            )
            .is_err());
        assert_eq!(
            conn.query_row("PRAGMA foreign_key_check", [], |_| Ok(()))
                .optional()
                .unwrap(),
            None
        );
        conn.execute("INSERT INTO elicitation_records(elicitation_id,owner,record_json,state,secret_use) VALUES('new','owner','{}','pending',1)", []).unwrap();
        conn.execute("INSERT INTO elicitation_answers(elicitation_id,owner,settled_at_ms) VALUES('new','owner',200)", []).unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT cursor FROM elicitation_answers WHERE elicitation_id='new'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            43
        );
        conn.execute(
            "UPDATE elicitation_records SET record_json=NULL WHERE elicitation_id='new'",
            [],
        )
        .unwrap();
        conn.execute(
            "DELETE FROM elicitation_records WHERE elicitation_id='new'",
            [],
        )
        .unwrap();
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM cingulate_changes WHERE elicitation_id='new' AND secret_use=1", [], |r| r.get::<_, i64>(0)).unwrap(), 3);
        assert_eq!(
            conn.query_row("SELECT value FROM cingulate_pending_revision", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            revision + 2
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM elicitation_answers WHERE elicitation_id='new'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }
}
