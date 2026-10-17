use std::{collections::BTreeMap, time::Duration};

use ck_cingulate::Stamp;
use serde_json::{json, Value};
use subc_client_rs::HandlerOutcome;

use crate::{
    phone,
    support::{app_body, stamp, Fixture, NOW},
};

pub const CORPUS: &str = "consent-no-expiry-v1";

async fn record(
    files: &mut BTreeMap<String, Vec<u8>>,
    fixture: &Fixture,
    requester: &Stamp,
    name: &str,
    method: &str,
    params: Value,
) -> Value {
    let request = serde_json::to_vec(&json!({"method":method,"params":params})).unwrap();
    let outcome = tokio::time::timeout(
        Duration::from_secs(1),
        fixture.handler.handle_stamped(requester, &request),
    )
    .await
    .expect("no-expiry vector operation exceeded 1 s");
    let response = match outcome {
        HandlerOutcome::Response(bytes) => bytes,
        HandlerOutcome::Error { code, message } => {
            serde_json::to_vec(&json!({"error":{"code":code,"message":message}})).unwrap()
        }
        other => panic!("{method}: {other:?}"),
    };
    let envelope: Value = serde_json::from_slice(&response).unwrap();
    files.insert(format!("{name}-request.json"), request);
    files.insert(format!("{name}-reply.json"), response);
    envelope
}

pub async fn generate() -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let fixture = Fixture::open(None).await;
    let requester = stamp("direct");
    let mut body = app_body();
    body.as_object_mut().unwrap().remove("expires_in_ms");
    let created = record(
        &mut files,
        &fixture,
        &requester,
        "no-deadline",
        "consent.request",
        body.clone(),
    )
    .await;
    let id = created["result"]["elicitation_id"].as_str().unwrap();
    files.insert(
        "no-deadline-stored.json".into(),
        serde_json::to_vec(&fixture.stored_record(id)).unwrap(),
    );
    record(
        &mut files,
        &fixture,
        &requester,
        "no-deadline-get",
        "consent.get",
        json!({"elicitation_id":id}),
    )
    .await;
    record(
        &mut files,
        &fixture,
        &phone::phone_stamp(),
        "no-deadline-phone-get",
        "consent.get",
        json!({"elicitationId":id}),
    )
    .await;

    fixture.clock.set(NOW + 10);
    let mut expiring = app_body();
    expiring["expires_in_ms"] = json!(1000);
    fixture.request(&requester, expiring).await;
    fixture.clock.set(NOW + 20);
    fixture.request(&requester, body).await;
    let mixed = record(
        &mut files,
        &fixture,
        &phone::phone_stamp(),
        "mixed-list-page",
        "consent.list_pending",
        json!({}),
    )
    .await;
    assert_eq!(mixed["result"]["records"][0]["elicitationId"], id);
    assert!(mixed["result"]["records"][0].get("expiresAtMs").is_none());
    assert_eq!(mixed["result"]["records"][1]["expiresAtMs"], NOW + 1010);
    assert!(mixed["result"]["records"][2].get("expiresAtMs").is_none());
    record(
        &mut files,
        &fixture,
        &phone::phone_stamp(),
        "mixed-list-continued",
        "consent.list_pending",
        json!({"continuation":{"created_at_ms":NOW+10,"elicitation_id":"el_2"}}),
    )
    .await;
    fixture.clock.set(NOW + 10_000_000_000);
    let pending = record(
        &mut files,
        &fixture,
        &requester,
        "no-deadline-future-get",
        "consent.get",
        json!({"elicitation_id":id}),
    )
    .await;
    assert_eq!(pending["result"]["state"], "pending");
    fixture
        .ok(
            &phone::phone_stamp(),
            "consent.answer",
            json!({"elicitationId":id,"choiceId":"allow_once"}),
        )
        .await;
    let reported = record(
        &mut files,
        &fixture,
        &requester,
        "execution-reason",
        "consent.report_execution",
        json!({"elicitation_id":id,"execution":"approved_not_executed","reason":"session_ended"}),
    )
    .await;
    assert_eq!(reported["result"]["execution_reason"], "session_ended");
    let phone_reported = record(
        &mut files,
        &fixture,
        &phone::phone_stamp(),
        "execution-reason-phone-get",
        "consent.get",
        json!({"elicitationId":id}),
    )
    .await;
    assert_eq!(phone_reported["result"]["executionReason"], "session_ended");
    for (name, execution, reason) in [
        ("reason-with-executed", "executed", "session_ended"),
        ("malformed-reason", "approved_not_executed", "Session ended"),
    ] {
        let refused = record(
            &mut files,
            &fixture,
            &requester,
            name,
            "consent.report_execution",
            json!({"elicitation_id":id,"execution":execution,"reason":reason}),
        )
        .await;
        assert_eq!(refused["error"]["code"], "elicitation_invalid_execution");
        assert!(refused["error"]["message"]
            .as_str()
            .unwrap()
            .contains("reason"));
    }
    files
}
