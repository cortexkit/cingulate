use std::{collections::BTreeMap, fs, sync::Arc};

use async_trait::async_trait;
use ck_cingulate::{ResolvedAgent, Resolver, Stamp, DEVELOPMENT_LOCAL_OPERATOR};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use subc_client_rs::{RouteBindRequest, RouteHandle};
use subc_protocol::{
    peer::{OperatorApp, RouteBindPeer},
    scope::ScopeKind,
    BindIdentity, Principal, RouteTarget,
};

use crate::support::{self, worker, Fixture};

pub const CORPUS: &str = "sandbox-escape-v1";
pub const FEATURE: &str = "sandbox-escape-approval";
pub const AGENT: &str = "agent-lark";
pub const DEVICE: &str = "48ec112eb467c1045fbd00e05cc64f67";
pub const NOW: i64 = 1_800_000_000_000;
pub const EXPIRY: i64 = NOW + 600_000;

pub struct LarkResolver;

#[async_trait]
impl Resolver for LarkResolver {
    async fn resolve(&self, agent: &str) -> Result<Option<ResolvedAgent>, String> {
        assert_eq!(agent, AGENT);
        Ok(Some(ResolvedAgent {
            name: Some("Lark".into()),
            retired: false,
        }))
    }
}

pub async fn fixture() -> Fixture {
    let f = Fixture::open_with_resolver(
        None,
        cingulate_store::CHANGE_LOG_ROWS,
        Arc::new(LarkResolver),
    )
    .await;
    f.clock.set(NOW);
    f
}

pub fn filer(module: &str) -> Stamp {
    let mut stamp = worker::stamp(module, ScopeKind::Head);
    stamp.scope.as_mut().unwrap().attributes.agent_id = Some(AGENT.into());
    stamp.carried_agent_id = AGENT.into();
    stamp
}

pub fn from_worker() -> Stamp {
    let mut stamp = worker::stamp("aft", ScopeKind::Worker);
    stamp.scope.as_mut().unwrap().attributes.on_behalf_of_agent = Some(AGENT.into());
    stamp
}

fn bind(harness: &str, principal: Principal) -> RouteBindRequest {
    RouteBindRequest::new(
        RouteHandle::detached(1, 1),
        RouteTarget::ManagementSurface {
            module_id: "cingulate".into(),
        },
        BindIdentity::new(harness, "untrusted-session"),
    )
    .with_principal(principal)
}

fn phone_bind() -> RouteBindRequest {
    bind(
        &format!("fed-phone:{DEVICE}"),
        worker::principal("callosum"),
    )
}

pub fn phone() -> Stamp {
    Stamp::from_bind(&phone_bind())
}

pub fn production_phone() -> Stamp {
    Stamp::from_bind(
        &phone_bind().with_peer(RouteBindPeer::new().with_peer_build(Some("production".into()))),
    )
}

pub fn local_operator() -> Stamp {
    Stamp::from_bind(&bind("example-desktop", Principal::Direct))
}

pub fn desktop() -> Stamp {
    let mut stamp = Stamp::from_bind(&bind("example-desktop", Principal::Direct).with_peer(
        RouteBindPeer::new().with_operator_app(Some(OperatorApp::new("example-desktop", 4))),
    ));
    stamp.local_operator = false;
    stamp
}

pub fn filing(call_key: &str) -> Value {
    json!({"kind":"sandbox_escape", "sandbox_escape":{
        "call_key":call_key,
        "command":"  printf '%s\\n' 'Garden inventory'\n\tprintf '%s\\n' 'End of example'  ",
        "cwd":"/example/orchard",
        "shell":"/example/bin/sh",
        "escapes_level":"standard",
        "background":true,
        "reason":"Show the inventory report in the example folder.",
        "expires_in_ms":600_000
    }})
}

pub fn invalid_filing(field: &str, value: Value) -> Value {
    let mut filing = filing("invalid-example");
    filing["sandbox_escape"][field] = value;
    filing
}

pub fn get(id: &str) -> Value {
    json!({"elicitationId":id,"clientFeatures":[FEATURE]})
}

pub fn answer(id: &str, choice: &str) -> Value {
    let mut params = get(id);
    params["choiceId"] = json!(choice);
    params
}

async fn capture(f: &Fixture, stamp: &Stamp, method: &str, params: Value) -> Value {
    let reply = match f.call(stamp, method, params.clone()).await {
        Ok(result) => json!({"result":result}),
        Err(error) => json!({"error":error}),
    };
    json!({"method":method,"params":params,"reply":reply})
}

pub async fn cases() -> Value {
    let f = fixture().await;
    let aft = filer("aft");
    let worker = from_worker();
    let phone = phone();
    let file = capture(&f, &worker, "consent.request", filing("sample-worker")).await;
    let id = file["reply"]["result"]["elicitation_id"].as_str().unwrap();
    let mut cases = BTreeMap::new();
    cases.insert(
        "phone-get",
        capture(&f, &phone, "consent.get", get(id)).await,
    );
    cases.insert(
        "desktop-get",
        capture(&f, &desktop(), "consent.get", get(id)).await,
    );
    cases.insert(
        "desktop-answer-refused",
        capture(&f, &desktop(), "consent.answer", answer(id, "run_once")).await,
    );
    cases.insert(
        "head-answer-refused",
        capture(&f, &aft, "consent.answer", answer(id, "run_once")).await,
    );
    cases.insert(
        "unsupported-get",
        capture(&f, &phone, "consent.get", json!({"elicitationId":id})).await,
    );
    cases.insert(
        "unverified-run-once-refused",
        capture(&f, &phone, "consent.answer", answer(id, "run_once")).await,
    );
    cases.insert(
        "run-once",
        capture(
            &f,
            &production_phone(),
            "consent.answer",
            answer(id, "run_once"),
        )
        .await,
    );
    cases.insert(
        "replay",
        capture(&f, &worker, "consent.request", filing("sample-worker")).await,
    );
    cases.insert(
        "report-execution",
        capture(
            &f,
            &worker,
            "consent.report_execution",
            json!({"elicitation_id":id,"execution":"executed"}),
        )
        .await,
    );
    cases.insert("file-worker", file);

    for (name, field, value) in [
        ("unknown-key", "agent_id", json!("untrusted-agent")),
        ("long-command", "command", json!("x".repeat(2001))),
        (
            "control-character",
            "command",
            json!("printf example\u{001b}"),
        ),
        ("text-direction", "command", json!("printf example\u{202e}")),
        ("relative-cwd", "cwd", json!("example/orchard")),
        ("long-expiry", "expires_in_ms", json!(600_001)),
    ] {
        cases.insert(
            name,
            capture(&f, &aft, "consent.request", invalid_filing(field, value)).await,
        );
    }
    cases.insert(
        "wrong-filer",
        capture(
            &f,
            &filer("broca"),
            "consent.request",
            filing("wrong-filer"),
        )
        .await,
    );
    let mut missing_agent = filer("aft");
    missing_agent.scope = None;
    cases.insert(
        "missing-agent-stamp",
        capture(&f, &missing_agent, "consent.request", filing("no-stamp")).await,
    );

    let declined = f.request(&aft, filing("sample-decline")).await;
    cases.insert(
        "decline",
        capture(&f, &phone, "consent.answer", answer(&declined, "decline")).await,
    );
    let withdrawn = f.request(&aft, filing("sample-withdraw")).await;
    cases.insert(
        "withdraw",
        capture(
            &f,
            &aft,
            "consent.withdraw",
            json!({"elicitation_id":withdrawn}),
        )
        .await,
    );
    let expired = f.request(&aft, filing("sample-expire")).await;
    f.clock.set(EXPIRY);
    cases.insert(
        "answer-at-expiry",
        capture(
            &f,
            &production_phone(),
            "consent.answer",
            answer(&expired, "run_once"),
        )
        .await,
    );
    cases.insert(
        "requester-expired",
        capture(
            &f,
            &aft,
            "consent.await",
            json!({"elicitation_id":expired,"timeout_ms":0}),
        )
        .await,
    );
    json!(cases)
}

pub async fn development_cases() -> Value {
    let f = fixture().await;
    let id = f.request(&filer("aft"), filing("sample-desktop")).await;
    let desktop = local_operator();
    json!({
        "desktop-get": capture(&f, &desktop, "consent.get", get(&id)).await,
        "desktop-run-once": capture(&f, &desktop, "consent.answer", answer(&id, "run_once")).await,
    })
}

fn bytes(value: &Value) -> Vec<u8> {
    let mut out = serde_json::to_vec_pretty(value).unwrap();
    out.push(b'\n');
    out
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
    let sums: String = files
        .iter()
        .map(|(name, bytes)| format!("{:x}  {name}\n", Sha256::digest(bytes)))
        .collect();
    files.insert("SHA256SUMS", sums.into_bytes());
    files
}
