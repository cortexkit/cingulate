mod support;

use cingulate_decode::ConsentRequest;
use cingulate_store::{AnswerOutcome, ConsentStore, ElicitationRecord, RequestIdentity, Subject};
use serde_json::{json, Value};
use support::*;

fn normalized_phone(record: &ElicitationRecord, fixture_id: &str) -> Value {
    let mut phone = record.phone(None);
    let object = phone.as_object_mut().unwrap();
    // Upstream uses symbolic ids and a clock starting at 1000. Only these
    // transport-independent values and cingulate's two additions may differ.
    assert_eq!(
        object.get("elicitationId"),
        Some(&json!(record.elicitation_id))
    );
    object.insert("elicitationId".into(), json!(fixture_id));
    for key in ["createdAtMs", "expiresAtMs", "answeredAtMs"] {
        if let Some(value) = object.get_mut(key) {
            *value = json!(value.as_i64().unwrap() - (NOW - 1000));
        }
    }
    object.remove("requesterLabel");
    object.remove("atConsole");
    phone
}

#[tokio::test]
async fn stored_phone_projections_match_all_record_vectors() {
    let dir = tempfile::tempdir().unwrap();
    let (store, settlements) =
        ConsentStore::open(&descriptor(&dir.path().join("records.sqlite")), NOW).unwrap();
    assert!(settlements.is_empty());
    let identity = RequestIdentity {
        owner: json!(["reserved:plexus", "provider-session"]).to_string(),
        requester: "reserved:plexus".into(),
        subject: Subject {
            agent_id: Some("agent-alf".into()),
            head_agent_id: "agent-alf".into(),
            label: "ALF".into(),
        },
        resolved_name: Some("ALF".into()),
        asker: None,
    };
    let head = store
        .create(
            &identity,
            ConsentRequest::decode(request_body()).unwrap(),
            NOW,
        )
        .unwrap()
        .record;
    let mut worker_body = request_body();
    worker_body["session_ref"] = json!("ses_worker");
    worker_body["parent_session_ref"] = json!("ses_head");
    // The record corpus explicitly includes this historical worker subject.
    // Store it verbatim, rather than resolving a carrier stamp as a head.
    let worker_identity = RequestIdentity {
        subject: Subject {
            agent_id: None,
            head_agent_id: "agent-alf".into(),
            label: "a worker of ALF".into(),
        },
        resolved_name: None,
        ..identity.clone()
    };
    let worker = store
        .create(
            &worker_identity,
            ConsentRequest::decode(worker_body).unwrap(),
            NOW,
        )
        .unwrap()
        .record;
    let pending = store.pending().unwrap();
    assert_eq!(pending.len(), 2);
    let projections: Vec<Value> = pending
        .iter()
        .map(|record| {
            let id = if record.elicitation_id == head.elicitation_id {
                "el_head"
            } else {
                assert_eq!(record.elicitation_id, worker.elicitation_id);
                "el_worker"
            };
            normalized_phone(record, id)
        })
        .collect();
    assert_eq!(
        json!(projections),
        vector("elicitation-records-v1", "list-pending.json")["reply"]["records"]
    );

    for (file, choice, variant) in [
        ("answer-success.json", "transfer:chrome", "answered"),
        ("answer-repeat.json", "transfer:chrome", "repeat"),
        ("answer-conflict.json", "deny", "conflict"),
    ] {
        let result = store
            .answer(&head.elicitation_id, choice, None, ANSWERED)
            .unwrap();
        match (variant, result.value) {
            ("answered", AnswerOutcome::Answered(_)) => assert_eq!(result.settlements.len(), 1),
            ("repeat", AnswerOutcome::AlreadyAnswered(_))
            | ("conflict", AnswerOutcome::Conflict(_)) => assert!(result.settlements.is_empty()),
            (_, other) => panic!("{file}: {other:?}"),
        }
        let stored = store.get(&head.elicitation_id, ANSWERED).unwrap().unwrap();
        assert_eq!(
            normalized_phone(&stored, "el_head"),
            vector("elicitation-records-v1", file)["reply"]["elicitation"],
            "{file}"
        );
    }
    store
        .report_execution(&identity.owner, &head.elicitation_id, "executed", ANSWERED)
        .unwrap();
    let stored = store.get(&head.elicitation_id, ANSWERED).unwrap().unwrap();
    assert_eq!(
        normalized_phone(&stored, "el_head"),
        vector("elicitation-records-v1", "get-settled.json")["reply"]
    );

    let fixture = Fixture::open(None).await;
    let missing = fixture
        .ok(
            &operator(),
            "consent.answer",
            json!({"elicitationId":"el_missing", "choiceId":"deny"}),
        )
        .await;
    assert_eq!(
        missing,
        vector("elicitation-records-v1", "answer-not-found.json")["reply"]
    );
    assert!(fixture.payloads().is_empty());
}

async fn compare_pushes(machine: Option<&str>) {
    let fixture = Fixture::open(machine).await;
    let id = fixture.request(&owner(), request_body()).await;
    let created = fixture
        .ok(&owner(), "consent.get", json!({"elicitation_id":id}))
        .await;
    assert_eq!(created["created_at_ms"], NOW);
    fixture.clock.set(ANSWERED);
    fixture
        .ok(
            &operator(),
            "consent.answer",
            json!({"elicitationId":id, "choiceId":"transfer:chrome"}),
        )
        .await;
    let settled = fixture
        .ok(&owner(), "consent.get", json!({"elicitation_id":id}))
        .await;
    assert_eq!(settled["answered_at_ms"], ANSWERED);
    let expected: Vec<Value> = [
        ("push-new.json", &created["created_at_ms"]),
        ("push-retraction.json", &settled["answered_at_ms"]),
    ]
    .into_iter()
    .map(|(file, event)| {
        let mut pointer = vector("elicitation-records-v1", file)["reply"].clone();
        assert_eq!(pointer["elicitationId"], "el_head");
        pointer["elicitationId"] = json!(id);
        pointer["eventAtMs"] = event.clone();
        if let Some(machine) = machine {
            pointer["machineId"] = json!(machine);
        }
        pointer
    })
    .collect();
    assert_eq!(fixture.payloads(), expected);
}

#[tokio::test]
async fn recording_notifier_matches_push_vectors_without_machine() {
    compare_pushes(None).await;
}

#[tokio::test]
async fn recording_notifier_matches_push_vectors_with_hello_machine() {
    compare_pushes(Some("m1")).await;
}
