use async_trait::async_trait;
use cingulate_store::{Isolation, ScopeIdentity, StorageBackend, StorageDescriptor};
use ck_cingulate::{
    AccessRuntime, Clock, Handler, NoticeResult, Options, Resolver, ScopeLife, Stamp,
    UnavailableResolver,
};
use serde_json::{json, Value};
use std::{collections::BTreeMap, sync::Arc};
use subc_client_rs::HandlerOutcome;
use subc_protocol::{
    scope::{ScopeAttributes, ScopeKind, ScopeParent, ScopeStamp},
    ModuleHelloAckBody, Principal,
};

pub const CORPUS: &str = "tool-permission-v1";
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
        scope: Some(ScopeStamp {
            owner: principal("prefrontal-core"),
            scope_ref: if worker { "worker" } else { "head" }.into(),
            scope_epoch: if worker { 3 } else { 7 },
            kind: if worker {
                ScopeKind::Worker
            } else {
                ScopeKind::Head
            },
            parent: worker.then(|| ScopeParent::new(principal("prefrontal-core"), "head", 7)),
            parent_state: None,
            attributes: if worker {
                ScopeAttributes::new()
            } else {
                ScopeAttributes::new().with_agent_id(Some("agent".into()))
            },
            owner_authorized: true,
        }),
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
pub async fn generate() -> BTreeMap<String, Vec<u8>> {
    generate_with_resolver(Arc::new(UnavailableResolver)).await
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
    files
}
