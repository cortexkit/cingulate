//! Late answers on generic consent cards: `detach` marks the moment the
//! requester stops holding the agent's call, and only a settlement committed
//! after that mark queues a `consent_answered` notice in the shared outbox.

use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Barrier,
    },
};

use cingulate_decode::{ConsentOption, ConsentRequest};
use cingulate_store::{
    ConsentStore, ElicitationRecord, Isolation, RequestIdentity, ScopeIdentity, State,
    StorageBackend, StorageDescriptor, Subject, MIGRATIONS, MIGRATION_NAMESPACE,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct Scratch {
    root: PathBuf,
    descriptor: StorageDescriptor,
}

impl Scratch {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "cingulate-late-answer-{}-{}-{}",
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
                path: root.join("late.db").to_str().unwrap().into(),
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

    fn open(&self, now: i64) -> ConsentStore {
        ConsentStore::open(&self.descriptor, now).unwrap().0
    }

    /// Every outbox row as (elicitation_id, purpose, body_json, status).
    fn outbox(&self) -> Vec<(String, String, String, String)> {
        let conn = self.sql();
        let mut stmt = conn
            .prepare(
                "SELECT elicitation_id,purpose,body_json,status FROM access_notices ORDER BY rowid",
            )
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    fn late_notices(&self) -> Vec<(String, String)> {
        self.outbox()
            .into_iter()
            .filter(|(_, purpose, _, _)| purpose == "consent_answered")
            .map(|(id, _, body, _)| (id, body))
            .collect()
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

fn asker() -> ScopeIdentity {
    ScopeIdentity {
        scope_ref: "sc_head_alf".into(),
        scope_epoch: 3,
        agent_id: Some("agent-alf".into()),
    }
}

/// Cerebellum's stamped identity on a route whose scope is the agent session.
fn cerebellum() -> RequestIdentity {
    RequestIdentity {
        owner: json!(["reserved:cerebellum", "agent-session"]).to_string(),
        requester: "reserved:cerebellum".into(),
        subject: Subject {
            agent_id: Some("agent-alf".into()),
            head_agent_id: "agent-alf".into(),
            label: "ALF".into(),
            via_worker: false,
            federated: None,
        },
        resolved_name: Some("ALF".into()),
        asker: Some(asker()),
        head: None,
    }
}

fn create(store: &ConsentStore, now: i64) -> String {
    store
        .create(&cerebellum(), request(), now)
        .unwrap()
        .record
        .elicitation_id
}

fn pinned(file: &str) -> String {
    fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../test-vectors/consent-late-answer-v1")
            .join(file),
    )
    .unwrap()
}

/// Inputs matching test-vectors/consent-late-answer-v1/: Cerebellum's detached
/// card in session `sc_head_alf`. Changing them changes the expected bytes.
fn pinned_record(id: &str, state: State, choice: Option<&str>, at: i64) -> ElicitationRecord {
    let mut request = request();
    request.options = vec![
        ConsentOption {
            id: "allow_30m".into(),
            label: "Allow for 30 minutes".into(),
            effect: "grant".into(),
            detail: None,
            group: None,
        },
        ConsentOption {
            id: "deny".into(),
            label: "Not now".into(),
            effect: "decline".into(),
            detail: None,
            group: None,
        },
    ];
    ElicitationRecord {
        elicitation_id: id.into(),
        request,
        created_at_ms: at - 1000,
        expires_at_ms: Some(at),
        subject: cerebellum().subject,
        state,
        answered_choice_id: choice.map(str::to_owned),
        answered_at_ms: Some(at),
        answered_device: None,
        answered_surface: None,
        secret_use_answer: None,
        manifest_binding_answer: None,
        input_supplied: false,
        sealed_inputs: None,
        answer_seq: None,
        statement: None,
        signature_b64url: None,
        unverified: None,
        notice_round: 0,
        execution: None,
        execution_reason: None,
        requester: "reserved:cerebellum".into(),
        detached: true,
        late: true,
        asker: Some(asker()),
        head: None,
    }
}

#[test]
fn late_answer_notice_serializes_to_the_pinned_bytes() {
    for (file, record) in [
        (
            "notice-answered.json",
            pinned_record(
                "el_0000000000000042",
                State::Answered,
                Some("allow_30m"),
                1_792_000_000_000,
            ),
        ),
        (
            "notice-expired.json",
            pinned_record(
                "el_0000000000000043",
                State::Expired,
                None,
                1_792_000_600_000,
            ),
        ),
        (
            "notice-withdrawn.json",
            pinned_record(
                "el_0000000000000044",
                State::Withdrawn,
                None,
                1_792_000_700_000,
            ),
        ),
    ] {
        assert_eq!(
            record.late_answer_notice().unwrap(),
            pinned(file),
            "{file} differs"
        );
    }
}

#[test]
fn late_answer_notice_never_carries_requester_ref_or_card_content() {
    let scratch = Scratch::new();
    let store = scratch.open(100);
    let mut consent = request();
    consent.requester_ref = Some("tool-call-42".into());
    let id = store
        .create(&cerebellum(), consent, 100)
        .unwrap()
        .record
        .elicitation_id;
    store.detach(&cerebellum().owner, &id, 101).unwrap();
    store.answer(&id, "allow_once", None, 102).unwrap();
    let notices = scratch.late_notices();
    assert_eq!(notices.len(), 1);
    let body = &notices[0].1;
    for private in [
        "requester_ref",
        "tool-call-42",
        "Allow interaction with this app?",
        "Allow app access",
        "com.apple.TextEdit",
    ] {
        assert!(!body.contains(private), "notice leaked {private}: {body}");
    }
    let value: Value = serde_json::from_str(body).unwrap();
    assert_eq!(
        value,
        json!({"elicitation_id":id,"purpose":"consent_answered",
            "asker":{"scope_ref":"sc_head_alf","scope_epoch":3},
            "fields":{"requester":"cerebellum","outcome":"answered","choice_id":"allow_once",
                "choice_label":"Allow once","settled_at_ms":102}})
    );
}

#[test]
fn detach_then_settle_queues_exactly_one_late_answer_notice() {
    for settle in ["answer", "expire", "withdraw", "replace"] {
        let scratch = Scratch::new();
        let store = scratch.open(100);
        let owner = cerebellum().owner;
        let mut consent = request();
        consent.dedup_key = Some("call-42".into());
        let id = store
            .create(&cerebellum(), consent.clone(), 100)
            .unwrap()
            .record
            .elicitation_id;
        let detached = store.detach(&owner, &id, 101).unwrap();
        assert!(detached.settlements.is_empty(), "{settle}");
        assert!(detached.value.detached, "{settle}");
        assert_eq!(detached.value.state, State::Pending, "{settle}");
        // Detach on a pending card queues nothing yet.
        assert!(scratch.late_notices().is_empty(), "{settle}");
        let settled = match settle {
            "answer" => {
                store
                    .answer(&id, "allow_once", None, 102)
                    .unwrap()
                    .settlements
            }
            "expire" => store.sweep(100 + 60_000).unwrap(),
            "withdraw" => store.withdraw(&owner, &id, 102).unwrap().settlements,
            "replace" => {
                consent.title = "Changed content withdraws the old card".into();
                store
                    .create(&cerebellum(), consent, 102)
                    .unwrap()
                    .settlements
            }
            _ => unreachable!(),
        };
        assert_eq!(settled.len(), 1, "{settle}");
        assert!(settled[0].detached, "{settle}");
        let notices = scratch.late_notices();
        assert_eq!(notices.len(), 1, "{settle}");
        assert_eq!(notices[0].0, id, "{settle}");
        assert_eq!(
            notices[0].1,
            settled[0].late_answer_notice().unwrap(),
            "{settle}"
        );
        let outcome =
            serde_json::from_str::<Value>(&notices[0].1).unwrap()["fields"]["outcome"].clone();
        assert_eq!(
            outcome,
            match settle {
                "answer" => "answered",
                "expire" => "expired",
                _ => "withdrawn",
            },
            "{settle}"
        );
        // A repeat detach returns the settled record, still marked, and
        // neither undoes the mark nor queues a second notice.
        let repeat = store.detach(&owner, &id, 103).unwrap();
        assert!(repeat.value.detached, "{settle}");
        assert_ne!(repeat.value.state, State::Pending, "{settle}");
        assert!(repeat.settlements.is_empty(), "{settle}");
        assert_eq!(scratch.late_notices().len(), 1, "{settle}");
    }
}

#[test]
fn repeated_detach_on_a_pending_card_returns_the_current_record() {
    let scratch = Scratch::new();
    let store = scratch.open(100);
    let owner = cerebellum().owner;
    let id = create(&store, 100);
    let first = store.detach(&owner, &id, 101).unwrap().value;
    let second = store.detach(&owner, &id, 102).unwrap().value;
    assert_eq!(first, second);
    assert!(second.detached);
    assert_eq!(second.state, State::Pending);
}

#[test]
fn settle_then_detach_returns_the_settled_record_unmarked() {
    for settle in ["answer", "expire_at_detach"] {
        let scratch = Scratch::new();
        let store = scratch.open(100);
        let owner = cerebellum().owner;
        let id = create(&store, 100);
        let detach_at = if settle == "answer" {
            store.answer(&id, "allow_once", None, 101).unwrap();
            102
        } else {
            // The card is already due when detach arrives: the detach
            // transaction settles it first, so it settled while still held.
            100 + 60_000
        };
        let reply = store.detach(&owner, &id, detach_at).unwrap();
        assert!(!reply.value.detached, "{settle}");
        assert_eq!(
            reply.value.state,
            if settle == "answer" {
                State::Answered
            } else {
                State::Expired
            },
            "{settle}"
        );
        assert_eq!(
            reply.settlements.len(),
            usize::from(settle != "answer"),
            "{settle}"
        );
        let stored = store.get(&id, detach_at).unwrap().unwrap();
        assert!(!stored.detached, "{settle}");
        assert_eq!(stored, reply.value, "{settle}");
    }
}

#[test]
fn never_detached_card_queues_no_late_answer_notice() {
    let scratch = Scratch::new();
    let store = scratch.open(100);
    let owner = cerebellum().owner;
    let answered = create(&store, 100);
    let withdrawn = create(&store, 100);
    let mut consent = request();
    consent.dedup_key = Some("call-7".into());
    let replaced = store
        .create(&cerebellum(), consent.clone(), 100)
        .unwrap()
        .record
        .elicitation_id;
    let held_then_detached = create(&store, 100);
    let expired = create(&store, 1000);
    store.answer(&answered, "allow_once", None, 101).unwrap();
    store.withdraw(&owner, &withdrawn, 101).unwrap();
    consent.title = "Changed content withdraws the old card".into();
    store.create(&cerebellum(), consent, 101).unwrap();
    store
        .answer(&held_then_detached, "deny", None, 101)
        .unwrap();
    store.detach(&owner, &held_then_detached, 102).unwrap();
    let swept = store.sweep(1000 + 60_000).unwrap();
    assert!(swept.iter().any(|record| record.elicitation_id == expired));
    for id in [
        &answered,
        &withdrawn,
        &replaced,
        &held_then_detached,
        &expired,
    ] {
        assert_ne!(
            store.get(id, 1000 + 60_000).unwrap().unwrap().state,
            State::Pending
        );
    }
    assert_eq!(scratch.late_notices(), vec![]);
}

#[test]
fn detach_is_raiser_only_and_kind_consent_only() {
    let scratch = Scratch::new();
    let store = scratch.open(100);
    let id = create(&store, 100);
    let other = json!(["reserved:plexus", "agent-session"]).to_string();
    for (owner, card) in [(&other, id.as_str()), (&cerebellum().owner, "el_999")] {
        assert_eq!(
            store.detach(owner, card, 101).unwrap_err().code(),
            "elicitation_not_owner"
        );
    }
    let mut decision = serde_json::to_value(request()).unwrap();
    decision["kind"] = json!("flow_decision");
    decision["flow_decision"] = json!({"arbitrary":"decision"});
    decision["options"] = json!([
        {"id":"proceed","label":"Proceed","effect":"choose"},
        {"id":"decline","label":"Decline","effect":"decline"},
    ]);
    decision["default"] = json!("decline");
    let basal = RequestIdentity {
        owner: json!(["reserved:basal", "s"]).to_string(),
        requester: "reserved:basal".into(),
        ..cerebellum()
    };
    let flow = store
        .create(&basal, ConsentRequest::decode(decision).unwrap(), 100)
        .unwrap()
        .record
        .elicitation_id;
    assert_eq!(
        store.detach(&basal.owner, &flow, 101).unwrap_err().code(),
        "elicitation_requester_refused"
    );
    // Neither refused detach call set the `detached` flag on its card.
    assert!(!store.get(&id, 101).unwrap().unwrap().detached);
    assert!(!store.get(&flow, 101).unwrap().unwrap().detached);
}

#[test]
fn concurrent_detach_and_answer_classify_every_card_exactly_once() {
    let scratch = Scratch::new();
    let store = Arc::new(scratch.open(100));
    let owner = cerebellum().owner;
    for round in 0..40 {
        let id = create(&store, 100);
        let barrier = Arc::new(Barrier::new(2));
        let detach = {
            let (store, barrier, id, owner) =
                (store.clone(), barrier.clone(), id.clone(), owner.clone());
            std::thread::spawn(move || {
                barrier.wait();
                store.detach(&owner, &id, 101).unwrap().value
            })
        };
        let answer = {
            let (store, barrier, id) = (store.clone(), barrier.clone(), id.clone());
            std::thread::spawn(move || {
                barrier.wait();
                store.answer(&id, "allow_once", None, 101).unwrap()
            })
        };
        let reply = detach.join().unwrap();
        answer.join().unwrap();
        let notices = scratch
            .late_notices()
            .into_iter()
            .filter(|(notice, _)| *notice == id)
            .count();
        if reply.detached {
            // Detach committed first: the answer settled a detached card.
            assert_eq!(notices, 1, "round {round}");
        } else {
            // The answer committed first: detach saw a settled, held card.
            assert_eq!(reply.state, State::Answered, "round {round}");
            assert_eq!(notices, 0, "round {round}");
        }
        assert!(store.get(&id, 101).unwrap().unwrap().state == State::Answered);
    }
}

#[test]
fn shared_outbox_migration_upgrades_version_four_keeping_rows_and_order() {
    let scratch = Scratch::new();
    let old = cortexkit_store::open_sqlite(&scratch.descriptor).unwrap();
    old.migrate(MIGRATION_NAMESPACE, &MIGRATIONS[..4]).unwrap();
    old.with_conn(|conn| {
        for id in ["acc_2", "acc_1"] {
            conn.execute(
                "INSERT INTO access_asks(elicitation_id,principal,owner,state,record_json) \
                 VALUES (?1,'reserved:aft','{}','settled','{}')",
                [id],
            )?;
        }
        // Rows inserted out of id order: delivery order is rowid, not the key.
        conn.execute(
            "INSERT INTO access_notices(elicitation_id,purpose,body_json,status,attempts,retry_at_ms,last_error) \
             VALUES ('acc_2','user_ask','{\"b\":2}','delivered',3,4000,NULL)",
            [],
        )?;
        conn.execute(
            "INSERT INTO access_notices(elicitation_id,purpose,body_json,status,attempts,retry_at_ms,last_error) \
             VALUES ('acc_1','settled','{\"a\":1}','pending',1,500,'transport')",
            [],
        )?;
        // At schema version 4 every outbox row must reference a permission
        // ask, so a notice for a generic consent card is rejected.
        assert!(conn
            .execute(
                "INSERT INTO access_notices(elicitation_id,purpose,body_json) VALUES ('el_1','consent_answered','{}')",
                [],
            )
            .is_err());
        Ok(())
    })
    .unwrap();
    drop(old);
    let before = scratch.outbox();
    let store = scratch.open(100);
    assert_eq!(scratch.outbox(), before);
    let conn = scratch.sql();
    let detail: (i64, i64, Option<String>) = conn
        .query_row(
            "SELECT attempts,retry_at_ms,last_error FROM access_notices WHERE elicitation_id='acc_1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(detail, (1, 500, Some("transport".into())));
    let schema: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE name='access_notices'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!schema.contains("REFERENCES"), "{schema}");
    assert_eq!(
        conn.query_row(
            "SELECT MAX(version) FROM cortexkit_schema_version WHERE namespace=?1",
            [MIGRATION_NAMESPACE],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        13
    );
    drop(conn);
    // After migration 5 the shared outbox also accepts a late-answer notice
    // for a generic consent card.
    let id = create(&store, 100);
    store.detach(&cerebellum().owner, &id, 101).unwrap();
    store.answer(&id, "allow_once", None, 102).unwrap();
    assert_eq!(scratch.late_notices().len(), 1);
    let notices = store.access_notices(1000).unwrap();
    assert_eq!(
        notices
            .iter()
            .map(|n| (n.elicitation_id.as_str(), n.purpose.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("acc_1", "settled"),
            (id.as_str(), "settled"),
            (id.as_str(), "consent_answered")
        ]
    );
    assert_eq!(notices[2].body_json, scratch.late_notices()[0].1);
    let _ = params![];
}
