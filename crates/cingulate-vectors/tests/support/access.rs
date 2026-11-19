use async_trait::async_trait;
use cingulate_store::{Isolation, ScopeIdentity, StorageBackend, StorageDescriptor};
use ck_cingulate::{
    AccessRuntime, Clock, Handler, NoticeResult, Options, ResolvedAgent, Resolver, ScopeLife, Stamp,
};
use serde_json::{json, Value};
use std::{collections::BTreeMap, sync::Arc};
use subc_client_rs::{HandlerOutcome, RouteBindRequest, RouteHandle};
use subc_protocol::{
    peer::{OperatorApp, RouteBindPeer},
    scope::{ScopeAttributes, ScopeKind, ScopeParent, ScopeStamp},
    BindIdentity, ModuleHelloAckBody, Principal, RouteTarget,
};

pub const CORPUS: &str = "tool-permission-v1.1";
pub fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test-vectors")
        .join(CORPUS)
}
struct FixedClock;
#[async_trait]
impl Clock for FixedClock {
    fn now_ms(&self) -> i64 {
        1000
    }
    async fn sleep_until(&self, _: i64) {
        panic!("vectors never sleep");
    }
}
struct Runtime;
#[async_trait]
impl AccessRuntime for Runtime {
    async fn describe(&self, _: &ScopeIdentity) -> Result<ScopeLife, String> {
        Ok(ScopeLife::Live(head()))
    }
    async fn notice(&self, _: Value) -> NoticeResult {
        NoticeResult::Delivered
    }
    fn is_macos(&self) -> bool {
        true
    }
}
fn principal(id: &str) -> Principal {
    Principal::Reserved {
        module_id: id.into(),
    }
}
fn head() -> ScopeIdentity {
    ScopeIdentity {
        scope_ref: "head".into(),
        scope_epoch: 7,
        agent_id: Some("agent".into()),
    }
}
fn asker(worker: bool) -> Stamp {
    Stamp {
        principal: Some("reserved:aft".into()),
        harness: "test-harness".into(),
        scope: Some(
            ScopeStamp::new(
                principal("prefrontal-core"),
                if worker { "worker" } else { "head" },
                if worker { 3 } else { 7 },
                if worker {
                    ScopeKind::Worker
                } else {
                    ScopeKind::Head
                },
            )
            .with_parent(worker.then(|| ScopeParent::new(principal("prefrontal-core"), "head", 7)))
            .with_attributes(if worker {
                ScopeAttributes::new()
            } else {
                ScopeAttributes::new().with_agent_id(Some("agent".into()))
            })
            .with_owner_authorized(true),
        ),
        ..Default::default()
    }
}
async fn capture(
    files: &mut BTreeMap<String, Vec<u8>>,
    handler: &Handler,
    stamp: &Stamp,
    name: &str,
    method: &str,
    params: Value,
) -> Value {
    let request = serde_json::to_vec(&json!({"method":method,"params":params})).unwrap();
    files.insert(format!("{name}-request.json"), request.clone());
    match handler.handle_stamped(stamp, &request).await {
        HandlerOutcome::Response(bytes) => {
            let value: Value = serde_json::from_slice(&bytes).unwrap();
            files.insert(format!("{name}-reply.json"), bytes);
            value["result"].clone()
        }
        HandlerOutcome::Error { code, message } => {
            files.insert(
                format!("{name}-refusal.json"),
                serde_json::to_vec(&json!({"code":code,"message":message})).unwrap(),
            );
            Value::Null
        }
        _ => panic!("unary vector required"),
    }
}
const NAMED_AGENT_ID: &str = "agent-example";
struct VectorNames;
#[async_trait]
impl Resolver for VectorNames {
    async fn resolve(&self, agent_id: &str) -> Result<Option<ResolvedAgent>, String> {
        Ok((agent_id == NAMED_AGENT_ID).then(|| ResolvedAgent {
            name: Some("Navigator".into()),
            retired: false,
        }))
    }
}

pub async fn generate() -> BTreeMap<String, Vec<u8>> {
    generate_with_resolver(Arc::new(VectorNames)).await
}
pub async fn generate_with_resolver(resolver: Arc<dyn Resolver>) -> BTreeMap<String, Vec<u8>> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cards.sqlite");
    let handler = Handler::new(Options {
        clock: Arc::new(FixedClock),
        resolver,
        sweep_timer: false,
        data_home: dir.path().to_str().unwrap().into(),
        ..Default::default()
    });
    handler.set_access_runtime(Arc::new(Runtime));
    handler
        .hello_ack(&ModuleHelloAckBody {
            negotiated_ver: 1,
            subc_ops: vec![],
            subc_capabilities: vec![],
            storage: Some(
                serde_json::to_value(StorageDescriptor {
                    module_id: "cingulate".into(),
                    storage_namespace: "main".into(),
                    isolation: Isolation::Module,
                    backend: StorageBackend::Sqlite {
                        path: path.to_str().unwrap().into(),
                    },
                })
                .unwrap(),
            ),
            machine_id: None,
        })
        .await;
    let mut files = BTreeMap::new();
    let core = Stamp::from_principal(Some(&principal("prefrontal-core")));
    let phone = Stamp {
        principal: Some("reserved:callosum".into()),
        operator_surface: true,
        harness: "fed-phone:test".into(),
        ..Default::default()
    };
    let worker = asker(true);
    let own = asker(false);
    files.insert("bind-stamps.json".into(),serde_json::to_vec(&json!({"worker":{"principal":"reserved:aft","scope":worker.scope,"harness":worker.harness},"head":{"principal":"reserved:aft","scope":own.scope,"harness":own.harness},"relay_principal":"reserved:prefrontal-core","phone_principal":"reserved:callosum","phone_harness":"fed-phone:test"})).unwrap());
    let req = json!({"kind":"tool_permission","call_key":"call-1","tool":"read","operation":"read","paths":["/tmp/file"],"reason":"Inspect file","args_hash":"hash","pattern":"/tmp/file","head_policy":"not_held"});
    let id = capture(
        &mut files,
        &handler,
        &worker,
        "worker-open",
        "consent.request",
        req.clone(),
    )
    .await["elicitation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    handler.deliver_access_notices().await.unwrap();
    capture(
        &mut files,
        &handler,
        &worker,
        "worker-get-head",
        "consent.get",
        json!({"elicitation_id":id}),
    )
    .await;
    capture(
        &mut files,
        &handler,
        &phone,
        "phone-head-leg",
        "consent.answer",
        json!({"elicitationId":id,"choiceId":"allow_once"}),
    )
    .await;
    capture(
        &mut files,
        &handler,
        &core,
        "head-not-held",
        "consent.answer",
        json!({"elicitation_id":id,"answer":"allow_once","answered_by":head()}),
    )
    .await;
    capture(&mut files,&handler,&core,"escalated","consent.answer",json!({"elicitation_id":id,"answer":"escalate","note":"Please decide","answered_by":head()})).await;
    handler.deliver_access_notices().await.unwrap();
    capture(
        &mut files,
        &handler,
        &phone,
        "phone-get-escalated",
        "consent.get",
        json!({"elicitationId":id}),
    )
    .await;
    capture(
        &mut files,
        &handler,
        &phone,
        "phone-list",
        "consent.list_pending",
        json!({}),
    )
    .await;
    capture(
        &mut files,
        &handler,
        &phone,
        "phone-settled",
        "consent.answer",
        json!({"elicitationId":id,"choiceId":"allow_scope"}),
    )
    .await;
    handler.deliver_access_notices().await.unwrap();
    capture(
        &mut files,
        &handler,
        &core,
        "late-head",
        "consent.answer",
        json!({"elicitation_id":id,"answer":"deny","answered_by":head()}),
    )
    .await;
    capture(
        &mut files,
        &handler,
        &worker,
        "replay",
        "consent.request",
        req,
    )
    .await;
    capture(
        &mut files,
        &handler,
        &worker,
        "await-settled",
        "consent.await",
        json!({"elicitation_id":id,"timeout_ms":0}),
    )
    .await;
    let own_req = json!({"kind":"tool_permission","call_key":"call-2","tool":"bash","operation":"exec","paths":[],"command":"pwd","reason":"Inspect directory","args_hash":"hash-2","pattern":"pwd"});
    let own_id = capture(
        &mut files,
        &handler,
        &own,
        "user-open",
        "consent.request",
        own_req,
    )
    .await["elicitation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    handler.deliver_access_notices().await.unwrap();
    capture(
        &mut files,
        &handler,
        &own,
        "await-pending",
        "consent.await",
        json!({"elicitation_id":own_id,"timeout_ms":0}),
    )
    .await;
    capture(
        &mut files,
        &handler,
        &own,
        "withdrawn",
        "consent.withdraw",
        json!({"elicitation_id":own_id}),
    )
    .await;
    handler.deliver_access_notices().await.unwrap();
    let mac = json!({"kind":"macos_permission","folder":"/protected","program":"Terminal","steps":"Open System Settings, Privacy & Security, Files and Folders"});
    let mac_id = capture(
        &mut files,
        &handler,
        &own,
        "macos-open",
        "consent.request",
        mac.clone(),
    )
    .await["elicitation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    capture(
        &mut files,
        &handler,
        &own,
        "macos-repeat",
        "consent.request",
        mac,
    )
    .await;
    capture(
        &mut files,
        &handler,
        &phone,
        "phone-macos",
        "consent.get",
        json!({"elicitationId":mac_id}),
    )
    .await;
    let db = rusqlite::Connection::open(&path).unwrap();
    let mut stmt = db
        .prepare("SELECT purpose,body_json FROM access_notices ORDER BY rowid")
        .unwrap();
    for row in stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .unwrap()
    {
        let (purpose, body) = row.unwrap();
        files.insert(format!("notice-{purpose}.json"), body.into_bytes());
    }
    let req = json!({"kind":"tool_permission","call_key":"call-phone-once","tool":"read","operation":"read","paths":["/x/y"],"reason":"Inspect the requested path","args_hash":"hash-phone-once","pattern":"/x/y","head_policy":"allow"});
    let id = capture(
        &mut files,
        &handler,
        &worker,
        "access-escalated-phone-open",
        "consent.request",
        req,
    )
    .await["elicitation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    capture(&mut files, &handler, &core, "access-escalated-phone-escalate", "consent.answer", json!({"elicitation_id": id, "answer": "escalate", "note": "Please approve this read", "answered_by": head()})).await;
    capture(
        &mut files,
        &handler,
        &phone,
        "access-escalated-phone-list",
        "consent.list_pending",
        json!({}),
    )
    .await;
    capture(
        &mut files,
        &handler,
        &phone,
        "access-escalated-phone-get",
        "consent.get",
        json!({"elicitationId": id}),
    )
    .await;
    capture(
        &mut files,
        &handler,
        &phone,
        "access-escalated-phone-answer",
        "consent.answer",
        json!({"elicitationId": id, "choiceId": "allow_once"}),
    )
    .await;
    capture(
        &mut files,
        &handler,
        &worker,
        "access-escalated-phone-readback",
        "consent.get",
        json!({"elicitation_id": id}),
    )
    .await;
    capture(
        &mut files,
        &handler,
        &phone,
        "access-escalated-phone-get-after-answer",
        "consent.get",
        json!({"elicitationId": id}),
    )
    .await;
    let unrelated = Stamp::from_principal(Some(&principal("basal")));
    files.insert(
        "withdraw-bind-stamps.json".into(),
        serde_json::to_vec(&json!({
            "scope-owner-withdraw":"reserved:prefrontal-core",
            "non-owner-withdraw":"reserved:basal",
            "settled-withdraw":"reserved:prefrontal-core",
            "worker":{"principal":worker.principal,"scope":worker.scope,"harness":worker.harness}
        }))
        .unwrap(),
    );
    let pending_id = capture(
        &mut files,
        &handler,
        &worker,
        "owner-worker-open",
        "consent.request",
        json!({"kind":"tool_permission","call_key":"call-owner-withdraw","tool":"read","operation":"read","paths":["/tmp/worker"],"reason":"Inspect worker file","args_hash":"hash-owner-withdraw","pattern":"/tmp/worker","head_policy":"allow"}),
    ).await["elicitation_id"].as_str().unwrap().to_owned();
    capture(
        &mut files,
        &handler,
        &unrelated,
        "non-owner-withdraw",
        "consent.withdraw",
        json!({"elicitation_id":pending_id}),
    )
    .await;
    capture(
        &mut files,
        &handler,
        &core,
        "scope-owner-withdraw",
        "consent.withdraw",
        json!({"elicitation_id":pending_id}),
    )
    .await;
    let withdrawn_body: String = db
        .query_row(
            "SELECT body_json FROM access_notices WHERE elicitation_id=?1 AND purpose='withdrawn'",
            [&pending_id],
            |row| row.get(0),
        )
        .unwrap();
    files.insert(
        "notice-owner-withdrawn.json".into(),
        withdrawn_body.into_bytes(),
    );
    capture(
        &mut files,
        &handler,
        &core,
        "settled-withdraw",
        "consent.withdraw",
        json!({"elicitation_id":id}),
    )
    .await;
    let id = capture(
        &mut files,
        &handler,
        &worker,
        "escalator-open",
        "consent.request",
        json!({"kind":"tool_permission","call_key":"call-escalator-deny","tool":"read","operation":"read","paths":["/tmp/alternative"],"reason":"Inspect file","args_hash":"hash-escalator-deny","pattern":"/tmp/alternative","head_policy":"allow"}),
    ).await["elicitation_id"].as_str().unwrap().to_owned();
    capture(
        &mut files, &handler, &core, "escalator-escalate", "consent.answer",
        json!({"elicitation_id":id,"answer":"escalate","note":"Please decide","answered_by":head()}),
    ).await;
    for (name, answer) in [
        ("escalator-allow-once", "allow_once"),
        ("escalator-allow-scope", "allow_scope"),
    ] {
        capture(
            &mut files,
            &handler,
            &core,
            name,
            "consent.answer",
            json!({"elicitation_id":id,"answer":answer,"answered_by":head()}),
        )
        .await;
    }
    let mut other_head = head();
    other_head.scope_ref = "other-head".into();
    capture(
        &mut files,
        &handler,
        &core,
        "escalator-wrong-head",
        "consent.answer",
        json!({"elicitation_id":id,"answer":"deny","answered_by":other_head}),
    )
    .await;
    capture(
        &mut files, &handler, &core, "escalator-deny", "consent.answer",
        json!({"elicitation_id":id,"answer":"deny","note":"Solved another way","answered_by":head()}),
    ).await;
    capture(
        &mut files,
        &handler,
        &phone,
        "escalator-phone-pending",
        "consent.list_pending",
        json!({}),
    )
    .await;
    capture(
        &mut files,
        &handler,
        &worker,
        "escalator-await",
        "consent.await",
        json!({"elicitation_id":id,"timeout_ms":0}),
    )
    .await;
    let settled_body: String = db
        .query_row(
            "SELECT body_json FROM access_notices WHERE elicitation_id=?1 AND purpose='settled'",
            [&id],
            |row| row.get(0),
        )
        .unwrap();
    files.insert(
        "notice-escalator-settled.json".into(),
        settled_body.into_bytes(),
    );
    capture(
        &mut files,
        &handler,
        &phone,
        "access-escalated-phone-get-after-head-deny",
        "consent.get",
        json!({"elicitationId": id}),
    )
    .await;
    capture(
        &mut files,
        &handler,
        &phone,
        "access-escalated-phone-get-never-escalated",
        "consent.get",
        json!({"elicitationId": pending_id}),
    )
    .await;
    let id = capture(
        &mut files,
        &handler,
        &worker,
        "access-escalated-phone-withdraw-open",
        "consent.request",
        json!({"kind":"tool_permission","call_key":"call-phone-withdraw","tool":"read","operation":"read","paths":["/x/y"],"reason":"Inspect the requested path","args_hash":"hash-phone-withdraw","pattern":"/x/y","head_policy":"allow"}),
    ).await["elicitation_id"].as_str().unwrap().to_owned();
    capture(
        &mut files,
        &handler,
        &core,
        "access-escalated-phone-withdraw-escalate",
        "consent.answer",
        json!({"elicitation_id":id,"answer":"escalate","answered_by":head()}),
    )
    .await;
    capture(
        &mut files,
        &handler,
        &worker,
        "access-escalated-phone-worker-withdraw",
        "consent.withdraw",
        json!({"elicitation_id": id}),
    )
    .await;
    capture(
        &mut files,
        &handler,
        &phone,
        "access-escalated-phone-get-after-worker-withdraw",
        "consent.get",
        json!({"elicitationId": id}),
    )
    .await;
    let bind = RouteBindRequest::new(
        RouteHandle::detached(1, 1),
        RouteTarget::ManagementSurface {
            module_id: "cingulate".into(),
        },
        BindIdentity::new("local-desktop", "desktop-session"),
    )
    .with_principal(Principal::Direct)
    .with_peer(
        RouteBindPeer::new().with_operator_app(Some(OperatorApp::new("alfonso-desktop", 5))),
    );
    files.insert(
        "operator-app-peer.json".into(),
        serde_json::to_vec(&bind.peer).unwrap(),
    );
    let id = capture(
        &mut files,
        &handler,
        &own,
        "operator-app-open",
        "consent.request",
        json!({"kind":"tool_permission","call_key":"call-operator-app","tool":"bash",
            "operation":"exec","paths":[],"command":"pwd","reason":"Inspect directory",
            "args_hash":"hash-operator-app","pattern":"pwd"}),
    )
    .await["elicitation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let answer = capture(
        &mut files,
        &handler,
        &Stamp::from_bind(&bind),
        "operator-app-answer",
        "consent.answer",
        json!({"elicitationId":id,"choiceId":"allow_once"}),
    )
    .await;
    assert_eq!(answer["ok"], true);
    let audit: String = db
        .query_row(
            "SELECT record_json FROM access_asks WHERE elicitation_id=?1",
            [&id],
            |row| row.get(0),
        )
        .unwrap();
    let value: Value = serde_json::from_str(&audit).unwrap();
    assert_eq!(value["answered_by"], "user");
    assert_eq!(value["attempts"][0]["surface"], "user");
    assert_eq!(value["attempts"][0]["via"], "operator_app");
    assert!(value["attempts"][0].get("device").is_none());
    files.insert("operator-app-audit.json".into(), audit.into_bytes());

    let mut named_head = asker(false);
    named_head.scope.as_mut().unwrap().attributes.agent_id = Some(NAMED_AGENT_ID.into());
    files.insert(
        "named-head-bind-stamp.json".into(),
        serde_json::to_vec(&json!({
            "principal": named_head.principal,
            "scope": named_head.scope,
            "harness": named_head.harness,
        }))
        .unwrap(),
    );
    let card = capture(
        &mut files,
        &handler,
        &named_head,
        "named-head-open",
        "consent.request",
        json!({"kind":"tool_permission","call_key":"call-named-head","tool":"read",
            "operation":"read","paths":["/example/notes"],"reason":"Inspect notes",
            "args_hash":"hash-named-head","pattern":"/example/notes"}),
    )
    .await;
    capture(
        &mut files,
        &handler,
        &phone,
        "named-head-phone-get",
        "consent.get",
        json!({"elicitationId":card["elicitation_id"]}),
    )
    .await;
    files
}
