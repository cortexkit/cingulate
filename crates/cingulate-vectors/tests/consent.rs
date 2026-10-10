mod support;

use ck_cingulate::Stamp;
use serde_json::json;
use support::*;

#[tokio::test]
async fn consent_requester_record_matches_unfiltered_answered_owner_reply() {
    let fixture = Fixture::open(None).await;
    let direct = stamp("direct");
    let mut body = app_body();
    body["binding"] = json!({"k":1});
    let id = fixture.request(&direct, body).await;
    fixture.clock.set(ANSWERED);
    fixture
        .ok(
            &operator(),
            "consent.answer",
            json!({"elicitationId":id,"choiceId":"allow_once"}),
        )
        .await;
    let substitution = Substitution {
        id: &id,
        now: NOW,
        expires: NOW + 60_000,
        answered: ANSWERED,
    };
    let vector = substitution.apply(&vector("consent-records-v1", "requester-record.json"));
    let reply = fixture
        .ok(
            &direct,
            vector["method"].as_str().unwrap(),
            vector["request"].clone(),
        )
        .await;
    assert_eq!(reply, vector["reply"]);
}

#[tokio::test]
async fn consent_console_vectors_match_unfiltered_ops_and_notifier_payloads() {
    let fixture = Fixture::open(Some("m1")).await;
    let direct = stamp("direct");
    let mut body = app_body();
    body["at_console"] = json!("m1");
    let id = fixture.request(&direct, body).await;
    let substitution = Substitution {
        id: &id,
        now: NOW,
        expires: NOW + 60_000,
        answered: ANSWERED,
    };
    let new =
        substitution.apply(&vector("consent-records-v1", "push-new-machine.json"))["reply"].clone();
    assert_eq!(fixture.payloads(), vec![new.clone()]);
    let phone = substitution.apply(&vector(
        "consent-records-v1",
        "phone-pending-at-console.json",
    ));
    let reply = fixture
        .ok(
            &operator(),
            phone["method"].as_str().unwrap(),
            phone["request"].clone(),
        )
        .await;
    assert_eq!(reply, phone["reply"]);

    let refusal = substitution.apply(&vector("consent-records-v1", "answer-not-at-console.json"));
    let rows = fixture.rows();
    let payloads = fixture.payloads();
    let operator = Stamp {
        console_machine_id: None,
        ..operator()
    };
    let error = fixture
        .call(
            &operator,
            refusal["method"].as_str().unwrap(),
            refusal["request"].clone(),
        )
        .await
        .unwrap_err();
    assert_eq!(error, refusal["error"]);
    assert_eq!(fixture.rows(), rows);
    assert_eq!(fixture.payloads(), payloads);

    fixture.clock.set(ANSWERED);
    let withdrawn = fixture
        .ok(&direct, "consent.withdraw", json!({"elicitation_id":id}))
        .await;
    assert_eq!(withdrawn["state"], "withdrawn");
    assert_eq!(withdrawn["answered_at_ms"], ANSWERED);
    let retraction = substitution.apply(&vector(
        "consent-records-v1",
        "push-retraction-machine.json",
    ))["reply"]
        .clone();
    assert_eq!(fixture.payloads(), vec![new, retraction]);
}

#[tokio::test]
async fn tagged_package_vector_hash_is_pinned_and_card_is_stored_end_to_end() {
    use sha2::{Digest, Sha256};
    let bytes = include_bytes!("../../../test-vectors/consent-package-card-v1/request.json");
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "960d3fd19ee6bcec2cabc22c86063208e6dca5ded4da45bbe4e9b779262be5ed"
    );
    let fixture = Fixture::open(None).await;
    let requester = stamp("reserved:prefrontal-core");
    let reply = fixture.handler.handle_stamped(&requester, bytes).await;
    let subc_client_rs::HandlerOutcome::Response(reply) = reply else {
        panic!("package request refused: {reply:?}")
    };
    let reply: serde_json::Value = serde_json::from_slice(&reply).unwrap();
    let id = reply["result"]["elicitation_id"].as_str().unwrap();
    let request: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    let record = fixture
        .ok(&requester, "consent.get", json!({"elicitation_id":id}))
        .await;
    assert_eq!(record["kind"], "package");
    assert_eq!(record["package"], request["params"]["package"]);
    assert_eq!(record["binding"], request["params"]["binding"]);
    let rows = fixture.rows();
    assert_eq!(rows[0].len(), 1);
    assert!(rows[0][0].iter().any(|column| {
        let rusqlite::types::Value::Text(raw) = column else {
            return false;
        };
        serde_json::from_str::<serde_json::Value>(raw)
            .is_ok_and(|stored| stored["request"]["package"] == request["params"]["package"])
    }));
}
