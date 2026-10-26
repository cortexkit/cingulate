#![allow(dead_code)]

use std::collections::BTreeMap;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ck_cingulate::{Stamp, UnavailableResolver};
use serde_json::{json, Value};
use subc_client_rs::{HandlerOutcome, RouteBindRequest, RouteHandle};
use subc_protocol::{
    scope::{ScopeAttributes, ScopeKind, ScopeStamp},
    BindIdentity, Principal, RouteTarget,
};

use crate::support::Fixture;

pub const CORPUS: &str = "secret-use-approval-v1";
pub const DEVICE: &str = "11111111111111111111111111111111";
pub const NOW: i64 = 1_700_000_000_000;

pub fn vector(case: &str) -> Value {
    let all: Value = serde_json::from_slice(include_bytes!(
        "../../../../test-vectors/secret-use-approval-v1/secret-approval-v1.json"
    ))
    .unwrap();
    all["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["case"] == case)
        .unwrap()
        .clone()
}

pub fn body(case: &str) -> Value {
    let items: Vec<_> = vector(case)["statements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| json!({"request_id":s["request_id"],"statement_b64url":s["statement_b64url"]}))
        .collect();
    json!({"kind":"secret_use","secret_use":{"items":items}})
}

pub fn filer(module: &str, agent: Option<&str>) -> Stamp {
    Stamp::from_bind(
        &RouteBindRequest::new(
            RouteHandle::detached(1, 1),
            RouteTarget::ManagementSurface {
                module_id: "cingulate".into(),
            },
            BindIdentity::new("/scratch", "sender", "ignored"),
        )
        .with_principal(Principal::Reserved {
            module_id: module.into(),
        })
        .with_scope(
            ScopeStamp::new(
                Principal::Reserved {
                    module_id: "prefrontal-core".into(),
                },
                "H",
                7,
                ScopeKind::Head,
            )
            .with_attributes(ScopeAttributes::new().with_agent_id(agent.map(str::to_owned)))
            .with_owner_authorized(true),
        ),
    )
}

pub fn phone(device: &str) -> Stamp {
    Stamp::from_bind(
        &RouteBindRequest::new(
            RouteHandle::detached(1, 1),
            RouteTarget::ManagementSurface {
                module_id: "cingulate".into(),
            },
            BindIdentity::new("/scratch", format!("fed-phone:{device}"), "ignored"),
        )
        .with_principal(Principal::Reserved {
            module_id: "callosum".into(),
        }),
    )
}

pub fn answer(id: &str, case: &str) -> Value {
    json!({"elicitationId":id,"choiceId":"allow","clientFeatures":["secret-use-approval"],"envelope_b64url":vector(case)["envelope_b64url"]})
}

pub fn decoded(encoded: &Value) -> Value {
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(encoded.as_str().unwrap()).unwrap()).unwrap()
}

pub fn encoded(value: &Value) -> Value {
    json!(URL_SAFE_NO_PAD.encode(serde_jcs::to_vec(value).unwrap()))
}

pub async fn fixture() -> Fixture {
    let f = Fixture::open_with_resolver(
        None,
        cingulate_store::CHANGE_LOG_ROWS,
        std::sync::Arc::new(UnavailableResolver),
    )
    .await;
    f.clock.set(NOW);
    f
}

pub async fn generate() -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    for (case, name) in [
        ("approval", "single-phone-get.json"),
        ("batch-omitted", "batch-phone-get.json"),
    ] {
        let f = fixture().await;
        let id = f.request(&filer("aft", Some("agent-A")), body(case)).await;
        let request = serde_json::to_vec(&json!({"method":"consent.get","params":{"elicitationId":id,"clientFeatures":["secret-use-approval"]}})).unwrap();
        let HandlerOutcome::Response(bytes) =
            f.handler.handle_stamped(&phone(DEVICE), &request).await
        else {
            panic!("phone get failed")
        };
        files.insert(name.into(), bytes);
    }
    files
}
