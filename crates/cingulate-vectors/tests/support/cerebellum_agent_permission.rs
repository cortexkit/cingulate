use std::{collections::BTreeMap, time::Duration};

use ck_cingulate::{ScopeLife, Stamp};
use serde_json::{json, Value};
use subc_client_rs::HandlerOutcome;
use subc_protocol::scope::ScopeKind;

use crate::{
    phone,
    support::{self, worker, Fixture},
};

pub const CORPUS: &str = "cerebellum-agent-permission-v1";

const REQUESTS: [(&str, &[u8], ScopeKind); 4] = [
    (
        "head-app-control",
        include_bytes!("../../../../test-vectors/cerebellum-agent-permission-v1/requests/head-app-control.json"),
        ScopeKind::Head,
    ),
    (
        "head-takeover",
        include_bytes!("../../../../test-vectors/cerebellum-agent-permission-v1/requests/head-takeover.json"),
        ScopeKind::Head,
    ),
    (
        "head-website-public-in-call",
        include_bytes!("../../../../test-vectors/cerebellum-agent-permission-v1/requests/head-website-public-in-call.json"),
        ScopeKind::Head,
    ),
    (
        "worker-app-control",
        include_bytes!("../../../../test-vectors/cerebellum-agent-permission-v1/requests/worker-app-control.json"),
        ScopeKind::Worker,
    ),
];

async fn invoke(fixture: &Fixture, route: &Stamp, method: &str, params: &[u8]) -> Vec<u8> {
    let method = serde_json::to_string(method).unwrap();
    let params = std::str::from_utf8(params).expect("request params are UTF-8 JSON");
    let request = format!(r#"{{"method":{method},"params":{params}}}"#);
    match tokio::time::timeout(
        Duration::from_secs(1),
        fixture.handler.handle_stamped(route, request.as_bytes()),
    )
    .await
    .expect("Cerebellum permission handler operation exceeded 1 s")
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
    fixture
        .handler
        .set_access_runtime(worker::Runtime::new(Ok(ScopeLife::Live(worker::head()))));

    for (index, (kind, params, scope_kind)) in REQUESTS.into_iter().enumerate() {
        let name = format!("{kind}.json");
        files.insert(format!("requests/{name}"), params.to_vec());

        let route = worker::stamp("cerebellum", scope_kind);
        let reply = invoke(&fixture, &route, "consent.request", params).await;
        let id = request_id(&reply, kind);
        assert_eq!(id, format!("el_{}", index + 1));
        files.insert(format!("{kind}-request-reply.json"), reply);

        let get_params = serde_json::to_vec(&json!({
            "elicitationId": id,
            "clientFeatures": ["option-group"],
        }))
        .unwrap();
        files.insert(
            format!("{kind}-phone-get.json"),
            invoke(&fixture, &phone::phone_stamp(), "consent.get", &get_params).await,
        );
    }

    files.insert(
        "requests/SHA256SUMS".into(),
        include_bytes!(
            "../../../../test-vectors/cerebellum-agent-permission-v1/requests/SHA256SUMS"
        )
        .to_vec(),
    );
    files.insert(
        "list-pending.json".into(),
        invoke(
            &fixture,
            &phone::phone_stamp(),
            "consent.list_pending",
            br#"{"clientFeatures":["option-group"]}"#,
        )
        .await,
    );
    files
}

pub fn corpus() -> std::path::PathBuf {
    support::corpus(CORPUS)
}
