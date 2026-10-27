#![allow(dead_code)]

use std::collections::BTreeMap;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ck_cingulate::Stamp;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{
    secret_use::{decoded, encoded, filer, fixture, phone, DEVICE, NOW},
    support::Fixture,
};

pub const CORPUS: &str = "flow-install-secrets-v1";
// Claustrum 1ee946f, docs/wire-contract-v1.md: golden:statement.flow.
pub const GOLDEN: &str = r#"{"durations":["version"],"flow":{"code_hash":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","flow_id":"daily-fetch","install_id":"install-A","version":7},"member_id":"fed-computer:22222222222222222222222222222222","request_expires_at_ms":1700000600000,"request_id":"01HF7YAT000000000000000001","secret":{"incarnation":"00000000000000000000000000000149","kind":"agent_api_key","name":"key"},"use":{"host":"x.test","location":"header:X-Api-Key","methods":["GET"],"path_prefix":"/repos/a"},"v":"ck-secret-flow-statement/v1"}"#;

pub fn body() -> Value {
    let first: Value = serde_json::from_str(GOLDEN).unwrap();
    let mut second = first.clone();
    second["request_id"] = json!("01HF7YAT000000000000000002");
    second["secret"]["name"] = json!("other-key");
    second["secret"]["incarnation"] = json!("00000000000000000000000000000150");
    let items: Vec<_> = [first.clone(), second]
        .iter()
        .map(|s| json!({"request_id":s["request_id"],"statement_b64url":encoded(s)}))
        .collect();
    json!({
        "kind":"flow_install", "title":"Install daily-fetch", "prompt":"Review the flow and its vault access.",
        "options":[{"id":"install","label":"Install","effect":"grant"},{"id":"decline","label":"Decline","effect":"decline"}],
        "default":"decline", "on_expiry":"deny", "urgency":"normal", "material_damage":false,
        "late_execution":"notify_only", "scope":{}, "target":{"kind":"flow","label":"daily-fetch"},
        "flow_install":{"flow":"daily-fetch", "manifest":{"opaque":true}, "flow_identity":first["flow"], "secrets":items}
    })
}

pub fn envelope(request: &Value) -> Value {
    let items: Vec<_> = request["flow_install"]["secrets"].as_array().unwrap().iter().map(|s| {
        let bytes = URL_SAFE_NO_PAD.decode(s["statement_b64url"].as_str().unwrap()).unwrap();
        json!({"request_id":s["request_id"], "statement_sha256":format!("{:x}",Sha256::digest(bytes))})
    }).collect();
    json!({"v":"ck-secret-use-envelope/v1", "decision":"allow", "device_fingerprint":DEVICE, "duration":"version", "grant_expires_at_ms":0, "issued_at_ms":NOW, "items":items})
}

pub fn answer(id: &str, envelope: &Value) -> Value {
    json!({"elicitationId":id,"choiceId":"install","clientFeatures":["secret-use-approval"],"envelope_b64url":encoded(envelope)})
}

pub fn decline(id: &str) -> Value {
    json!({"elicitationId":id,"choiceId":"decline","clientFeatures":["secret-use-approval"]})
}

pub fn alter_statement(request: &mut Value, index: usize, change: impl FnOnce(&mut Value)) {
    let item = &mut request["flow_install"]["secrets"][index];
    let mut statement = decoded(&item["statement_b64url"]);
    change(&mut statement);
    item["statement_b64url"] = encoded(&statement);
}

pub fn filing_refusals() -> Vec<(&'static str, Value, &'static str)> {
    let mut cases = Vec::new();
    let mut r = body();
    alter_statement(&mut r, 0, |s| s["flow"]["version"] = json!(8));
    cases.push(("mismatched-flow", r, "elicitation_flow_identity_mismatch"));
    let mut r = body();
    alter_statement(&mut r, 0, |s| s["durations"] = json!(["hour"]));
    cases.push(("wrong-durations", r, "elicitation_invalid_flow_durations"));
    let mut r = body();
    r["flow_install"]["secrets"][1] = r["flow_install"]["secrets"][0].clone();
    cases.push(("duplicate-request", r, "elicitation_duplicate_request_id"));
    let mut r = body();
    r["flow_install"]["secrets"] = json!(vec![r["flow_install"]["secrets"][0].clone(); 17]);
    cases.push(("seventeen-entries", r, "elicitation_invalid_flow_secrets"));
    let mut r = body();
    r["flow_install"]
        .as_object_mut()
        .unwrap()
        .remove("flow_identity");
    cases.push((
        "missing-flow-identity",
        r,
        "elicitation_flow_secrets_pair_required",
    ));
    let mut r = body();
    alter_statement(&mut r, 0, |s| s["request_expires_at_ms"] = json!(NOW));
    cases.push(("expired-statement", r, "elicitation_expired"));
    cases
}

pub fn answer_refusals() -> Vec<(&'static str, Value, Stamp)> {
    let mut cases = Vec::new();
    let mut e = envelope(&body());
    e["items"].as_array_mut().unwrap().pop();
    cases.push(("envelope-subset", answer("el_1", &e), phone(DEVICE)));
    let mut e = envelope(&body());
    let mut item = e["items"][0].clone();
    item["request_id"] = json!("01HF7YAT000000000000000003");
    e["items"].as_array_mut().unwrap().push(item);
    cases.push(("envelope-extra", answer("el_1", &e), phone(DEVICE)));
    cases.push((
        "another-phone",
        answer("el_1", &envelope(&body())),
        phone("22222222222222222222222222222222"),
    ));
    cases.push((
        "non-phone-answer",
        decline("el_1"),
        crate::support::operator(),
    ));
    cases
}

pub async fn capture(f: &Fixture, stamp: &Stamp, method: &str, params: Value) -> Value {
    let reply = match f.call(stamp, method, params.clone()).await {
        Ok(reply) => json!({"result":reply}),
        Err(error) => json!({"error":error}),
    };
    json!({"method":method,"params":params,"reply":reply})
}

pub async fn generate() -> BTreeMap<String, Vec<u8>> {
    let mut values = BTreeMap::new();
    let sender = filer("prefrontal-core", None);
    let f = fixture().await;
    values.insert(
        "filing.json",
        capture(&f, &sender, "consent.request", body()).await,
    );
    values.insert(
        "phone-get.json",
        capture(
            &f,
            &phone(DEVICE),
            "consent.get",
            json!({"elicitationId":"el_1","clientFeatures":["secret-use-approval"]}),
        )
        .await,
    );
    values.insert(
        "allow.json",
        capture(
            &f,
            &phone(DEVICE),
            "consent.answer",
            answer("el_1", &envelope(&body())),
        )
        .await,
    );
    let f = fixture().await;
    f.request(&sender, body()).await;
    values.insert(
        "decline.json",
        capture(&f, &phone(DEVICE), "consent.answer", decline("el_1")).await,
    );
    for (name, request, _) in filing_refusals() {
        let f = fixture().await;
        values.insert(name, capture(&f, &sender, "consent.request", request).await);
    }
    for (name, params, stamp) in answer_refusals() {
        let f = fixture().await;
        f.request(&sender, body()).await;
        values.insert(name, capture(&f, &stamp, "consent.answer", params).await);
    }
    values
        .into_iter()
        .map(|(name, value)| {
            let name = if name.ends_with(".json") {
                name.to_owned()
            } else {
                format!("{name}.json")
            };
            let mut bytes = serde_json::to_vec_pretty(&value).unwrap();
            bytes.push(b'\n');
            (name, bytes)
        })
        .collect()
}
