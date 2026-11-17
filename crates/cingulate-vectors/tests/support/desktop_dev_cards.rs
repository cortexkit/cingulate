#![allow(dead_code)]

use std::{collections::BTreeMap, fs};

use ck_cingulate::{Stamp, DEVELOPMENT_LOCAL_OPERATOR};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use subc_client_rs::{HandlerOutcome, RouteBindRequest, RouteHandle};
use subc_protocol::{
    peer::{OperatorApp, RouteBindPeer},
    BindIdentity, Principal, RouteTarget,
};

use crate::{config_plan, flow_secrets, manifest_binding, secret_use, support};

pub const CORPUS: &str = "desktop-dev-cards-v1";
const ZERO: &str = "00000000000000000000000000000000";

fn bind() -> RouteBindRequest {
    RouteBindRequest::new(
        RouteHandle::detached(1, 1),
        RouteTarget::ManagementSurface {
            module_id: "cingulate".into(),
        },
        BindIdentity::new("local-desktop", "ignored"),
    )
    .with_principal(Principal::Direct)
}

async fn capture(f: &support::Fixture, stamp: &Stamp, method: &str, params: Value) -> Value {
    let reply = match f.call(stamp, method, params.clone()).await {
        Ok(result) => json!({"result":result}),
        Err(error) => json!({"error":error}),
    };
    json!({"method":method, "params":params, "reply":reply})
}

fn bytes(value: &Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(value).unwrap();
    bytes.push(b'\n');
    bytes
}

#[allow(clippy::assertions_on_constants)]
pub async fn generate() -> BTreeMap<String, Vec<u8>> {
    assert!(
        DEVELOPMENT_LOCAL_OPERATOR,
        "generator requires ck-cingulate/dev-local-operator"
    );
    let local = Stamp::from_bind(&bind());
    let mut desktop = Stamp::from_bind(&bind().with_peer(
        RouteBindPeer::new().with_operator_app(Some(OperatorApp::new("example-desktop", 5))),
    ));
    desktop.local_operator = false;
    let mut files = BTreeMap::new();
    for kind in ["secret-use", "flow-install", "manifest-binding"] {
        let f = if kind == "manifest-binding" {
            manifest_binding::fixture().await
        } else {
            secret_use::fixture().await
        };
        let (sender, body, feature) = match kind {
            "secret-use" => (
                secret_use::filer("aft", Some("agent-A")),
                secret_use::body("approval"),
                "secret-use-approval",
            ),
            "flow-install" => (
                secret_use::filer("prefrontal-core", None),
                flow_secrets::body(),
                "secret-use-approval",
            ),
            _ => (
                manifest_binding::filer(),
                manifest_binding::body(),
                "manifest-binding-approval",
            ),
        };
        let id = f.request(&sender, body.clone()).await;
        let features = json!([feature]);
        let list = capture(
            &f,
            &local,
            "consent.list_pending",
            json!({"clientFeatures":features}),
        )
        .await;
        assert_eq!(
            list["reply"]["result"]["records"].as_array().unwrap().len(),
            1
        );
        files.insert(format!("{kind}-list.json"), bytes(&list));
        let request = serde_json::to_vec(
            &json!({"method":"consent.list_pending", "params":{"clientFeatures":features}}),
        )
        .unwrap();
        let HandlerOutcome::Response(raw) = f.handler.handle_stamped(&local, &request).await else {
            panic!("list response expected");
        };
        let raw_value: Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(raw_value, list["reply"]);
        files.insert(format!("{kind}-list-reply.json"), raw);
        let hidden = capture(&f, &local, "consent.list_pending", json!({})).await;
        assert_eq!(hidden["reply"]["result"]["records"], json!([]));
        files.insert(format!("{kind}-missing-feature-list.json"), bytes(&hidden));
        let hidden = capture(
            &f,
            &desktop,
            "consent.list_pending",
            json!({"clientFeatures":features}),
        )
        .await;
        assert_eq!(hidden["reply"]["result"]["records"], json!([]));
        files.insert(format!("{kind}-operator-list.json"), bytes(&hidden));
        let mut envelope = match kind {
            "secret-use" => secret_use::decoded(&secret_use::vector("approval")["envelope_b64url"]),
            "flow-install" => flow_secrets::envelope(&body),
            _ => secret_use::decoded(&manifest_binding::sample("envelope")["envelope_b64url"]),
        };
        envelope["device_fingerprint"] = json!(ZERO);
        if kind == "secret-use" {
            envelope["issued_at_ms"] = json!(secret_use::NOW);
            envelope["grant_expires_at_ms"] = json!(secret_use::NOW + 600_000);
            envelope["duration"] = json!("once");
        }
        let choice = if kind == "flow-install" {
            "install"
        } else {
            "allow"
        };
        let allow = json!({"elicitationId":id,"choiceId":choice,"clientFeatures":features,"envelope_b64url":secret_use::encoded(&envelope)});
        let mut signed = allow.clone();
        signed["signature_b64url"] = json!("AQIDBA");
        let refused = capture(&f, &local, "consent.answer", signed).await;
        assert!(refused["reply"]["error"].is_object());
        files.insert(format!("{kind}-signature-refusal.json"), bytes(&refused));
        let allowed = capture(&f, &local, "consent.answer", allow).await;
        assert_eq!(allowed["reply"]["result"]["ok"], true);
        files.insert(format!("{kind}-allow.json"), bytes(&allowed));
        let decline_fixture = if kind == "manifest-binding" {
            manifest_binding::fixture().await
        } else {
            secret_use::fixture().await
        };
        let decline_id = decline_fixture.request(&sender, body).await;
        let declined = capture(
            &decline_fixture,
            &local,
            "consent.answer",
            json!({"elicitationId":decline_id,"choiceId":"decline","clientFeatures":features}),
        )
        .await;
        assert_eq!(declined["reply"]["result"]["ok"], true);
        files.insert(format!("{kind}-decline.json"), bytes(&declined));
    }
    let f = secret_use::fixture().await;
    f.request(&support::stamp("reserved:callosum"), config_plan::body())
        .await;
    let hidden = capture(
        &f,
        &local,
        "consent.list_pending",
        json!({"clientFeatures":["config-plan"]}),
    )
    .await;
    assert_eq!(hidden["reply"]["result"]["records"], json!([]));
    files.insert("config-plan-local-list.json".into(), bytes(&hidden));
    files.insert(
        "README.md".into(),
        fs::read(support::corpus(CORPUS).join("README.md")).unwrap(),
    );
    let sums: String = files
        .iter()
        .map(|(name, bytes)| format!("{:x}  {name}\n", Sha256::digest(bytes)))
        .collect();
    files.insert("SHA256SUMS".into(), sums.into_bytes());
    files
}
