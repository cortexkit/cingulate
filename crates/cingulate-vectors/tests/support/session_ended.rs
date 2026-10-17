use std::collections::BTreeMap;

use ck_cingulate::{ScopeLife, Stamp};
use serde_json::{json, Value};
use subc_protocol::scope::ScopeKind;

use crate::{
    phone,
    support::{app_body, worker, Fixture, ANSWERED},
};

pub const CORPUS: &str = "consent-session-ended-v1";

pub async fn answered(fixture: &Fixture, requester: &Stamp) -> String {
    let id = fixture.request(requester, app_body()).await;
    fixture.clock.set(ANSWERED);
    fixture
        .ok(
            &phone::phone_stamp(),
            "consent.answer",
            json!({"elicitationId":id,"choiceId":"allow_once"}),
        )
        .await;
    id
}

pub async fn generate() -> BTreeMap<String, Vec<u8>> {
    let fixture = Fixture::open(None).await;
    let requester = worker::stamp("cerebellum", ScopeKind::Head);
    let id = answered(&fixture, &requester).await;
    fixture
        .handler
        .set_access_runtime(worker::Runtime::new(Ok(ScopeLife::Gone)));
    fixture.handler.timer_tick().await.unwrap();
    let mut files = BTreeMap::new();
    for (name, stamp, params) in [
        ("requester-get", requester, json!({"elicitation_id":id})),
        (
            "phone-get",
            phone::phone_stamp(),
            json!({"elicitationId":id}),
        ),
    ] {
        let request = serde_json::to_vec(&json!({"method":"consent.get","params":params})).unwrap();
        let subc_client_rs::HandlerOutcome::Response(reply) =
            fixture.handler.handle_stamped(&stamp, &request).await
        else {
            panic!("session-ended vector get refused");
        };
        let result: Value = serde_json::from_slice(&reply).unwrap();
        assert_eq!(result["result"]["execution"], "approved_not_executed");
        let reason = if name == "phone-get" {
            "executionReason"
        } else {
            "execution_reason"
        };
        assert_eq!(result["result"][reason], "session_ended");
        files.insert(format!("{name}-request.json"), request);
        files.insert(format!("{name}-reply.json"), reply);
    }
    files
}
