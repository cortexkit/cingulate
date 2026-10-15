use std::{collections::BTreeMap, time::Duration};

use ck_cingulate::Stamp;
use serde_json::{json, Value};
use subc_client_rs::HandlerOutcome;

use crate::{
    phone,
    support::{stamp, Fixture},
};

pub const CORPUS: &str = "flow-decision-phone-v1";

const DECISIONS: [(&str, &[u8]); 3] = [
    (
        "reconcile",
        include_bytes!("../inputs/flow-decision-phone-v1/decision-reconcile-request.json"),
    ),
    (
        "reenable",
        include_bytes!("../inputs/flow-decision-phone-v1/decision-reenable-request.json"),
    ),
    (
        "grant-lost",
        include_bytes!("../inputs/flow-decision-phone-v1/decision-grant-lost-request.json"),
    ),
];

async fn invoke(fixture: &Fixture, route: &Stamp, method: &str, params: &[u8]) -> Vec<u8> {
    let method = serde_json::to_string(method).unwrap();
    let params = std::str::from_utf8(params).expect("request params are UTF-8 JSON");
    // Keep the supplied body bytes intact inside the handler envelope.
    let request = format!(r#"{{"method":{method},"params":{params}}}"#);
    match tokio::time::timeout(
        Duration::from_secs(1),
        fixture.handler.handle_stamped(route, request.as_bytes()),
    )
    .await
    .expect("flow-decision handler operation exceeded 1 s")
    {
        HandlerOutcome::Response(bytes) => bytes,
        other => panic!("{method}: {other:?}"),
    }
}

fn request_id(reply: &[u8], kind: &str) -> String {
    let envelope: Value = serde_json::from_slice(reply).unwrap();
    envelope["result"]["elicitation_id"]
        .as_str()
        .unwrap_or_else(|| panic!("consent.request refused {kind}: {envelope}"))
        .to_owned()
}

pub async fn generate() -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let fixture = Fixture::open(None).await;
    let basal = stamp("reserved:basal");

    for (index, (kind, params)) in DECISIONS.into_iter().enumerate() {
        let request_name = format!("decision-{kind}-request.json");
        files.insert(request_name, params.to_vec());

        let reply = invoke(&fixture, &basal, "consent.request", params).await;
        let id = request_id(&reply, kind);
        assert_eq!(id, format!("el_{}", index + 1));
        files.insert(format!("decision-{kind}-request-reply.json"), reply);

        let get_params = serde_json::to_vec(&json!({"elicitationId":id})).unwrap();
        files.insert(
            format!("decision-{kind}-phone-get.json"),
            invoke(&fixture, &phone::phone_stamp(), "consent.get", &get_params).await,
        );
    }

    files.insert(
        "list-pending.json".into(),
        invoke(
            &fixture,
            &phone::phone_stamp(),
            "consent.list_pending",
            b"{}",
        )
        .await,
    );
    files
}
