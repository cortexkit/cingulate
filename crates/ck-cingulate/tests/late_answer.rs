//! Late answers on generic consent cards, through the handler: `consent.detach`
//! replies, the `consent_answered` notice on the shared outbox, and the
//! ordering of detach against an answer while a requester's await is in flight.

use async_trait::async_trait;
use cingulate_store::{Isolation, ScopeIdentity, StorageBackend, StorageDescriptor};
use ck_cingulate::{AccessRuntime, AwaitHook, Clock, Handler, NoticeResult, Options, Stamp};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use subc_client_rs::{HandlerOutcome, RouteBindRequest, RouteHandle};
use subc_protocol::{
    scope::{ScopeAttributes, ScopeKind, ScopeStamp},
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

/// Records each notice exactly as the delivery loop hands it over.
#[derive(Default)]
struct Runtime {
    replies: Mutex<Vec<NoticeResult>>,
    sent: Mutex<Vec<String>>,
}
#[async_trait]
impl AccessRuntime for Runtime {
    async fn describe(&self, scope: &ScopeIdentity) -> Result<ck_cingulate::ScopeLife, String> {
        Ok(ck_cingulate::ScopeLife::Live(scope.clone()))
    }
    async fn notice(&self, body: Value) -> NoticeResult {
        self.notice_json(&body.to_string()).await
    }
    async fn notice_json(&self, body: &str) -> NoticeResult {
        self.sent.lock().unwrap().push(body.to_owned());
        let mut replies = self.replies.lock().unwrap();
        if replies.is_empty() {
            NoticeResult::Delivered
        } else {
            replies.remove(0)
        }
    }
    fn is_macos(&self) -> bool {
        true
    }
}

struct WaitHook(watch::Sender<usize>);
#[async_trait]
impl AwaitHook for WaitHook {
    async fn after_register(&self, _: &Handler, _: &str) {
        self.0.send_modify(|n| *n += 1);
    }
}

fn principal(id: &str) -> Principal {
    Principal::Reserved {
        module_id: id.into(),
    }
}

/// A route stamped with the agent session `sc_head_alf`, epoch 3.
fn stamped(module: &str, handle: u16) -> Stamp {
    Stamp::from_bind(
        &RouteBindRequest::new(
            RouteHandle::detached(handle, 1),
            RouteTarget::ManagementSurface {
                module_id: "cingulate".into(),
            },
            BindIdentity::new("/tmp", "test-harness", "untrusted-session"),
        )
        .with_principal(principal(module))
        .with_scope(
            ScopeStamp::new(
                principal("prefrontal-core"),
                "sc_head_alf",
                3,
                ScopeKind::Head,
            )
            .with_attributes(ScopeAttributes::new().with_agent_id(Some("agent-alf".into())))
            .with_owner_authorized(true),
        ),
    )
}
fn cerebellum() -> Stamp {
    stamped("cerebellum", 1)
}
fn plexus() -> Stamp {
    stamped("plexus", 3)
}
fn broca() -> Stamp {
    stamped("broca", 4)
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
fn card() -> Value {
    serde_json::from_str(include_str!(
        "../../../test-vectors/elicitation-request-v1/valid/app-grant-once.json"
    ))
    .unwrap()
}

struct Fixture {
    _dir: tempfile::TempDir,
    path: std::path::PathBuf,
    handler: Handler,
    runtime: Arc<Runtime>,
    clock: Arc<TestClock>,
    hook: Arc<WaitHook>,
}
impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("store.sqlite");
        let runtime = Arc::new(Runtime::default());
        let clock = Arc::new(TestClock(watch::channel(1000).0));
        let hook = Arc::new(WaitHook(watch::channel(0).0));
        let handler = Handler::new(Options {
            clock: clock.clone(),
            await_hook: Some(hook.clone()),
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
        }
    }
    async fn call(&self, stamp: &Stamp, method: &str, params: Value) -> Result<Value, String> {
        call(&self.handler, stamp, method, params).await
    }
    async fn ok(&self, stamp: &Stamp, method: &str, params: Value) -> Value {
        self.call(stamp, method, params)
            .await
            .unwrap_or_else(|code| panic!("{method}: {code}"))
    }
    async fn raise(&self, body: Value) -> String {
        self.ok(&cerebellum(), "consent.request", body).await["elicitation_id"]
            .as_str()
            .unwrap()
            .into()
    }
    async fn detach(&self, id: &str) -> Value {
        self.ok(
            &cerebellum(),
            "consent.detach",
            json!({"elicitation_id":id}),
        )
        .await
    }
    async fn answer(&self, id: &str, choice: &str) -> Value {
        self.ok(
            &phone(),
            "consent.answer",
            json!({"elicitationId":id,"choiceId":choice}),
        )
        .await
    }
    async fn deliver(&self) {
        // Let a spawned delivery run first, then flush the outbox directly.
        tokio::task::yield_now().await;
        self.handler.deliver_access_notices().await.unwrap();
        tokio::task::yield_now().await;
        self.handler.deliver_access_notices().await.unwrap();
    }
    fn sent(&self) -> Vec<String> {
        self.runtime.sent.lock().unwrap().clone()
    }
    fn late_sent(&self) -> Vec<String> {
        self.sent()
            .into_iter()
            .filter(|body| body.contains(r#""purpose":"consent_answered""#))
            .collect()
    }
    fn outbox_status(&self, id: &str) -> Option<(String, i64)> {
        rusqlite::Connection::open(&self.path)
            .unwrap()
            .query_row(
                "SELECT status,attempts FROM access_notices WHERE elicitation_id=?1 AND purpose='consent_answered'",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok()
    }
}

async fn call(
    handler: &Handler,
    stamp: &Stamp,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    match handler
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

fn mentions_detached(value: &Value) -> bool {
    value.to_string().contains("detached")
}

#[tokio::test]
async fn detach_reply_alone_reports_the_mark() {
    let f = Fixture::new().await;
    let answered = f.raise(card()).await;
    let withdrawn = f.raise(card()).await;

    let reply = f.detach(&answered).await;
    assert_eq!(reply["detached"], true);
    assert_eq!(reply["state"], "pending");
    assert_eq!(reply["elicitation_id"], answered);
    // A repeat returns the current record, still marked.
    assert_eq!(f.detach(&answered).await, reply);

    for method in ["consent.get", "consent.await"] {
        let pending = f
            .ok(
                &cerebellum(),
                method,
                json!({"elicitation_id":answered,"timeout_ms":0}),
            )
            .await;
        assert_eq!(pending["state"], "pending");
        assert!(!mentions_detached(&pending), "{method}: {pending}");
    }
    let mut unmarked = reply.clone();
    unmarked.as_object_mut().unwrap().remove("detached");
    assert_eq!(
        f.ok(
            &cerebellum(),
            "consent.get",
            json!({"elicitation_id":answered})
        )
        .await,
        unmarked
    );

    f.answer(&answered, "allow_once").await;
    let settled = f.detach(&answered).await;
    assert_eq!(settled["detached"], true);
    assert_eq!(settled["state"], "answered");
    for (stamp, method, params) in [
        (
            cerebellum(),
            "consent.get",
            json!({"elicitation_id":answered}),
        ),
        (
            cerebellum(),
            "consent.await",
            json!({"elicitation_id":answered,"timeout_ms":0}),
        ),
        (cerebellum(), "consent.answers", json!({"since":0})),
        (core(), "consent.outcomes", json!({"since":0})),
        (phone(), "consent.get", json!({"elicitationId":answered})),
    ] {
        let value = f.ok(&stamp, method, params).await;
        assert!(
            value.to_string().contains(&answered),
            "{method} must show the card"
        );
        assert!(!mentions_detached(&value), "{method}: {value}");
    }

    f.detach(&withdrawn).await;
    let readback = f
        .ok(
            &cerebellum(),
            "consent.withdraw",
            json!({"elicitation_id":withdrawn}),
        )
        .await;
    assert_eq!(readback["state"], "withdrawn");
    assert!(!mentions_detached(&readback), "{readback}");
}

#[tokio::test]
async fn late_answer_reaches_the_stamped_session_once_in_pinned_key_order() {
    let f = Fixture::new().await;
    let mut body = card();
    body["requester_ref"] = json!("tool-call-42");
    let id = f.raise(body).await;
    f.detach(&id).await;
    f.deliver().await;
    assert!(f.sent().is_empty(), "detach alone sends nothing");
    f.clock.0.send_replace(1500);
    assert_eq!(f.answer(&id, "allow_once").await["ok"], true);
    f.deliver().await;
    assert_eq!(
        f.sent(),
        vec![format!(
            r#"{{"elicitation_id":"{id}","purpose":"consent_answered","asker":{{"scope_ref":"sc_head_alf","scope_epoch":3}},"fields":{{"requester":"cerebellum","outcome":"answered","choice_id":"allow_once","choice_label":"Allow once","settled_at_ms":1500}}}}"#
        )]
    );
    assert!(!f.sent()[0].contains("requester_ref"));
    assert!(!f.sent()[0].contains("tool-call-42"));
    assert_eq!(f.outbox_status(&id), Some(("delivered".into(), 1)));
    // Deduplicated: a repeat detach or a later delivery pass sends nothing more.
    f.detach(&id).await;
    f.deliver().await;
    assert_eq!(f.sent().len(), 1);
}

#[tokio::test]
async fn plexus_late_answer_notice_names_plexus_as_requester() {
    let f = Fixture::new().await;
    let id = f.ok(&plexus(), "consent.request", card()).await["elicitation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    f.ok(&plexus(), "consent.detach", json!({"elicitation_id":id}))
        .await;
    f.deliver().await;
    f.clock.0.send_replace(1500);
    assert_eq!(f.answer(&id, "allow_once").await["ok"], true);
    f.deliver().await;
    assert_eq!(
        f.sent(),
        vec![format!(
            r#"{{"elicitation_id":"{id}","purpose":"consent_answered","asker":{{"scope_ref":"sc_head_alf","scope_epoch":3}},"fields":{{"requester":"plexus","outcome":"answered","choice_id":"allow_once","choice_label":"Allow once","settled_at_ms":1500}}}}"#
        )]
    );
}

#[tokio::test]
async fn broca_late_answer_notice_names_broca_as_requester() {
    let f = Fixture::new().await;
    let id = f.ok(&broca(), "consent.request", card()).await["elicitation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    f.ok(&broca(), "consent.detach", json!({"elicitation_id":id}))
        .await;
    f.deliver().await;
    f.clock.0.send_replace(1500);
    assert_eq!(f.answer(&id, "allow_once").await["ok"], true);
    f.deliver().await;
    assert_eq!(
        f.sent(),
        vec![format!(
            r#"{{"elicitation_id":"{id}","purpose":"consent_answered","asker":{{"scope_ref":"sc_head_alf","scope_epoch":3}},"fields":{{"requester":"broca","outcome":"answered","choice_id":"allow_once","choice_label":"Allow once","settled_at_ms":1500}}}}"#
        )]
    );
}

#[tokio::test]
async fn expiry_and_withdrawal_of_detached_cards_send_outcome_only_notices() {
    let f = Fixture::new().await;
    let expiring = f.raise(card()).await;
    let withdrawn = f.raise(card()).await;
    f.detach(&expiring).await;
    f.detach(&withdrawn).await;
    f.ok(
        &cerebellum(),
        "consent.withdraw",
        json!({"elicitation_id":withdrawn}),
    )
    .await;
    f.clock.0.send_replace(1000 + 60_000);
    f.handler.timer_tick().await.unwrap();
    f.deliver().await;
    let mut sent: Vec<Value> = f
        .late_sent()
        .iter()
        .map(|body| serde_json::from_str(body).unwrap())
        .collect();
    sent.sort_by_key(|notice| notice["fields"]["outcome"].as_str().unwrap().to_owned());
    assert_eq!(
        sent,
        vec![
            json!({"elicitation_id":expiring,"purpose":"consent_answered","asker":{"scope_ref":"sc_head_alf","scope_epoch":3},"fields":{"requester":"cerebellum","outcome":"expired","settled_at_ms":61000}}),
            json!({"elicitation_id":withdrawn,"purpose":"consent_answered","asker":{"scope_ref":"sc_head_alf","scope_epoch":3},"fields":{"requester":"cerebellum","outcome":"withdrawn","settled_at_ms":1000}}),
        ]
    );
}

#[tokio::test]
async fn never_detached_cards_send_no_late_answer_notice() {
    let f = Fixture::new().await;
    let answered = f.raise(card()).await;
    let withdrawn = f.raise(card()).await;
    let expired = f.raise(card()).await;
    let held = f.raise(card()).await;
    // The requester holds its call and receives the answer through await.
    let waiter = {
        let handler = f.handler.clone();
        let held = held.clone();
        tokio::spawn(async move {
            call(
                &handler,
                &cerebellum(),
                "consent.await",
                json!({"elicitation_id":held,"timeout_ms":30_000}),
            )
            .await
        })
    };
    f.hook.0.subscribe().wait_for(|n| *n == 1).await.unwrap();
    f.answer(&held, "allow_once").await;
    assert_eq!(waiter.await.unwrap().unwrap()["state"], "answered");
    f.answer(&answered, "deny").await;
    f.ok(
        &cerebellum(),
        "consent.withdraw",
        json!({"elicitation_id":withdrawn}),
    )
    .await;
    f.clock.0.send_replace(1000 + 60_000);
    f.handler.timer_tick().await.unwrap();
    f.deliver().await;
    assert_eq!(
        f.ok(
            &cerebellum(),
            "consent.get",
            json!({"elicitation_id":expired})
        )
        .await["state"],
        "expired"
    );
    assert_eq!(f.late_sent(), Vec::<String>::new());
    for id in [&answered, &withdrawn, &expired, &held] {
        assert_eq!(f.outbox_status(id), None);
    }
}

#[tokio::test]
async fn missing_session_records_unclaimed_without_blocking_settlement() {
    for refusal in ["unknown_session", "session_not_live"] {
        let f = Fixture::new().await;
        f.runtime
            .replies
            .lock()
            .unwrap()
            .push(NoticeResult::Refused(refusal.into()));
        let id = f.raise(card()).await;
        f.detach(&id).await;
        assert_eq!(f.answer(&id, "allow_once").await["ok"], true, "{refusal}");
        f.deliver().await;
        assert_eq!(
            f.outbox_status(&id),
            Some(("unclaimed".into(), 1)),
            "{refusal}"
        );
        f.clock.0.send_replace(1_000_000);
        f.deliver().await;
        assert_eq!(f.sent().len(), 1, "{refusal}: no retry");
        assert_eq!(
            f.ok(&cerebellum(), "consent.get", json!({"elicitation_id":id}))
                .await["state"],
            "answered",
            "{refusal}"
        );
    }
}

#[tokio::test]
async fn unreachable_core_retries_the_late_answer_from_the_outbox() {
    let f = Fixture::new().await;
    f.runtime
        .replies
        .lock()
        .unwrap()
        .push(NoticeResult::Transport("core unavailable".into()));
    let id = f.raise(card()).await;
    f.detach(&id).await;
    f.answer(&id, "allow_once").await;
    f.deliver().await;
    assert_eq!(f.outbox_status(&id), Some(("pending".into(), 1)));
    f.clock.0.send_replace(1000 + 250);
    f.deliver().await;
    assert_eq!(f.outbox_status(&id), Some(("delivered".into(), 2)));
    assert_eq!(f.late_sent().len(), 2);
    assert_eq!(f.late_sent()[0], f.late_sent()[1]);
}

/// While the requester's await is in flight, the operator's answer and the
/// requester's detach land in a fixed order. Whatever the await observes, the
/// store's commit order alone decides whether a late notice follows.
#[tokio::test]
async fn in_flight_await_detach_and_answer_are_classified_by_commit_order() {
    for detach_first in [false, true] {
        let f = Fixture::new().await;
        let id = f.raise(card()).await;
        let waiter = {
            let handler = f.handler.clone();
            let id = id.clone();
            tokio::spawn(async move {
                call(
                    &handler,
                    &cerebellum(),
                    "consent.await",
                    json!({"elicitation_id":id,"timeout_ms":30_000}),
                )
                .await
            })
        };
        f.hook.0.subscribe().wait_for(|n| *n == 1).await.unwrap();
        let reply = if detach_first {
            let reply = f.detach(&id).await;
            f.answer(&id, "allow_once").await;
            reply
        } else {
            f.answer(&id, "allow_once").await;
            f.detach(&id).await
        };
        assert_eq!(
            waiter.await.unwrap().unwrap()["state"],
            "answered",
            "detach_first={detach_first}"
        );
        f.deliver().await;
        if detach_first {
            assert_eq!(reply["state"], "pending");
            assert_eq!(reply["detached"], true);
            assert_eq!(f.late_sent().len(), 1);
        } else {
            assert_eq!(reply["state"], "answered");
            assert!(reply.get("detached").is_none(), "{reply}");
            assert_eq!(f.late_sent(), Vec::<String>::new());
        }
    }
}

#[tokio::test]
async fn detach_refuses_other_owners_unknown_ids_and_other_kinds() {
    let f = Fixture::new().await;
    let id = f.raise(card()).await;
    for (stamp, card) in [
        (stamped("plexus", 3), id.as_str()),
        (cerebellum(), "el_999"),
    ] {
        assert_eq!(
            f.call(&stamp, "consent.detach", json!({"elicitation_id":card}))
                .await
                .unwrap_err(),
            "elicitation_not_owner"
        );
    }
    assert_eq!(
        f.call(&phone(), "consent.detach", json!({"elicitation_id":id}))
            .await
            .unwrap_err(),
        "elicitation_requester_refused"
    );
    let aft = stamped("aft", 4);
    let ask = f
        .ok(
            &aft,
            "consent.request",
            json!({"kind":"tool_permission","call_key":"call-1","tool":"bash","operation":"read","paths":["/tmp/work"],"reason":"Read the folder","args_hash":"hash-1","pattern":"/tmp/work/**"}),
        )
        .await["elicitation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        f.call(&aft, "consent.detach", json!({"elicitation_id":ask}))
            .await
            .unwrap_err(),
        "elicitation_requester_refused"
    );
    assert_eq!(
        f.call(
            &cerebellum(),
            "consent.detach",
            json!({"elicitation_id":ask})
        )
        .await
        .unwrap_err(),
        "elicitation_not_owner"
    );
    let mut decision = card();
    decision["kind"] = json!("flow_decision");
    decision["flow_decision"] = json!({"arbitrary":"decision"});
    decision["options"] = json!([
        {"id":"proceed","label":"Proceed","effect":"choose"},
        {"id":"decline","label":"Decline","effect":"decline"},
    ]);
    decision["default"] = json!("decline");
    let basal = stamped("basal", 5);
    let flow = f.ok(&basal, "consent.request", decision).await["elicitation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        f.call(&basal, "consent.detach", json!({"elicitation_id":flow}))
            .await
            .unwrap_err(),
        "elicitation_requester_refused"
    );
    assert_eq!(
        f.ok(&cerebellum(), "consent.get", json!({"elicitation_id":id}))
            .await["state"],
        "pending"
    );
}

/// Raising the same card again (an in-place dedup refresh) means the requester
/// holds a new call for it, so the refresh clears the detach mark. Only a
/// detach after the refresh makes a later answer a late one.
#[tokio::test]
async fn dedup_refresh_clears_the_detach_mark() {
    for detach_again in [false, true] {
        let f = Fixture::new().await;
        let mut body = card();
        body["dedup_key"] = json!("tool-call-42");
        let id = f.raise(body.clone()).await;
        assert_eq!(f.detach(&id).await["detached"], true);
        assert_eq!(f.raise(body).await, id, "refresh keeps the card in place");
        if detach_again {
            let reply = f.detach(&id).await;
            assert_eq!(reply["state"], "pending");
            assert_eq!(reply["detached"], true);
        }
        assert_eq!(f.answer(&id, "allow_once").await["ok"], true);
        let reply = f.detach(&id).await;
        assert_eq!(reply["state"], "answered");
        f.deliver().await;
        if detach_again {
            assert_eq!(reply["detached"], true, "{reply}");
            assert_eq!(f.late_sent().len(), 1);
        } else {
            // Answered while the new call was held: no mark and no notice.
            assert!(reply.get("detached").is_none(), "{reply}");
            assert_eq!(f.late_sent(), Vec::<String>::new());
            assert_eq!(f.outbox_status(&id), None);
        }
    }
}
