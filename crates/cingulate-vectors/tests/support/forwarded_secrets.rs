#![allow(dead_code)]
//! v2 vault statements: requests an agent on another paired computer made,
//! forwarded to the home computer's vault. A v2 statement is the v1 object plus
//! `asking_member_id`, with `v` ending in `/v2`. The vault's own vectors are
//! vendored byte for byte; everything else here is built from them.

use std::collections::BTreeMap;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{
    flow_secrets,
    secret_use::{decoded, encoded, filer, fixture, phone, DEVICE, NOW},
    support::Fixture,
};

pub const USE_CORPUS: &str = "secret-use-approval-v2";
pub const FLOW_CORPUS: &str = "flow-install-secrets-v2";
pub const USE_VAULT_FILE: &str = "secret-approval-v2.json";
pub const FLOW_VAULT_FILE: &str = "secret-flow-approval-v2.json";
// Claustrum af71c1648da4aeccd624e84894c4a6671c78b676,
// crates/credentials-core/tests/vectors/<file>.
pub const USE_VAULT_SHA256: &str =
    "6117ab23408df9aa3ae901ddab58b8ea395ba310e350f4ec8b4e5e9ed338c26a";
pub const FLOW_VAULT_SHA256: &str =
    "05ec7aef2351f58e7e4151526425c5be70d678bfba85931ccddd0f4de246551c";
pub const USE_CASES: [&str; 2] = ["forwarded-use-api-key", "forwarded-use-site-login"];
pub const FLOW_CASES: [&str; 2] = ["forwarded-flow-api-key", "forwarded-flow-site-login"];
/// A third paired computer, used to give one statement on a card a different
/// asker than the others.
pub const OTHER_ASKER: &str = "fed-computer:33333333333333333333333333333333";

pub fn vault(case: &str) -> Value {
    let file: Value = if case.starts_with("forwarded-flow") {
        serde_json::from_slice(include_bytes!(
            "../../../../test-vectors/flow-install-secrets-v2/secret-flow-approval-v2.json"
        ))
    } else {
        serde_json::from_slice(include_bytes!(
            "../../../../test-vectors/secret-use-approval-v2/secret-approval-v2.json"
        ))
    }
    .unwrap();
    file["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["case"] == case)
        .unwrap()
        .clone()
}

fn items(cases: &[&str]) -> Value {
    json!(cases
        .iter()
        .map(|case| {
            let v = vault(case);
            json!({"request_id":v["request_id"],"statement_b64url":v["statement_b64url"]})
        })
        .collect::<Vec<_>>())
}

/// A secret_use filing carrying the vault's statements for `cases`, unchanged.
pub fn use_body(cases: &[&str]) -> Value {
    json!({"kind":"secret_use","secret_use":{"items":items(cases)}})
}

/// A flow_install filing carrying the vault's statements for `cases`,
/// unchanged, with `flow_identity` equal to their shared flow object.
pub fn flow_body(cases: &[&str]) -> Value {
    let mut body = flow_secrets::body();
    body["flow_install"]["flow_identity"] = vault(cases[0])["statement"]["flow"].clone();
    body["flow_install"]["secrets"] = items(cases);
    body
}

/// Every statement on a card, wherever the card keeps them.
fn entries(request: &mut Value) -> &mut Vec<Value> {
    if request.get("secret_use").is_some() {
        request["secret_use"]["items"].as_array_mut().unwrap()
    } else {
        request["flow_install"]["secrets"].as_array_mut().unwrap()
    }
}

/// Re-encodes statement `index` after `change`, keeping canonical bytes.
pub fn alter(request: &mut Value, index: usize, change: impl FnOnce(&mut Value)) {
    let item = &mut entries(request)[index];
    let mut statement = decoded(&item["statement_b64url"]);
    change(&mut statement);
    item["statement_b64url"] = encoded(&statement);
}

/// The phone's allow envelope over every statement on a secret_use card.
pub fn use_envelope(request: &Value) -> Value {
    let items: Vec<_> = request["secret_use"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            let bytes = URL_SAFE_NO_PAD
                .decode(s["statement_b64url"].as_str().unwrap())
                .unwrap();
            json!({"request_id":s["request_id"],"statement_sha256":format!("{:x}", Sha256::digest(bytes))})
        })
        .collect();
    json!({"v":"ck-secret-use-envelope/v1","decision":"allow","device_fingerprint":DEVICE,"duration":"hour","grant_expires_at_ms":NOW + 3_600_000,"issued_at_ms":NOW,"items":items})
}

pub fn use_answer(id: &str, envelope: &Value) -> Value {
    json!({"elicitationId":id,"choiceId":"allow","clientFeatures":["secret-use-approval"],"envelope_b64url":encoded(envelope)})
}

pub fn use_decline(id: &str) -> Value {
    json!({"elicitationId":id,"choiceId":"decline","clientFeatures":["secret-use-approval"]})
}

/// Re-encodes every statement on the card after `change`.
pub fn alter_all(request: &mut Value, change: impl Fn(&mut Value)) {
    for index in 0..entries(request).len() {
        alter(request, index, &change);
    }
}

fn to_v1(statement: &mut Value) {
    let v = statement["v"].as_str().unwrap().replace("/v2", "/v1");
    statement["v"] = json!(v);
}

/// Named filings that must be refused, built from a valid two-statement v2
/// card by changing only what the name says. Each change applies to every
/// statement unless the name is about mixing, so the card stays internally
/// consistent and only the named rule can refuse it.
pub fn refusals(body: fn() -> Value, other_family: &str) -> Vec<(&'static str, Value)> {
    let mut cases: Vec<(&'static str, Value)> = Vec::new();
    let mut every = |name, change: &dyn Fn(&mut Value)| {
        let mut r = body();
        alter_all(&mut r, change);
        cases.push((name, r));
    };
    every("v1-with-asking-member", &to_v1);
    every("v1-with-null-asking-member", &|s| {
        to_v1(s);
        s["asking_member_id"] = Value::Null;
    });
    every("v2-without-asking-member", &|s| {
        s.as_object_mut().unwrap().remove("asking_member_id");
    });
    for (name, asking) in [
        ("asking-member-null", Value::Null),
        (
            "asking-member-uppercase",
            json!(format!("fed-computer:{}", "A".repeat(32))),
        ),
        (
            "asking-member-short",
            json!(format!("fed-computer:{}", "1".repeat(31))),
        ),
        (
            "asking-member-long",
            json!(format!("fed-computer:{}", "1".repeat(33))),
        ),
        (
            "asking-member-not-computer",
            json!(format!("fed-phone:{}", "1".repeat(32))),
        ),
        ("asking-member-bare-hex", json!("1".repeat(32))),
        ("asking-member-number", json!(1)),
    ] {
        every(name, &|s| s["asking_member_id"] = asking.clone());
    }
    every("asking-member-is-serving-member", &|s| {
        s["asking_member_id"] = s["member_id"].clone()
    });
    every("other-family-version", &|s| s["v"] = json!(other_family));
    let mut r = body();
    alter(&mut r, 1, |s| s["asking_member_id"] = json!(OTHER_ASKER));
    cases.push(("mixed-asking-members", r));
    let mut r = body();
    alter(&mut r, 1, |s| {
        to_v1(s);
        s.as_object_mut().unwrap().remove("asking_member_id");
    });
    cases.push(("mixed-v1-and-v2", r));
    cases
}

pub fn use_refusals() -> Vec<(&'static str, Value)> {
    refusals(|| use_body(&USE_CASES), "ck-secret-flow-statement/v2")
}

pub fn flow_refusals() -> Vec<(&'static str, Value)> {
    refusals(|| flow_body(&FLOW_CASES), "ck-secret-use-statement/v2")
}

fn pretty(value: &Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(value).unwrap();
    bytes.push(b'\n');
    bytes
}

pub fn check_published_refusals(name: &str, pinned: &[u8], current: &[u8]) {
    let field = match name {
        "filer-refusals.json" => "secret_use.items",
        "filing-refusals.json" => "flow_install.secrets",
        _ => panic!("not a forwarded refusal file: {name}"),
    };
    let message = format!("{field}[1] statement asking_member_id must match the first statement, including when absent");
    crate::secret_use::check_refusal_messages(
        pinned,
        current,
        &[
            ("mixed-asking-members", &message),
            ("mixed-v1-and-v2", &message),
        ],
    );
}

async fn refusal_file(sender: &ck_cingulate::Stamp, cases: Vec<(&str, Value)>) -> Vec<u8> {
    let mut all = Vec::new();
    for (name, request) in cases {
        let f = fixture().await;
        let mut captured = flow_secrets::capture(&f, sender, "consent.request", request).await;
        captured["name"] = json!(name);
        all.push(captured);
    }
    pretty(&json!({"cases": all}))
}

/// Phone reads and filer replies for v2 secret_use cards, all from the real
/// handler in fresh scratch stores.
pub async fn generate_use() -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let sender = filer("aft", Some("agent-A"));
    // Phone reads keep the handler's untouched response bytes, as v1 does.
    for (cases, name) in [
        (&USE_CASES[..1], "single-phone-get.json"),
        (&USE_CASES[..], "batch-phone-get.json"),
    ] {
        let f = fixture().await;
        let id = f.request(&sender, use_body(cases)).await;
        let request = serde_json::to_vec(&json!({"method":"consent.get","params":{"elicitationId":id,"clientFeatures":["secret-use-approval"]}})).unwrap();
        let subc_client_rs::HandlerOutcome::Response(bytes) =
            f.handler.handle_stamped(&phone(DEVICE), &request).await
        else {
            panic!("phone get failed")
        };
        files.insert(name.into(), bytes);
    }
    let f = fixture().await;
    let body = use_body(&USE_CASES);
    let filing = flow_secrets::capture(&f, &sender, "consent.request", body.clone()).await;
    let id = filing["reply"]["result"]["elicitation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    files.insert("filer-request.json".into(), pretty(&filing));
    files.insert(
        "phone-allow.json".into(),
        pretty(
            &flow_secrets::capture(
                &f,
                &phone(DEVICE),
                "consent.answer",
                use_answer(&id, &use_envelope(&body)),
            )
            .await,
        ),
    );
    files.insert(
        "filer-await-allowed.json".into(),
        pretty(&filer_await(&f, &sender, &id).await),
    );
    let f = fixture().await;
    let id = f.request(&sender, body).await;
    f.ok(&phone(DEVICE), "consent.answer", use_decline(&id))
        .await;
    files.insert(
        "filer-await-declined.json".into(),
        pretty(&filer_await(&f, &sender, &id).await),
    );
    files.insert(
        "filer-refusals.json".into(),
        refusal_file(&sender, use_refusals()).await,
    );
    files
}

async fn filer_await(f: &Fixture, sender: &ck_cingulate::Stamp, id: &str) -> Value {
    flow_secrets::capture(
        f,
        sender,
        "consent.await",
        json!({"elicitation_id": id, "timeout_ms": 0}),
    )
    .await
}

/// Filing, phone and answer samples for a flow_install card carrying v2 flow
/// statements, all from the real handler in fresh scratch stores.
pub async fn generate_flow() -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let sender = filer("prefrontal-core", None);
    let body = flow_body(&FLOW_CASES);
    let f = fixture().await;
    files.insert(
        "filing.json".into(),
        pretty(&flow_secrets::capture(&f, &sender, "consent.request", body.clone()).await),
    );
    files.insert(
        "phone-get.json".into(),
        pretty(
            &flow_secrets::capture(
                &f,
                &phone(DEVICE),
                "consent.get",
                json!({"elicitationId":"el_1","clientFeatures":["secret-use-approval"]}),
            )
            .await,
        ),
    );
    files.insert(
        "allow.json".into(),
        pretty(
            &flow_secrets::capture(
                &f,
                &phone(DEVICE),
                "consent.answer",
                flow_secrets::answer("el_1", &flow_secrets::envelope(&body)),
            )
            .await,
        ),
    );
    let f = fixture().await;
    f.request(&sender, body).await;
    files.insert(
        "decline.json".into(),
        pretty(
            &flow_secrets::capture(
                &f,
                &phone(DEVICE),
                "consent.answer",
                flow_secrets::decline("el_1"),
            )
            .await,
        ),
    );
    files.insert(
        "filing-refusals.json".into(),
        refusal_file(&sender, flow_refusals()).await,
    );
    files
}
