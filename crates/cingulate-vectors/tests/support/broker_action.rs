use std::{collections::BTreeMap, fs};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ck_cingulate::{Stamp, DEVELOPMENT_LOCAL_OPERATOR};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use subc_client_rs::{RouteBindRequest, RouteHandle};
use subc_protocol::{
    peer::{OperatorApp, RouteBindPeer},
    scope::{ScopeAttributes, ScopeKind, ScopeStamp},
    BindIdentity, Principal, RouteTarget,
};

use crate::support::{self, Fixture};

pub const CORPUS: &str = "broker-action-v1";
pub const NOW: i64 = 1_800_000_000_000;
pub const EXPIRY: i64 = NOW + 600_000;
pub const AGENT: &str = "agent-alf";
pub const FEATURE: &str = "broker-action-approval";
pub const DEVICE: &str = "fe0123456789abcdfe0123456789abcd";
const ZERO: &str = "00000000000000000000000000000000";
pub const REQUEST_ID: &str = "01HF7YAT000000000000000001";
pub const SECOND_ID: &str = "01HF7YAT000000000000000002";
const RESOURCE: &str = "example-garden/demo-notes";
const HOST: &str = "api.github.example";
const OVERSIZE_REPEAT: usize = 512 * 1024;

pub fn bind(harness: &str, principal: Principal) -> RouteBindRequest {
    RouteBindRequest::new(
        RouteHandle::detached(1, 1),
        RouteTarget::ManagementSurface {
            module_id: "cingulate".into(),
        },
        BindIdentity::new(harness, "ignored"),
    )
    .with_principal(principal)
}

fn reserved(module: &str) -> Principal {
    Principal::Reserved {
        module_id: module.into(),
    }
}

pub fn filer(module: &str, agent: Option<&str>, epoch: u64) -> Stamp {
    Stamp::from_bind(
        &bind("sender", reserved(module)).with_scope(
            ScopeStamp::new(reserved("aft"), "sample-head", epoch, ScopeKind::Head)
                .with_attributes(ScopeAttributes::new().with_agent_id(agent.map(str::to_owned)))
                .with_owner_authorized(true),
        ),
    )
}

pub fn phone() -> Stamp {
    Stamp::from_bind(&bind(&format!("fed-phone:{DEVICE}"), reserved("callosum")))
}

pub fn desktop() -> Stamp {
    let mut stamp = Stamp::from_bind(&bind("sample-desktop", Principal::Direct).with_peer(
        RouteBindPeer::new().with_operator_app(Some(OperatorApp::new("example-desktop", 5))),
    ));
    stamp.local_operator = false;
    stamp
}

pub async fn fixture() -> Fixture {
    let fixture = Fixture::open(None).await;
    fixture.clock.set(NOW);
    fixture
}

fn digest_inputs() -> Value {
    let one = |method: &str, target: &str, body: &str| {
        let mut hash = Sha256::new();
        hash.update(b"ck-broker-request/v1\0");
        for part in [method, HOST, target, body] {
            hash.update(u32::try_from(part.len()).unwrap().to_be_bytes());
            hash.update(part.as_bytes());
        }
        json!({"method":method,"host":HOST,"target":target,"body":body,"request_sha256":format!("{:x}", hash.finalize())})
    };
    json!({
        "close": one("PATCH", "/repos/example-garden/demo-notes/issues/7?notify=false", r#"{"state":"closed"}"#),
        "comment": one("POST", "/repos/example-garden/demo-notes/issues/7/comments", r#"{"body":"Closing the sample issue."}"#),
        "remove_label": one("DELETE", "/repos/example-garden/demo-notes/issues/7/labels/needs%20review", "")
    })
}

pub fn statement(legs: bool) -> Value {
    let digests = digest_inputs();
    let mut statement = json!({
        "v":"ck-broker-action-statement/v1", "request_id":REQUEST_ID, "agent_id":AGENT,
        "scope":{"owner":"reserved:aft", "kind":"head", "ref":"sample-head", "epoch":7},
        "service":"github", "identity":"github_app:example-garden", "action":"close_issue",
        "host":HOST, "resource":RESOURCE, "policy_version":7, "policy_sha256":"01".repeat(32),
        "fields":[{"name":"Issue", "value":7},{"name":"State", "value":"closed"}],
        "context":"example issue close 7", "expires_at_ms":EXPIRY
    });
    if legs {
        statement["request_id"] = json!(SECOND_ID);
        statement["action"] = json!("close_with_note");
        statement["legs"] = json!([
            {"kind":"close_issue", "method":"PATCH", "resource":RESOURCE, "request_sha256":digests["close"]["request_sha256"]},
            {"kind":"comment", "method":"POST", "resource":RESOURCE, "request_sha256":digests["comment"]["request_sha256"]},
            {"kind":"remove_label", "method":"DELETE", "resource":RESOURCE, "request_sha256":digests["remove_label"]["request_sha256"]}
        ]);
    } else {
        statement["kind"] = statement["action"].clone();
        statement["method"] = json!("PATCH");
        statement["request_sha256"] = digests["close"]["request_sha256"].clone();
    }
    statement
}

pub fn encode(value: &Value) -> String {
    URL_SAFE_NO_PAD.encode(serde_jcs::to_vec(value).unwrap())
}

fn encoded_filing(encoded: &str) -> Value {
    json!({"kind":"broker_action","broker_action":{"statement_b64url":encoded}})
}

pub fn filing(statement: &Value) -> Value {
    encoded_filing(&encode(statement))
}

pub fn get(id: &str) -> Value {
    json!({"elicitationId":id,"clientFeatures":[FEATURE]})
}

pub fn answer(id: &str, envelope: Option<&Value>) -> Value {
    let mut params = get(id);
    if let Some(envelope) = envelope {
        params["choiceId"] = json!("allow");
        params["envelope_b64url"] = json!(encode(envelope));
    } else {
        params["choiceId"] = json!("decline");
    }
    params
}

pub fn envelope(statement: &Value, fingerprint: &str) -> Value {
    let digest = format!(
        "{:x}",
        Sha256::digest(serde_jcs::to_vec(statement).unwrap())
    );
    json!({"v":"ck-broker-action-envelope/v1","request_id":statement["request_id"],"statement_sha256":digest,
        "decision":"allow","device_fingerprint":fingerprint,"issued_at_ms":NOW})
}

async fn capture(f: &Fixture, stamp: &Stamp, method: &str, params: Value) -> Value {
    let reply = match f.call(stamp, method, params.clone()).await {
        Ok(result) => json!({"result":result}),
        Err(error) => json!({"error":error}),
    };
    json!({"method":method,"params":params,"reply":reply})
}

fn id(case: &Value) -> &str {
    case["reply"]["result"]["elicitation_id"]
        .as_str()
        .unwrap_or_else(|| panic!("filing failed: {}", case["reply"]))
}

pub async fn cases() -> Value {
    let f = fixture().await;
    let sender = filer("aft", Some(AGENT), 7);
    let phone = phone();
    let single = statement(false);
    let filed = capture(&f, &sender, "consent.request", filing(&single)).await;
    let card = id(&filed).to_owned();
    let mut cases = json!({"file-one-leg":filed});
    cases["dedup"] = capture(&f, &sender, "consent.request", filing(&single)).await;
    cases["file-legs"] = capture(
        &f,
        &filer("plexus", Some(AGENT), 7),
        "consent.request",
        filing(&statement(true)),
    )
    .await;
    cases["phone-get"] = capture(&f, &phone, "consent.get", get(&card)).await;
    cases["desktop-get-refused"] = capture(&f, &desktop(), "consent.get", get(&card)).await;
    cases["unsupported-get"] =
        capture(&f, &phone, "consent.get", json!({"elicitationId":card})).await;
    cases["unsupported-answer"] = capture(&f, &phone, "consent.answer", json!({"elicitationId":card,"choiceId":"decline","clientFeatures":["manifest-binding-approval"]})).await;
    for (name, stamp) in [
        ("unauthorized-filer", filer("broca", Some(AGENT), 7)),
        ("missing-scope", support::stamp("reserved:aft")),
        ("mismatched-agent", filer("aft", Some("agent-other"), 7)),
        ("different-owner", filer("aft", Some(AGENT), 8)),
    ] {
        cases[name] = capture(&f, &stamp, "consent.request", filing(&single)).await;
    }
    let mut closed_filing = filing(&single);
    closed_filing["expires_in_ms"] = json!(800_000);
    cases["expiry-override-refused"] = capture(&f, &sender, "consent.request", closed_filing).await;
    for (name, invalid) in [
        (
            "invalid-encoding",
            encoded_filing(&format!("{}=", encode(&single))),
        ),
        (
            "noncanonical",
            encoded_filing(&URL_SAFE_NO_PAD.encode(serde_json::to_vec_pretty(&single).unwrap())),
        ),
    ] {
        cases[name] = capture(&f, &sender, "consent.request", invalid).await;
    }
    for (name, key, value) in [
        ("unknown-statement-key", "unexpected", json!(true)),
        (
            "too-many-fields",
            "fields",
            json!(vec![json!({"name":"Sample","value":0}); 17]),
        ),
        (
            "float",
            "fields",
            json!([{"name":"Sample","value":{"nested":[0.5]}}]),
        ),
        (
            "context-character",
            "context",
            json!("sample\u{202e}context"),
        ),
    ] {
        let mut invalid = single.clone();
        invalid[key] = value;
        cases[name] = capture(&f, &sender, "consent.request", filing(&invalid)).await;
    }
    let mut big = single.clone();
    big["fields"][0]["value"] = json!("x".repeat(OVERSIZE_REPEAT));
    let mut oversize = capture(&f, &sender, "consent.request", filing(&big)).await;
    // Keep the actual refusal, but describe the large input instead of embedding it.
    oversize.as_object_mut().unwrap().remove("params");
    oversize["params_recipe"] = json!({"from":"file-one-leg", "decoded_statement_pointer":"/fields/0/value", "repeat":"x", "count":OVERSIZE_REPEAT});
    cases["oversize"] = oversize;

    let approval = envelope(&single, DEVICE);
    for (key, value) in [
        ("v", json!("ck-broker-action-envelope/v2")),
        ("request_id", json!(SECOND_ID)),
        ("statement_sha256", json!("0".repeat(64))),
        ("device_fingerprint", json!("f".repeat(32))),
    ] {
        let mut bad = approval.clone();
        bad[key] = value;
        cases[format!("binding-{key}")] =
            capture(&f, &phone, "consent.answer", answer(&card, Some(&bad))).await;
    }
    cases["allow"] = capture(&f, &phone, "consent.answer", answer(&card, Some(&approval))).await;
    cases["dedup-after-answer"] = capture(&f, &sender, "consent.request", filing(&single)).await;
    cases["requester-allow"] = capture(
        &f,
        &sender,
        "consent.await",
        json!({"elicitation_id":card,"timeout_ms":0}),
    )
    .await;

    let f = fixture().await;
    let card = f.request(&sender, filing(&single)).await;
    cases["decline"] = capture(&f, &phone, "consent.answer", answer(&card, None)).await;
    cases["requester-decline"] =
        capture(&f, &sender, "consent.get", json!({"elicitation_id":card})).await;

    let f = fixture().await;
    let card = f.request(&sender, filing(&single)).await;
    f.clock.set(EXPIRY);
    cases["filing-at-expiry"] = capture(&f, &sender, "consent.request", filing(&single)).await;
    cases["answer-at-expiry"] =
        capture(&f, &phone, "consent.answer", answer(&card, Some(&approval))).await;
    f.clock.set(EXPIRY + 1);
    cases["answer-after-expiry"] = capture(&f, &phone, "consent.answer", answer(&card, None)).await;
    cases["requester-expiry"] = capture(
        &f,
        &sender,
        "consent.await",
        json!({"elicitation_id":card,"timeout_ms":0}),
    )
    .await;
    f.clock.set(EXPIRY + cingulate_store::RETENTION_MS - 1);
    cases["phone-get-missed"] = capture(&f, &phone, "consent.get", get(&card)).await;
    f.clock.set(EXPIRY + cingulate_store::RETENTION_MS);
    cases["phone-get-after-retention"] = capture(&f, &phone, "consent.get", get(&card)).await;

    let f = fixture().await;
    let card = f.request(&sender, filing(&single)).await;
    cases["withdraw"] = capture(
        &f,
        &sender,
        "consent.withdraw",
        json!({"elicitation_id":card}),
    )
    .await;
    json!({"request_digests":digest_inputs(), "cases":cases})
}

pub async fn development_cases() -> Value {
    let f = fixture().await;
    let local = Stamp::from_bind(&bind("sample-desktop", Principal::Direct));
    let single = statement(false);
    let id = f
        .request(&filer("aft", Some(AGENT), 7), filing(&single))
        .await;
    let get = capture(&f, &local, "consent.get", get(&id)).await;
    let mut params = answer(&id, Some(&envelope(&single, ZERO)));
    params["signature_b64url"] = json!("AQIDBA");
    let signed = capture(&f, &local, "consent.answer", params.clone()).await;
    params.as_object_mut().unwrap().remove("signature_b64url");
    let allow = capture(&f, &local, "consent.answer", params).await;
    json!({"desktop-get":get,"desktop-signed-refused":signed,"desktop-allow":allow})
}

pub fn bytes(value: &Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(value).unwrap();
    bytes.push(b'\n');
    bytes
}

pub async fn generate() -> BTreeMap<&'static str, Vec<u8>> {
    let root = support::corpus(CORPUS);
    let mut files = BTreeMap::from([
        ("README.md", fs::read(root.join("README.md")).unwrap()),
        ("cases.json", bytes(&cases().await)),
        (
            "development.json",
            if DEVELOPMENT_LOCAL_OPERATOR {
                bytes(&development_cases().await)
            } else {
                fs::read(root.join("development.json"))
                    .expect("generate with ck-cingulate/dev-local-operator first")
            },
        ),
    ]);
    let sums = files
        .iter()
        .map(|(name, contents)| format!("{:x}  {name}\n", Sha256::digest(contents)))
        .collect::<String>();
    files.insert("SHA256SUMS", sums.into_bytes());
    files
}
