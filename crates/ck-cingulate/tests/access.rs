use async_trait::async_trait;
use cingulate_store::{Isolation, ScopeIdentity, StorageBackend, StorageDescriptor};
use ck_cingulate::{
    AccessRuntime, AwaitHook, Clock, Handler, NoticeResult, Notifier, Options, ScopeLife, Stamp,
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use subc_client_rs::{HandlerOutcome, RouteBindRequest, RouteHandle};
use subc_protocol::{
    scope::{ScopeAttributes, ScopeKind, ScopeParent, ScopeStamp},
    BindIdentity, ModuleHelloAckBody, Principal, RouteTarget,
};
use tokio::sync::watch;

struct TestClock(watch::Sender<i64>);
#[async_trait]
impl Clock for TestClock {
    fn now_ms(&self) -> i64 {
        *self.0.borrow()
    }
    async fn sleep_until(&self, until: i64) {
        let mut rx = self.0.subscribe();
        while *rx.borrow_and_update() < until {
            rx.changed().await.unwrap();
        }
    }
}
struct Runtime {
    life: Mutex<ScopeLife>,
    replies: Mutex<Vec<NoticeResult>>,
    notices: Mutex<Vec<Value>>,
    macos: bool,
}
struct WaitHook(watch::Sender<usize>);
#[derive(Default)]
struct PhoneNotices(Mutex<Vec<Value>>);
#[async_trait]
impl Notifier for PhoneNotices {
    async fn notify(&self, pointer: Value) -> Result<(), String> {
        self.0.lock().unwrap().push(pointer);
        Ok(())
    }
}
#[async_trait]
impl AwaitHook for WaitHook {
    async fn after_register(&self, _: &Handler, _: &str) {
        self.0.send_modify(|n| *n += 1);
    }
}
#[async_trait]
impl AccessRuntime for Runtime {
    async fn describe(&self, _: &ScopeIdentity) -> Result<ScopeLife, String> {
        Ok(self.life.lock().unwrap().clone())
    }
    async fn notice(&self, body: Value) -> NoticeResult {
        self.notices.lock().unwrap().push(body);
        let mut replies = self.replies.lock().unwrap();
        if replies.is_empty() {
            NoticeResult::Delivered
        } else {
            replies.remove(0)
        }
    }
    fn is_macos(&self) -> bool {
        self.macos
    }
}
fn principal(id: &str) -> Principal {
    Principal::Reserved {
        module_id: id.into(),
    }
}
fn head() -> ScopeIdentity {
    ScopeIdentity {
        scope_ref: "head-session".into(),
        scope_epoch: 7,
        agent_id: Some("agent-head".into()),
    }
}
fn stamp(module: &str, kind: ScopeKind) -> Stamp {
    let scope = ScopeStamp {
        owner: principal("prefrontal-core"),
        scope_ref: format!("{kind:?}-session"),
        scope_epoch: 3,
        kind,
        parent: (kind == ScopeKind::Worker)
            .then(|| ScopeParent::new(principal("prefrontal-core"), "head-session", 7)),
        parent_state: None,
        attributes: if kind == ScopeKind::Worker {
            ScopeAttributes::new()
        } else {
            ScopeAttributes::new().with_agent_id(Some("agent-head".into()))
        },
        owner_authorized: true,
    };
    Stamp::from_bind(
        &RouteBindRequest::new(
            RouteHandle::detached(1, 1),
            RouteTarget::ManagementSurface {
                module_id: "cingulate".into(),
            },
            BindIdentity::new("/tmp", "test-harness", "untrusted-session"),
        )
        .with_principal(principal(module))
        .with_scope(scope),
    )
}
fn phone() -> Stamp {
    Stamp::from_bind(
        &RouteBindRequest::new(
            RouteHandle::detached(2, 1),
            RouteTarget::ManagementSurface {
                module_id: "cingulate".into(),
            },
            BindIdentity::new("/tmp", "fed-phone:test", "phone"),
        )
        .with_principal(principal("callosum")),
    )
}
fn core() -> Stamp {
    Stamp::from_principal(Some(&principal("prefrontal-core")))
}
fn request(policy: Option<&str>) -> Value {
    let mut value = json!({"kind":"tool_permission","call_key":"call-1","tool":"bash","operation":"exec","paths":["/tmp/work"],"command":"pwd","reason":"Inspect the folder","args_hash":"hash-1","schema_pin":"schema-1","pattern":"/tmp/work/**"});
    if let Some(policy) = policy {
        value["head_policy"] = json!(policy);
    }
    value
}
struct Fixture {
    _dir: tempfile::TempDir,
    path: std::path::PathBuf,
    handler: Handler,
    runtime: Arc<Runtime>,
    clock: Arc<TestClock>,
    hook: Arc<WaitHook>,
    phone_notices: Arc<PhoneNotices>,
}
impl Fixture {
    async fn new(macos: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("store.sqlite");
        let runtime = Arc::new(Runtime {
            life: Mutex::new(ScopeLife::Live(head())),
            replies: Mutex::new(vec![]),
            notices: Mutex::new(vec![]),
            macos,
        });
        let clock = Arc::new(TestClock(watch::channel(1000).0));
        let hook = Arc::new(WaitHook(watch::channel(0).0));
        let phone_notices = Arc::new(PhoneNotices::default());
        let handler = Handler::new(Options {
            clock: clock.clone(),
            await_hook: Some(hook.clone()),
            notifier: phone_notices.clone(),
            sweep_timer: false,
            data_home: dir.path().to_str().unwrap().into(),
            ..Default::default()
        });
        handler.set_access_runtime(runtime.clone());
        let descriptor = StorageDescriptor {
            module_id: "cingulate".into(),
            storage_namespace: "main".into(),
            isolation: Isolation::Module,
            backend: StorageBackend::Sqlite {
                path: path.to_str().unwrap().into(),
            },
        };
        let ack: ModuleHelloAckBody = serde_json::from_value(
            json!({"negotiated_ver":1,"subc_ops":[],"subc_capabilities":[],"storage":descriptor}),
        )
        .unwrap();
        handler.hello_ack(&ack).await;
        Self {
            _dir: dir,
            path,
            handler,
            runtime,
            clock,
            hook,
            phone_notices,
        }
    }
    async fn ask(&self, stamp: &Stamp, params: Value) -> String {
        self.ok(stamp, "consent.request", params).await["elicitation_id"]
            .as_str()
            .unwrap()
            .into()
    }
    async fn call(&self, stamp: &Stamp, method: &str, params: Value) -> Result<Value, String> {
        match self
            .handler
            .handle_stamped(
                stamp,
                &serde_json::to_vec(&json!({"method":method,"params":params})).unwrap(),
            )
            .await
        {
            HandlerOutcome::Response(raw) => {
                Ok(serde_json::from_slice::<Value>(&raw).unwrap()["result"].clone())
            }
            HandlerOutcome::Error { code, .. } => Err(code),
            _ => panic!("unary response required"),
        }
    }
    async fn ok(&self, stamp: &Stamp, method: &str, params: Value) -> Value {
        self.call(stamp, method, params).await.unwrap()
    }
    async fn get(&self, id: &str) -> Value {
        self.ok(
            &stamp("aft", ScopeKind::Worker),
            "consent.get",
            json!({"elicitation_id":id}),
        )
        .await
    }
    async fn relay(&self, id: &str, answer: &str) -> Value {
        self.ok(
            &core(),
            "consent.answer",
            json!({"elicitation_id":id,"answer":answer,"answered_by":head()}),
        )
        .await
    }
    async fn deliver(&self) {
        // Give a previously spawned notice task a chance to run before
        // the test asks the same serialized delivery path to flush its outbox.
        tokio::task::yield_now().await;
        self.handler.deliver_access_notices().await.unwrap();
        tokio::task::yield_now().await;
        self.handler.deliver_access_notices().await.unwrap();
    }
    fn audit(&self, id: &str) -> Value {
        let db = rusqlite::Connection::open(&self.path).unwrap();
        let raw: String = db
            .query_row(
                "SELECT record_json FROM access_asks WHERE elicitation_id=?1",
                [id],
                |r| r.get(0),
            )
            .unwrap();
        serde_json::from_str(&raw).unwrap()
    }
    fn set_scope_owner(&self, id: &str, owner: Option<&str>) {
        let mut record = self.audit(id);
        if let Some(owner) = owner {
            record["identity"]["scope_owner"] = json!(owner);
        } else {
            record["identity"]
                .as_object_mut()
                .unwrap()
                .remove("scope_owner");
        }
        rusqlite::Connection::open(&self.path)
            .unwrap()
            .execute(
                "UPDATE access_asks SET record_json=?2 WHERE elicitation_id=?1",
                rusqlite::params![id, serde_json::to_string(&record).unwrap()],
            )
            .unwrap();
    }
}

#[tokio::test]
async fn only_aft_can_raise_permission_kinds() {
    let f = Fixture::new(true).await;
    for module in [
        "prefrontal-core",
        "basal",
        "callosum",
        "plexus",
        "unrecognized",
    ] {
        assert_eq!(
            f.call(
                &stamp(module, ScopeKind::Head),
                "consent.request",
                request(None)
            )
            .await
            .unwrap_err(),
            "elicitation_requester_refused"
        );
    }
    assert_eq!(
        f.call(
            &stamp("aft", ScopeKind::Head),
            "consent.request",
            json!({"kind":"consent"})
        )
        .await
        .unwrap_err(),
        "elicitation_invalid_request"
    );
    let id = f.ask(&stamp("aft", ScopeKind::Head), request(None)).await;
    assert_eq!(f.get(&id).await["state"], "pending:user");
}
#[tokio::test]
async fn routes_by_stamped_kind_and_parent_not_agent_id() {
    let f = Fixture::new(true).await;
    for (kind, policy, state) in [
        (ScopeKind::Worker, Some("allow"), "pending:head"),
        (ScopeKind::Head, None, "pending:user"),
        (ScopeKind::Ephemeral, None, "pending:user"),
    ] {
        let mut req = request(policy);
        req["call_key"] = json!(format!("{kind:?}"));
        let id = f.ask(&stamp("aft", kind), req).await;
        assert_eq!(f.get(&id).await["state"], state);
        assert_eq!(
            f.audit(&id)["identity"]["scope_kind"],
            format!("{kind:?}").to_lowercase()
        );
    }
}
#[tokio::test]
async fn orphaned_worker_and_foreign_scope_are_refused() {
    let f = Fixture::new(true).await;
    let mut worker = stamp("aft", ScopeKind::Worker);
    worker.scope.as_mut().unwrap().parent = None;
    assert_eq!(
        f.call(&worker, "consent.request", request(Some("allow")))
            .await
            .unwrap_err(),
        "access_no_head"
    );
    worker.scope.as_mut().unwrap().owner = principal("other-owner");
    assert_eq!(
        f.call(&worker, "consent.request", request(Some("allow")))
            .await
            .unwrap_err(),
        "access_not_alfonso"
    );
    let unscoped = Stamp::from_principal(Some(&principal("aft")));
    assert_eq!(
        f.call(&unscoped, "consent.request", request(None))
            .await
            .unwrap_err(),
        "access_not_alfonso"
    );
    *f.runtime.life.lock().unwrap() = ScopeLife::Gone;
    assert_eq!(
        f.call(
            &stamp("aft", ScopeKind::Worker),
            "consent.request",
            request(Some("allow"))
        )
        .await
        .unwrap_err(),
        "access_no_head"
    );
}
#[tokio::test]
async fn body_parent_cannot_replace_stamped_parent() {
    let f = Fixture::new(true).await;
    let mut req = request(Some("allow"));
    req["parent"] = json!({"scope_ref":"attacker","scope_epoch":9});
    assert_eq!(
        f.call(&stamp("aft", ScopeKind::Worker), "consent.request", req)
            .await
            .unwrap_err(),
        "elicitation_invalid_request"
    );
    let id = f
        .ask(&stamp("aft", ScopeKind::Worker), request(Some("allow")))
        .await;
    assert_eq!(f.audit(&id)["identity"]["head"], json!(head()));
}
#[tokio::test]
async fn request_fields_and_limits_are_closed() {
    let f = Fixture::new(true).await;
    for (key, value) in [
        ("tool", json!("x".repeat(65))),
        ("paths", json!(vec!["/p"; 17])),
        ("paths", json!(["relative"])),
        ("command", json!("x".repeat(601))),
        ("reason", json!("x".repeat(161))),
        ("reason", json!("two\nlines")),
        ("operation", json!("grant")),
        ("args_hash", json!("")),
        ("head_policy", json!("maybe")),
        ("principal", json!("reserved:aft")),
        ("expires_at_ms", json!(9999)),
    ] {
        let mut req = request(Some("allow"));
        req[key] = value;
        assert_eq!(
            f.call(&stamp("aft", ScopeKind::Worker), "consent.request", req)
                .await
                .unwrap_err(),
            "elicitation_invalid_request",
            "{key}"
        );
    }
    let mut req = request(None);
    req["operation"] = json!("read");
    assert!(f
        .call(&stamp("aft", ScopeKind::Head), "consent.request", req)
        .await
        .is_err());
    let mut req = request(Some("not_held"));
    req["paths"] = json!(vec!["/p"; 16]);
    req["reason"] = json!("é".repeat(160));
    req["command"] = json!("é".repeat(600));
    f.ask(&stamp("aft", ScopeKind::Worker), req).await;
}
#[tokio::test]
async fn head_allow_requires_held_permission() {
    let f = Fixture::new(true).await;
    let id = f
        .ask(&stamp("aft", ScopeKind::Worker), request(Some("not_held")))
        .await;
    for answer in ["allow_once", "allow_scope"] {
        let reply = f.relay(&id, answer).await;
        assert_eq!(reply["code"], "access_head_not_held");
        assert_eq!(f.get(&id).await["state"], "pending:head");
    }
    assert_eq!(
        f.get(&id).await["options"],
        json!([{"id":"deny","label":"deny","effect":"decline"},{"id":"escalate","label":"escalate","effect":"escalate"}])
    );
    assert_eq!(f.relay(&id, "deny").await["ok"], true);
    assert_eq!(f.get(&id).await["code"], "access_declined");
}
#[tokio::test]
async fn head_allow_and_scope_grant_are_for_the_asking_worker() {
    let f = Fixture::new(true).await;
    for answer in ["allow_once", "allow_scope"] {
        let mut req = request(Some("allow"));
        req["call_key"] = json!(answer);
        let id = f.ask(&stamp("aft", ScopeKind::Worker), req).await;
        assert_eq!(f.relay(&id, answer).await["ok"], true);
        let record = f.get(&id).await;
        assert_eq!(record["outcome"], answer);
        assert_eq!(record["asker"]["scope_ref"], "Worker-session");
    }
}
#[tokio::test]
async fn phone_cannot_answer_or_fetch_head_leg() {
    let f = Fixture::new(true).await;
    let id = f
        .ask(&stamp("aft", ScopeKind::Worker), request(Some("allow")))
        .await;
    let reply = f
        .ok(
            &phone(),
            "consent.answer",
            json!({"elicitationId":id,"choiceId":"allow_once"}),
        )
        .await;
    assert_eq!(reply["code"], "access_wrong_leg");
    assert_eq!(f.get(&id).await["state"], "pending:head");
    assert_eq!(
        f.call(&phone(), "consent.get", json!({"elicitationId":id}))
            .await
            .unwrap_err(),
        "elicitation_not_found"
    );
    assert_eq!(
        f.ok(&phone(), "consent.list_pending", json!({})).await["records"],
        json!([])
    );
}
#[tokio::test]
async fn wrong_head_or_epoch_or_agent_leaves_ask_pending() {
    let f = Fixture::new(true).await;
    let id = f
        .ask(&stamp("aft", ScopeKind::Worker), request(Some("allow")))
        .await;
    for (key, value) in [
        ("scope_ref", json!("other-head")),
        ("scope_epoch", json!(8)),
        ("agent_id", json!("other-agent")),
    ] {
        let mut by = json!(head());
        by[key] = value;
        let reply = f
            .ok(
                &core(),
                "consent.answer",
                json!({"elicitation_id":id,"answer":"deny","answered_by":by}),
            )
            .await;
        assert_eq!(reply["code"], "access_wrong_head");
    }
    assert_eq!(f.get(&id).await["state"], "pending:head");
    assert_eq!(f.audit(&id)["attempts"].as_array().unwrap().len(), 3);
}
#[tokio::test]
async fn escalation_carries_note_and_phone_rule_is_unchanged() {
    let f = Fixture::new(true).await;
    let id = f
        .ask(&stamp("aft", ScopeKind::Worker), request(Some("not_held")))
        .await;
    assert_eq!(f.ok(&core(),"consent.answer",json!({"elicitation_id":id,"answer":"escalate","note":"Please decide","answered_by":head()})).await["ok"],true);
    let records = f.ok(&phone(), "consent.list_pending", json!({})).await;
    assert_eq!(records["records"][0]["escalationNote"], "Please decide");
    assert!(records["records"][0].get("expiresAtMs").is_none());
    assert_eq!(f.relay(&id, "allow_once").await["code"], "access_wrong_leg");
    let mut bad_phone = phone();
    bad_phone.operator_surface = false;
    assert_eq!(
        f.call(
            &bad_phone,
            "consent.answer",
            json!({"elicitationId":id,"choiceId":"allow_scope"})
        )
        .await
        .unwrap_err(),
        "elicitation_operator_required"
    );
    assert_eq!(
        f.ok(
            &phone(),
            "consent.answer",
            json!({"elicitationId":id,"choiceId":"allow_scope"})
        )
        .await["ok"],
        true
    );
    assert_eq!(f.get(&id).await["asker"]["scope_ref"], "Worker-session");
}
#[tokio::test]
async fn escalated_phone_answers_keep_choices_and_remain_readable() {
    let options = json!([
        {"id":"allow_once","label":"Allow once","effect":"grant"},
        {"id":"allow_scope","label":"Allow for this task","effect":"grant"},
        {"id":"deny","label":"Don't allow","effect":"decline"}
    ]);
    for choice in ["allow_once", "allow_scope", "deny"] {
        let f = Fixture::new(true).await;
        let id = f
            .ask(&stamp("aft", ScopeKind::Worker), request(Some("allow")))
            .await;
        assert_eq!(f.relay(&id, "escalate").await["ok"], true);
        let pending = f
            .ok(&phone(), "consent.get", json!({"elicitationId":id}))
            .await;
        assert_eq!(pending["state"], "pending");
        assert_eq!(pending["options"], options);
        let answer = f
            .ok(
                &phone(),
                "consent.answer",
                json!({"elicitationId":id,"choiceId":choice}),
            )
            .await;
        assert_eq!(answer["ok"], true);
        let card = &answer["elicitation"];
        assert_eq!(card["state"], "answered");
        assert_eq!(card["answeredChoiceId"], choice);
        assert_eq!(card["answeredAtMs"], 1000);
        assert_eq!(card["options"], options);
        assert_eq!(card["subject"], pending["subject"]);
        assert_eq!(card["requesterName"], pending["requesterName"]);
        assert_eq!(
            f.ok(&phone(), "consent.get", json!({"elicitationId":id}))
                .await,
            *card
        );
        assert_eq!(
            f.ok(&phone(), "consent.list_pending", json!({})).await["records"],
            json!([])
        );
        let audit = f.audit(&id);
        let loser = f
            .ok(
                &phone(),
                "consent.answer",
                json!({"elicitationId":id,"choiceId":"allow_once"}),
            )
            .await;
        assert_eq!(loser["ok"], false);
        assert_eq!(loser["code"], "conflict");
        assert_eq!(loser["elicitation"], *card);
        assert_eq!(f.audit(&id)["state"], "settled");
        assert_eq!(f.audit(&id)["outcome"], audit["outcome"]);
        let requester = f.get(&id).await;
        assert_eq!(requester["state"], "settled");
        assert_eq!(requester["options"], json!([]));
        assert!(requester.get("answered_choice_id").is_none());
    }
}

#[tokio::test]
async fn escalated_phone_non_user_endings_are_readable_without_a_choice() {
    for ending in ["worker-withdraw", "core-withdraw", "head-deny"] {
        let f = Fixture::new(true).await;
        let worker = stamp("aft", ScopeKind::Worker);
        let id = f.ask(&worker, request(Some("allow"))).await;
        assert_eq!(f.relay(&id, "escalate").await["ok"], true);
        let pending = f
            .ok(&phone(), "consent.get", json!({"elicitationId":id}))
            .await;
        if ending == "head-deny" {
            assert_eq!(f.relay(&id, "deny").await["ok"], true);
        } else {
            let owner = if ending == "core-withdraw" {
                core()
            } else {
                worker
            };
            f.ok(&owner, "consent.withdraw", json!({"elicitation_id":id}))
                .await;
        }
        let audit = f.audit(&id);
        let card = f
            .ok(&phone(), "consent.get", json!({"elicitationId":id}))
            .await;
        assert_eq!(card["state"], "withdrawn", "{ending}");
        assert_eq!(card["options"], pending["options"]);
        assert_eq!(card["options"].as_array().unwrap().len(), 3);
        assert!(card.get("answeredChoiceId").is_none());
        assert_eq!(card["subject"], pending["subject"]);
        assert_eq!(card["requesterName"], pending["requesterName"]);
        assert_eq!(f.audit(&id), audit, "phone get must be read-only");
        assert_eq!(
            f.ok(&phone(), "consent.list_pending", json!({})).await["records"],
            json!([])
        );
    }
}

#[tokio::test]
async fn informational_phone_card_stays_readable_after_withdrawal() {
    let f = Fixture::new(true).await;
    let worker = stamp("aft", ScopeKind::Worker);
    let id = f
        .ask(&worker, json!({"kind":"macos_permission","folder":"/protected","program":"Terminal","steps":"Open System Settings"}))
        .await;
    f.ok(&worker, "consent.withdraw", json!({"elicitation_id":id}))
        .await;
    let card = f
        .ok(&phone(), "consent.get", json!({"elicitationId":id}))
        .await;
    assert_eq!(card["state"], "withdrawn");
    assert_eq!(card["options"], json!([]));
    assert!(card.get("answeredChoiceId").is_none());
    assert!(card.get("expiresAtMs").is_none());
}

#[tokio::test]
async fn escalated_head_deny_settles_wakes_worker_and_removes_phone_card() {
    for policy in ["allow", "not_held"] {
        let f = Fixture::new(true).await;
        let asker = stamp("aft", ScopeKind::Worker);
        let id = f.ask(&asker, request(Some(policy))).await;
        assert_eq!(f.relay(&id, "escalate").await["ok"], true);
        assert_eq!(
            f.ok(&phone(), "consent.list_pending", json!({})).await["records"][0]["elicitationId"],
            id
        );
        let mut registered = f.hook.0.subscribe();
        registered.borrow_and_update();
        let h = f.handler.clone();
        let wait_id = id.clone();
        let waiting = tokio::spawn(async move {
            h.handle_stamped(
                &asker,
                &serde_json::to_vec(&json!({
                    "method":"consent.await",
                    "params":{"elicitation_id":wait_id,"timeout_ms":5000}
                }))
                .unwrap(),
            )
            .await
        });
        registered.changed().await.unwrap();
        let note = (policy == "allow").then_some("Solved another way");
        let reply = f
            .ok(
                &core(),
                "consent.answer",
                json!({
                    "elicitation_id":id,"answer":"deny","note":note,"answered_by":head()
                }),
            )
            .await;
        assert_eq!(reply["ok"], true);
        let record = &reply["elicitation"];
        assert_eq!(record["state"], "settled");
        assert_eq!(record["outcome"], "deny");
        assert_eq!(record["code"], "access_declined");
        assert_eq!(record["answered_by"], "head");
        assert_eq!(record["note"], json!(note));
        let HandlerOutcome::Response(bytes) =
            tokio::time::timeout(std::time::Duration::from_secs(2), waiting)
                .await
                .unwrap()
                .unwrap()
        else {
            panic!("await response required")
        };
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap()["result"],
            *record
        );
        assert_eq!(f.get(&id).await, *record);
        assert_eq!(
            f.ok(&phone(), "consent.list_pending", json!({})).await["records"],
            json!([])
        );
        assert_eq!(
            f.ok(&phone(), "consent.get", json!({"elicitationId":id}))
                .await["state"],
            "withdrawn"
        );
        assert_eq!(
            f.phone_notices.0.lock().unwrap().last().unwrap()["kind"],
            "retraction"
        );
        f.deliver().await;
        let notices = f.runtime.notices.lock().unwrap();
        let settled: Vec<_> = notices
            .iter()
            .filter(|notice| notice["purpose"] == "settled")
            .collect();
        assert_eq!(settled.len(), 1);
        assert_eq!(
            settled[0]["asker"],
            json!({"scope_ref":"Worker-session","scope_epoch":3})
        );
        assert_eq!(settled[0]["fields"]["outcome"], "deny");
        let audit = f.audit(&id);
        assert_eq!(audit["attempts"].as_array().unwrap().len(), 2);
        for (attempt, answer) in audit["attempts"]
            .as_array()
            .unwrap()
            .iter()
            .zip(["escalate", "deny"])
        {
            assert_eq!(attempt["surface"], "head");
            assert_eq!(attempt["principal"], "reserved:prefrontal-core");
            assert_eq!(attempt["answered_by"], json!(head()));
            assert_eq!(attempt["answer"], answer);
            assert_eq!(attempt["result"], "accepted");
        }
    }
}

#[tokio::test]
async fn escalated_head_cannot_allow_or_escalate_again() {
    let f = Fixture::new(true).await;
    let id = f
        .ask(&stamp("aft", ScopeKind::Worker), request(Some("allow")))
        .await;
    assert_eq!(f.relay(&id, "escalate").await["ok"], true);
    for answer in ["allow_once", "allow_scope", "escalate"] {
        let reply = f.relay(&id, answer).await;
        assert_eq!(reply["ok"], false, "head answer {answer}");
        assert_eq!(reply["code"], "access_wrong_leg");
        assert_eq!(reply["elicitation"]["state"], "pending:user");
    }
    assert_eq!(f.audit(&id)["attempts"].as_array().unwrap().len(), 4);
}

#[tokio::test]
async fn escalated_head_deny_requires_exact_recorded_escalator_and_valid_note() {
    let f = Fixture::new(true).await;
    let id = f
        .ask(&stamp("aft", ScopeKind::Worker), request(Some("allow")))
        .await;
    assert_eq!(f.relay(&id, "escalate").await["ok"], true);
    for (key, value) in [
        ("scope_ref", json!("other-head")),
        ("scope_epoch", json!(8)),
        ("agent_id", json!("other-agent")),
    ] {
        let mut by = json!(head());
        by[key] = value;
        let reply = f
            .ok(
                &core(),
                "consent.answer",
                json!({
                    "elicitation_id":id,"answer":"deny","answered_by":by
                }),
            )
            .await;
        assert_eq!(reply["code"], "access_wrong_head");
        assert_eq!(reply["elicitation"]["state"], "pending:user");
    }
    for note in ["x".repeat(161), "two\nlines".into()] {
        assert_eq!(
            f.call(
                &core(),
                "consent.answer",
                json!({
                    "elicitation_id":id,"answer":"deny","answered_by":head(),"note":note
                })
            )
            .await
            .unwrap_err(),
            "elicitation_invalid_request"
        );
    }
    assert_eq!(f.get(&id).await["state"], "pending:user");
    let note = "x".repeat(160);
    let reply = f
        .ok(
            &core(),
            "consent.answer",
            json!({
                "elicitation_id":id,"answer":"deny","answered_by":head(),"note":note
            }),
        )
        .await;
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["elicitation"]["note"], note);
}

#[tokio::test]
async fn user_leg_without_recorded_escalator_refuses_head_deny() {
    let f = Fixture::new(true).await;
    let own = f.ask(&stamp("aft", ScopeKind::Head), request(None)).await;
    assert_eq!(f.relay(&own, "deny").await["code"], "access_wrong_leg");
    let mut req = request(Some("allow"));
    req["call_key"] = json!("legacy-escalation");
    let id = f.ask(&stamp("aft", ScopeKind::Worker), req).await;
    assert_eq!(f.relay(&id, "escalate").await["ok"], true);
    let mut legacy = f.audit(&id);
    for attempts in [
        json!([]),
        json!([{
            "surface":"head","principal":"reserved:prefrontal-core","at_ms":1000,
            "answer":"escalate","note":null,"answered_by":null,"result":"accepted"
        }]),
        json!([{
            "surface":"head","principal":"reserved:prefrontal-core","at_ms":1000,
            "answer":"escalate","note":null,"answered_by":head(),"result":"access_wrong_head"
        }]),
    ] {
        legacy["attempts"] = attempts;
        rusqlite::Connection::open(&f.path)
            .unwrap()
            .execute(
                "UPDATE access_asks SET record_json=?2 WHERE elicitation_id=?1",
                rusqlite::params![id, serde_json::to_string(&legacy).unwrap()],
            )
            .unwrap();
        assert_eq!(f.relay(&id, "deny").await["code"], "access_wrong_leg");
        assert_eq!(f.get(&id).await["state"], "pending:user");
    }
}

#[tokio::test]
async fn escalated_user_and_head_settlement_first_wins_in_both_orders() {
    for head_first in [true, false] {
        let f = Fixture::new(true).await;
        let id = f
            .ask(&stamp("aft", ScopeKind::Worker), request(Some("allow")))
            .await;
        assert_eq!(f.relay(&id, "escalate").await["ok"], true);
        let operator = phone();
        let user_answer = || {
            f.ok(
                &operator,
                "consent.answer",
                json!({
                    "elicitationId":id,"choiceId":"allow_once","note":"User decision"
                }),
            )
        };
        let (winner, outcome, by, note) = if head_first {
            (f.relay(&id, "deny").await, "deny", "head", Value::Null)
        } else {
            (
                user_answer().await,
                "allow_once",
                "user",
                json!("User decision"),
            )
        };
        let committed = f.get(&id).await;
        let loser = if head_first {
            user_answer().await
        } else {
            f.relay(&id, "deny").await
        };
        assert_eq!(winner["ok"], true);
        assert_eq!(loser["ok"], false);
        assert_eq!(loser["code"], "conflict");
        assert_eq!(
            loser["elicitation"]["outcome"],
            winner["elicitation"]["outcome"]
        );
        let record = f.get(&id).await;
        assert_eq!(record, committed);
        assert_eq!(record["outcome"], outcome);
        assert_eq!(record["answered_by"], by);
        assert_eq!(record["note"], note);
        let audit = f.audit(&id);
        assert_eq!(audit["attempts"].as_array().unwrap().len(), 3);
        assert_eq!(audit["attempts"][1]["result"], "accepted");
        assert_eq!(audit["attempts"][2]["result"], "conflict");
        let db = rusqlite::Connection::open(&f.path).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM access_notices WHERE elicitation_id=?1 AND purpose='settled'",
                [id],
                |r| r.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
    }
}

#[tokio::test]
async fn concurrent_escalated_user_answer_and_head_deny_settle_once() {
    let f = Fixture::new(true).await;
    let id = f
        .ask(&stamp("aft", ScopeKind::Worker), request(Some("allow")))
        .await;
    assert_eq!(f.relay(&id, "escalate").await["ok"], true);
    let operator = phone();
    let (head, user) = tokio::join!(
        f.relay(&id, "deny"),
        f.ok(
            &operator,
            "consent.answer",
            json!({"elicitationId":id,"choiceId":"allow_once"})
        )
    );
    assert_ne!(head["ok"], user["ok"]);
    let (winner, loser) = if head["ok"] == true {
        (&head, &user)
    } else {
        (&user, &head)
    };
    assert_eq!(loser["code"], "conflict");
    assert_eq!(
        loser["elicitation"]["outcome"],
        winner["elicitation"]["outcome"]
    );
    assert_eq!(
        f.get(&id).await["outcome"],
        winner["elicitation"]["outcome"]
    );
    let audit = f.audit(&id);
    assert_eq!(audit["attempts"].as_array().unwrap().len(), 3);
    assert_eq!(audit["attempts"][1]["result"], "accepted");
    assert_eq!(audit["attempts"][2]["result"], "conflict");
    let db = rusqlite::Connection::open(&f.path).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM access_notices WHERE elicitation_id=?1 AND purpose='settled'",
            [id],
            |r| r.get::<_, i64>(0),
        )
        .unwrap(),
        1
    );
}

#[tokio::test]
async fn first_answer_wins_and_losing_attempts_are_audited() {
    let f = Fixture::new(true).await;
    let id = f.ask(&stamp("aft", ScopeKind::Head), request(None)).await;
    assert_eq!(
        f.ok(
            &phone(),
            "consent.answer",
            json!({"elicitationId":id,"choiceId":"deny","note":"No"})
        )
        .await["ok"],
        true
    );
    for choice in ["allow_once", "deny"] {
        let reply = f
            .ok(
                &phone(),
                "consent.answer",
                json!({"elicitationId":id,"choiceId":choice}),
            )
            .await;
        assert_eq!(reply["code"], "conflict");
        assert_eq!(reply["elicitation"]["outcome"], "deny");
    }
    let record = f.audit(&id);
    assert_eq!(record["attempts"].as_array().unwrap().len(), 3);
    assert_eq!(record["note"], "No");
    assert_eq!(record["settled_at_ms"], 1000);
    assert_eq!(record["surfaces"][0]["surface"], "user");
}
#[tokio::test]
async fn raiser_can_withdraw_from_another_session() {
    let f = Fixture::new(true).await;
    let id = f
        .ask(&stamp("aft", ScopeKind::Worker), request(Some("allow")))
        .await;
    assert_eq!(
        f.call(
            &stamp("basal", ScopeKind::Head),
            "consent.withdraw",
            json!({"elicitation_id":id})
        )
        .await
        .unwrap_err(),
        "elicitation_not_owner"
    );
    assert_eq!(f.get(&id).await["state"], "pending:head");
    let another_aft_session = stamp("aft", ScopeKind::Head);
    assert_eq!(
        f.ok(
            &another_aft_session,
            "consent.withdraw",
            json!({"elicitationId":id})
        )
        .await["outcome"],
        "access_withdrawn"
    );
    assert_eq!(f.audit(&id)["withdrawn_by"], "reserved:aft");
    assert_eq!(f.relay(&id, "allow_once").await["code"], "conflict");
    let settled = f.audit(&id);
    assert_eq!(
        f.ok(
            &another_aft_session,
            "consent.withdraw",
            json!({"elicitation_id":id})
        )
        .await["outcome"],
        "access_withdrawn"
    );
    assert_eq!(f.audit(&id), settled);
}
#[tokio::test]
async fn core_withdraw_records_principal_and_notice() {
    for (kind, policy) in [(ScopeKind::Worker, Some("allow")), (ScopeKind::Head, None)] {
        let f = Fixture::new(true).await;
        let id = f.ask(&stamp("aft", kind), request(policy)).await;
        let pending = f.audit(&id);
        assert_eq!(
            pending["identity"]["scope_owner"],
            "reserved:prefrontal-core"
        );
        for method in ["consent.get", "consent.await", "consent.detach"] {
            assert_eq!(
                f.call(&core(), method, json!({"elicitation_id":id,"timeout_ms":0}))
                    .await
                    .unwrap_err(),
                "elicitation_not_owner"
            );
        }
        let reply = f
            .ok(&core(), "consent.withdraw", json!({"elicitation_id":id}))
            .await;
        assert_eq!(reply["state"], "refused:access_withdrawn");
        assert_eq!(reply["outcome"], "access_withdrawn");
        let settled = f.audit(&id);
        assert_eq!(settled["withdrawn_by"], "reserved:prefrontal-core");
        assert_eq!(settled["answered_by"], Value::Null);
        assert_eq!(settled["identity"], pending["identity"]);
        assert_eq!(settled["settled_at_ms"], 1000);
        f.deliver().await;
        let db = rusqlite::Connection::open(&f.path).unwrap();
        let body: String = db
            .query_row(
                "SELECT body_json FROM access_notices WHERE elicitation_id=?1 AND purpose='withdrawn'",
                [&id],
                |r| r.get(0),
            )
            .unwrap();
        let body: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(body["purpose"], "withdrawn");
        assert_eq!(body["elicitation_id"], id);
        assert_eq!(body["fields"], json!({}));
        assert_eq!(
            body["asker"],
            json!({
                "scope_ref":pending["identity"]["asker"]["scope_ref"],
                "scope_epoch":pending["identity"]["asker"]["scope_epoch"]
            })
        );
        assert_eq!(
            f.ok(&core(), "consent.withdraw", json!({"elicitation_id":id}))
                .await,
            reply
        );
        assert_eq!(f.audit(&id), settled);
        let count: i64 = db.query_row(
            "SELECT count(*) FROM access_notices WHERE elicitation_id=?1 AND purpose='withdrawn'",
            [&id], |r| r.get(0)
        ).unwrap();
        assert_eq!(count, 1);
    }
}
#[tokio::test]
async fn core_withdraw_respects_recorded_owner() {
    let f = Fixture::new(true).await;
    let id = f
        .ask(&stamp("aft", ScopeKind::Worker), request(Some("allow")))
        .await;
    // Creation admits only Core-owned scopes; model a stored foreign-owned ask.
    f.set_scope_owner(&id, Some("reserved:basal"));
    let before = f.audit(&id);
    assert_eq!(
        f.call(
            &core(),
            "consent.withdraw",
            json!({
                "elicitation_id":id,
                "scope_owner":"reserved:prefrontal-core",
                "asker":{"owner":"reserved:prefrontal-core"}
            })
        )
        .await
        .unwrap_err(),
        "elicitation_not_owner"
    );
    assert_eq!(f.audit(&id), before);
    assert_eq!(f.get(&id).await["state"], "pending:head");
}
#[tokio::test]
async fn missing_stored_scope_owner_does_not_authorize_core() {
    let f = Fixture::new(true).await;
    let id = f
        .ask(&stamp("aft", ScopeKind::Worker), request(Some("allow")))
        .await;
    f.set_scope_owner(&id, None);
    let before = f.audit(&id);
    assert_eq!(
        f.call(&core(), "consent.withdraw", json!({"elicitation_id":id}))
            .await
            .unwrap_err(),
        "elicitation_not_owner"
    );
    assert_eq!(f.audit(&id), before);
    f.ok(
        &stamp("aft", ScopeKind::Head),
        "consent.withdraw",
        json!({"elicitation_id":id}),
    )
    .await;
    assert_eq!(f.audit(&id)["withdrawn_by"], "reserved:aft");
}
#[tokio::test]
async fn core_cannot_withdraw_macos_or_generic_consent() {
    let f = Fixture::new(true).await;
    let id = f
        .ask(
            &stamp("aft", ScopeKind::Head),
            json!({
                "kind":"macos_permission","folder":"/protected","program":"Terminal",
                "steps":"Open System Settings"
            }),
        )
        .await;
    let before = f.audit(&id);
    assert_eq!(
        f.call(&core(), "consent.withdraw", json!({"elicitation_id":id}))
            .await
            .unwrap_err(),
        "elicitation_not_owner"
    );
    assert_eq!(f.audit(&id), before);
    let id = f.ask(&stamp("cerebellum", ScopeKind::Head), json!({
            "kind":"consent","title":"Approve operation","prompt":"Proceed?",
            "options":[{"id":"allow","label":"Allow","effect":"grant"},{"id":"deny","label":"Deny","effect":"decline"}],
            "default":"deny","urgency":"normal","on_expiry":"deny",
            "material_damage":false,"late_execution":"notify_only",
            "target":{"kind":"operation","label":"operation"},
            "expires_in_ms":60000,"args_digest":"hash-generic"
    })).await;
    assert_eq!(
        f.call(&core(), "consent.withdraw", json!({"elicitation_id":id}))
            .await
            .unwrap_err(),
        "elicitation_not_owner"
    );
    assert_eq!(
        f.ok(
            &stamp("cerebellum", ScopeKind::Head),
            "consent.get",
            json!({"elicitation_id":id})
        )
        .await["state"],
        "pending"
    );
}
#[tokio::test]
async fn withdraw_of_answered_ask_returns_record_unchanged() {
    let f = Fixture::new(true).await;
    let id = f
        .ask(&stamp("aft", ScopeKind::Worker), request(Some("allow")))
        .await;
    f.relay(&id, "deny").await;
    let before = f.audit(&id);
    let reply = f.get(&id).await;
    for caller in [core(), stamp("aft", ScopeKind::Head)] {
        assert_eq!(
            f.ok(&caller, "consent.withdraw", json!({"elicitation_id":id}))
                .await,
            reply
        );
        assert_eq!(f.audit(&id), before);
    }
    assert_eq!(before.get("withdrawn_by"), None);
}
#[tokio::test]
async fn idempotent_call_key_survives_settlement_and_restart() {
    let mut f = Fixture::new(true).await;
    let worker = stamp("aft", ScopeKind::Worker);
    let id = f.ask(&worker, request(Some("allow"))).await;
    f.relay(&id, "deny").await;
    *f.runtime.life.lock().unwrap() = ScopeLife::Gone;
    let mut req = request(Some("not_held"));
    req["args_hash"] = json!("changed");
    assert_eq!(f.ask(&worker, req).await, id);
    f.deliver().await;
    let path = f.path.clone();
    let reopened = Handler::new(Options {
        sweep_timer: false,
        data_home: f._dir.path().to_str().unwrap().into(),
        ..Default::default()
    });
    reopened.set_access_runtime(f.runtime.clone());
    let old = std::mem::replace(&mut f.handler, reopened);
    drop(old);
    let ack: ModuleHelloAckBody = serde_json::from_value(json!({"negotiated_ver":1,"subc_ops":[],"subc_capabilities":[],"storage":StorageDescriptor { module_id:"cingulate".into(),storage_namespace:"main".into(),isolation:Isolation::Module,backend:StorageBackend::Sqlite { path:path.to_str().unwrap().into() } }})).unwrap();
    f.handler.hello_ack(&ack).await;
    assert_eq!(f.ask(&worker, request(Some("allow"))).await, id);
    assert_eq!(f.get(&id).await["outcome"], "deny");
    assert_eq!(f.audit(&id)["attempts"].as_array().unwrap().len(), 1);
}
#[tokio::test]
async fn no_clock_advance_expires_either_leg() {
    let f = Fixture::new(true).await;
    let id = f
        .ask(&stamp("aft", ScopeKind::Worker), request(Some("allow")))
        .await;
    f.clock.0.send_replace(1_000_000_000);
    f.handler.timer_tick().await.unwrap();
    assert_eq!(f.get(&id).await["state"], "pending:head");
    f.relay(&id, "escalate").await;
    f.clock.0.send_replace(2_000_000_000);
    f.handler.timer_tick().await.unwrap();
    let record = f.get(&id).await;
    assert_eq!(record["state"], "pending:user");
    assert!(record.get("expires_at_ms").is_none());
}
#[tokio::test]
async fn head_scope_ending_refuses_but_unknown_liveness_waits() {
    let f = Fixture::new(true).await;
    let id = f
        .ask(&stamp("aft", ScopeKind::Worker), request(Some("allow")))
        .await;
    *f.runtime.life.lock().unwrap() = ScopeLife::Unknown;
    f.handler.timer_tick().await.unwrap();
    assert_eq!(f.get(&id).await["state"], "pending:head");
    *f.runtime.life.lock().unwrap() = ScopeLife::Gone;
    f.handler.timer_tick().await.unwrap();
    assert_eq!(f.get(&id).await["outcome"], "access_head_gone");
}
#[tokio::test]
async fn macos_cards_are_informational_and_deduplicate_open_folder_program() {
    let f = Fixture::new(true).await;
    let asker = stamp("aft", ScopeKind::Head);
    let req = json!({"kind":"macos_permission","folder":"/protected","program":"Terminal","steps":"Open System Settings"});
    let id = f.ask(&asker, req.clone()).await;
    assert_eq!(f.ask(&asker, req.clone()).await, id);
    let card = f
        .ok(&phone(), "consent.get", json!({"elicitationId":id}))
        .await;
    assert_eq!(card["options"], json!([]));
    assert!(card.get("default").is_none());
    assert_eq!(
        f.ok(
            &phone(),
            "consent.answer",
            json!({"elicitationId":id,"choiceId":"allow_once"})
        )
        .await["ok"],
        false
    );
    assert_eq!(
        f.call(
            &asker,
            "consent.await",
            json!({"elicitation_id":id,"timeout_ms":0})
        )
        .await
        .unwrap_err(),
        "access_informational"
    );
    f.ok(&asker, "consent.withdraw", json!({"elicitation_id":id}))
        .await;
    assert_ne!(f.ask(&asker, req.clone()).await, id);
    let other = Fixture::new(false).await;
    assert_eq!(
        other
            .call(&asker, "consent.request", req)
            .await
            .unwrap_err(),
        "access_macos_only"
    );
}

#[tokio::test]
async fn await_is_read_only_reawaitable_and_wakes_on_settlement() {
    let f = Fixture::new(true).await;
    let asker = stamp("aft", ScopeKind::Head);
    let id = f.ask(&asker, request(None)).await;
    let before = f.audit(&id);
    for _ in 0..2 {
        assert_eq!(
            f.ok(
                &asker,
                "consent.await",
                json!({"elicitationId":id,"timeout_ms":0})
            )
            .await["state"],
            "pending:user"
        );
    }
    assert_eq!(f.audit(&id), before);
    let mut registered = f.hook.0.subscribe();
    registered.borrow_and_update();
    let h = f.handler.clone();
    let wait_id = id.clone();
    let wait_asker = asker.clone();
    let waiting = tokio::spawn(async move {
        h.handle_stamped(&wait_asker,&serde_json::to_vec(&json!({"method":"consent.await","params":{"elicitation_id":wait_id,"timeout_ms":5000}})).unwrap()).await
    });
    registered.changed().await.unwrap();
    f.ok(
        &phone(),
        "consent.answer",
        json!({"elicitationId":id,"choiceId":"allow_once"}),
    )
    .await;
    let reply = tokio::time::timeout(std::time::Duration::from_secs(2), waiting)
        .await
        .unwrap()
        .unwrap();
    let HandlerOutcome::Response(bytes) = reply else {
        panic!("await response required")
    };
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap()["result"]["outcome"],
        "allow_once"
    );
    assert_eq!(
        f.ok(
            &asker,
            "consent.await",
            json!({"elicitation_id":id,"timeout_ms":5000})
        )
        .await["outcome"],
        "allow_once"
    );
}
#[tokio::test]
async fn await_bound_returns_pending_without_mutation() {
    let f = Fixture::new(true).await;
    let asker = stamp("aft", ScopeKind::Head);
    let id = f.ask(&asker, request(None)).await;
    let before = f.audit(&id);
    let mut registered = f.hook.0.subscribe();
    let h = f.handler.clone();
    let wait_id = id.clone();
    let waiting = tokio::spawn(async move {
        h.handle_stamped(&asker,&serde_json::to_vec(&json!({"method":"consent.await","params":{"elicitation_id":wait_id,"timeout_ms":10}})).unwrap()).await
    });
    registered.changed().await.unwrap();
    f.clock.0.send_replace(1010);
    let HandlerOutcome::Response(bytes) =
        tokio::time::timeout(std::time::Duration::from_secs(2), waiting)
            .await
            .unwrap()
            .unwrap()
    else {
        panic!("await response required")
    };
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap()["result"]["state"],
        "pending:user"
    );
    assert_eq!(f.audit(&id), before);
}
#[tokio::test]
async fn notice_wire_is_structured_and_deduplicated() {
    let f = Fixture::new(true).await;
    let id = f
        .ask(&stamp("aft", ScopeKind::Worker), request(Some("not_held")))
        .await;
    f.deliver().await;
    let notices = f.runtime.notices.lock().unwrap().clone();
    assert_eq!(notices.len(), 1);
    assert_eq!(
        notices[0],
        json!({"elicitation_id":id,"purpose":"head_ask","asker":{"scope_ref":"Worker-session","scope_epoch":3},"head":{"scope_ref":"head-session","scope_epoch":7},"fields":{"tool":"bash","operation":"exec","paths":["/tmp/work"],"command":"pwd","reason":"Inspect the folder","head_policy":"not_held","answers":["deny","escalate"]}})
    );
    f.deliver().await;
    assert_eq!(f.runtime.notices.lock().unwrap().len(), 1);
    f.relay(&id, "escalate").await;
    f.deliver().await;
    assert_eq!(f.runtime.notices.lock().unwrap()[1]["purpose"], "escalated");
    f.ok(
        &phone(),
        "consent.answer",
        json!({"elicitationId":id,"choiceId":"deny","note":"No"}),
    )
    .await;
    f.deliver().await;
    let notices = f.runtime.notices.lock().unwrap();
    assert_eq!(
        notices[2]["fields"],
        json!({"outcome":"deny","answered_by":"user","note":"No"})
    );
}
#[tokio::test]
async fn user_ask_and_withdrawn_notices_use_stamped_targets() {
    let f = Fixture::new(true).await;
    let id = f.ask(&stamp("aft", ScopeKind::Head), request(None)).await;
    f.deliver().await;
    assert_eq!(f.runtime.notices.lock().unwrap()[0]["purpose"], "user_ask");
    assert!(f.runtime.notices.lock().unwrap()[0].get("head").is_none());
    f.ok(
        &stamp("aft", ScopeKind::Head),
        "consent.withdraw",
        json!({"elicitation_id":id}),
    )
    .await;
    f.deliver().await;
    assert_eq!(f.runtime.notices.lock().unwrap()[1]["purpose"], "withdrawn");
    assert_eq!(f.runtime.notices.lock().unwrap()[1]["fields"], json!({}));
}
#[tokio::test]
async fn notice_missing_head_refuses_without_retry() {
    for refusal in ["unknown_session", "session_not_live"] {
        let f = Fixture::new(true).await;
        f.runtime
            .replies
            .lock()
            .unwrap()
            .push(NoticeResult::Refused(refusal.into()));
        let id = f
            .ask(&stamp("aft", ScopeKind::Worker), request(Some("allow")))
            .await;
        f.deliver().await;
        assert_eq!(f.get(&id).await["outcome"], "access_head_gone");
        f.deliver().await;
        let notices = f.runtime.notices.lock().unwrap();
        assert_eq!(
            notices
                .iter()
                .filter(|n| n["purpose"] == "head_ask")
                .count(),
            1
        );
        assert!(notices.iter().any(|n| n["purpose"] == "settled"));
    }
}
#[tokio::test]
async fn notice_missing_settled_session_records_unclaimed() {
    for refusal in ["unknown_session", "session_not_live"] {
        let f = Fixture::new(true).await;
        let id = f.ask(&stamp("aft", ScopeKind::Head), request(None)).await;
        f.deliver().await;
        f.runtime
            .replies
            .lock()
            .unwrap()
            .push(NoticeResult::Refused(refusal.into()));
        f.ok(
            &phone(),
            "consent.answer",
            json!({"elicitationId":id,"choiceId":"allow_once"}),
        )
        .await;
        f.deliver().await;
        assert_eq!(f.audit(&id)["unclaimed"], true);
        f.deliver().await;
        assert_eq!(f.runtime.notices.lock().unwrap().len(), 2);
        assert_eq!(f.get(&id).await["outcome"], "allow_once");
    }
}
#[tokio::test]
async fn notice_defects_do_not_retry_or_change_ask() {
    for refusal in ["unauthorized_caller", "invalid_params"] {
        let f = Fixture::new(true).await;
        f.runtime
            .replies
            .lock()
            .unwrap()
            .push(NoticeResult::Refused(refusal.into()));
        let id = f
            .ask(&stamp("aft", ScopeKind::Worker), request(Some("allow")))
            .await;
        f.deliver().await;
        f.clock.0.send_replace(100_000);
        f.deliver().await;
        assert_eq!(f.get(&id).await["state"], "pending:head");
        assert_eq!(f.runtime.notices.lock().unwrap().len(), 1);
        let db = rusqlite::Connection::open(&f.path).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT last_error FROM access_notices WHERE elicitation_id=?1",
                [&id],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            refusal
        );
    }
}
#[tokio::test]
async fn notice_transport_retries_with_backoff_without_blocking_state() {
    let f = Fixture::new(true).await;
    f.runtime
        .replies
        .lock()
        .unwrap()
        .push(NoticeResult::Transport("core unavailable".into()));
    let id = f
        .ask(&stamp("aft", ScopeKind::Worker), request(Some("allow")))
        .await;
    f.deliver().await;
    assert_eq!(f.get(&id).await["state"], "pending:head");
    f.clock.0.send_replace(1249);
    f.deliver().await;
    assert_eq!(f.runtime.notices.lock().unwrap().len(), 1);
    f.clock.0.send_replace(1250);
    f.deliver().await;
    assert_eq!(f.runtime.notices.lock().unwrap().len(), 2);
    f.deliver().await;
    assert_eq!(f.runtime.notices.lock().unwrap().len(), 2);
    assert_eq!(f.relay(&id, "allow_once").await["ok"], true);
}
#[tokio::test]
async fn notice_other_missing_sessions_do_not_retry_or_settle() {
    for purpose in ["user_ask", "escalated", "withdrawn"] {
        let f = Fixture::new(true).await;
        let worker = purpose == "escalated";
        if purpose == "user_ask" {
            f.runtime
                .replies
                .lock()
                .unwrap()
                .push(NoticeResult::Refused("unknown_session".into()));
        }
        let id = f
            .ask(
                &stamp(
                    "aft",
                    if worker {
                        ScopeKind::Worker
                    } else {
                        ScopeKind::Head
                    },
                ),
                request(if worker { Some("allow") } else { None }),
            )
            .await;
        f.deliver().await;
        if purpose != "user_ask" {
            f.runtime
                .replies
                .lock()
                .unwrap()
                .push(NoticeResult::Refused("session_not_live".into()));
            if worker {
                f.relay(&id, "escalate").await;
            } else {
                f.ok(
                    &stamp("aft", ScopeKind::Head),
                    "consent.withdraw",
                    json!({"elicitation_id":id}),
                )
                .await;
            }
            f.deliver().await;
        }
        let count = f.runtime.notices.lock().unwrap().len();
        f.clock.0.send_replace(100_000);
        f.deliver().await;
        assert_eq!(f.runtime.notices.lock().unwrap().len(), count);
        assert_eq!(f.audit(&id)["unclaimed"], false);
        assert_eq!(
            f.get(&id).await["state"],
            if purpose == "withdrawn" {
                "refused:access_withdrawn"
            } else {
                "pending:user"
            }
        );
    }
}

#[tokio::test]
async fn malformed_attempts_and_audit_survive_retention_sweep() {
    let f = Fixture::new(true).await;
    let id = f
        .ask(&stamp("aft", ScopeKind::Worker), request(Some("allow")))
        .await;
    for params in [
        json!({"elicitation_id":id,"answer":"deny","answered_by":{"scope_ref":"head-session","scope_epoch":7}}),
        json!({"elicitation_id":id,"answer":"deny","note":"two\nlines","answered_by":head()}),
    ] {
        assert_eq!(
            f.call(&core(), "consent.answer", params).await.unwrap_err(),
            "elicitation_invalid_request"
        );
    }
    f.relay(&id, "deny").await;
    f.clock.0.send_replace(1_000_000_000);
    f.handler.timer_tick().await.unwrap();
    let audit = f.audit(&id);
    assert_eq!(audit["attempts"].as_array().unwrap().len(), 3);
    assert_eq!(
        audit["attempts"][0]["result"],
        "elicitation_invalid_request"
    );
    assert!(audit["attempts"][0].get("malformed_params").is_some());
    assert_eq!(audit["identity"]["harness"], "test-harness");
    assert_eq!(audit["outcome"], "deny");
}
#[tokio::test]
async fn concurrent_answers_compare_and_set_one_settlement() {
    let f = Fixture::new(true).await;
    let id = f.ask(&stamp("aft", ScopeKind::Head), request(None)).await;
    let operator = phone();
    let (first, second) = tokio::join!(
        f.ok(
            &operator,
            "consent.answer",
            json!({"elicitationId":id,"choiceId":"allow_once"})
        ),
        f.ok(
            &operator,
            "consent.answer",
            json!({"elicitationId":id,"choiceId":"deny"})
        )
    );
    assert_ne!(first["ok"], second["ok"]);
    assert_eq!(f.audit(&id)["attempts"].as_array().unwrap().len(), 2);
    let db = rusqlite::Connection::open(&f.path).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM access_notices WHERE elicitation_id=?1 AND purpose='settled'",
            [id],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
}

#[tokio::test]
async fn head_leg_stays_hidden_and_user_leg_retracts_without_hiding_record() {
    let f = Fixture::new(true).await;
    let id = f
        .ask(&stamp("aft", ScopeKind::Worker), request(Some("allow")))
        .await;
    assert!(f.phone_notices.0.lock().unwrap().is_empty());
    assert_eq!(
        f.call(&phone(), "consent.get", json!({"elicitationId":id}))
            .await
            .unwrap_err(),
        "elicitation_not_found"
    );
    f.relay(&id, "deny").await;
    assert!(f.phone_notices.0.lock().unwrap().is_empty());
    assert_eq!(
        f.call(&phone(), "consent.get", json!({"elicitationId":id}))
            .await
            .unwrap_err(),
        "elicitation_not_found"
    );
    let mut req = request(Some("allow"));
    req["call_key"] = json!("escalated-call");
    let id = f.ask(&stamp("aft", ScopeKind::Worker), req).await;
    f.relay(&id, "escalate").await;
    assert_eq!(f.phone_notices.0.lock().unwrap()[0]["kind"], "elicitation");
    f.ok(
        &phone(),
        "consent.answer",
        json!({"elicitationId":id,"choiceId":"deny"}),
    )
    .await;
    assert_eq!(f.phone_notices.0.lock().unwrap()[1]["kind"], "retraction");
    assert_eq!(
        f.ok(&phone(), "consent.get", json!({"elicitationId":id}))
            .await["state"],
        "answered"
    );
}

#[tokio::test]
async fn permission_phone_pages_and_request_bodies_obey_shared_byte_budgets() {
    let f = Fixture::new(true).await;
    let asker = stamp("aft", ScopeKind::Head);
    let mut oversized = request(None);
    oversized["pattern"] = json!("p".repeat(1_310_720));
    assert_eq!(
        f.call(&asker, "consent.request", oversized)
            .await
            .unwrap_err(),
        "elicitation_request_too_large"
    );
    for n in 0..4 {
        let mut req = request(None);
        req["call_key"] = json!(format!("large-{n}"));
        req["pattern"] = json!("p".repeat(1_100_000));
        f.ask(&asker, req).await;
    }
    let mut params = json!({});
    let mut seen = std::collections::HashSet::new();
    for page_number in 0..4 {
        let page = f.ok(&phone(), "consent.list_pending", params).await;
        // Plain-text facts repeat the pattern, so these larger phone cards fit
        // one per page without truncating the requested permission scope.
        assert_eq!(page["records"].as_array().unwrap().len(), 1);
        assert!(serde_json::to_vec(&page).unwrap().len() < 3 * 1024 * 1024);
        let card = &page["records"][0];
        assert!(card.get("expiresAtMs").is_none());
        assert!(seen.insert(card["elicitationId"].as_str().unwrap().to_owned()));
        assert_eq!(card["pattern"], "p".repeat(1_100_000));
        assert_eq!(
            card["facts"]
                .as_array()
                .unwrap()
                .iter()
                .find(|fact| fact["label"] == "Pattern")
                .unwrap()["value"],
            "p".repeat(1_100_000)
        );
        if page_number == 3 {
            assert!(page.get("continuation").is_none());
        } else {
            assert!(page["continuation"].is_object());
            assert_eq!(page["continuation"]["created_at_ms"], card["createdAtMs"]);
            assert!(page["continuation"].get("expires_at_ms").is_none());
        }
        params = json!({"continuation": page["continuation"]});
    }
    assert_eq!(seen.len(), 4);
}
