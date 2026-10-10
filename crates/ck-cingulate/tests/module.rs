use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use async_trait::async_trait;
use cingulate_store::{ConsentStore, Isolation, StorageBackend, StorageDescriptor, RETENTION_MS};
use ck_cingulate::{AwaitHook, Clock, Handler, Notifier, Options, ResolvedAgent, Resolver, Stamp};
use rusqlite::{types::Value as SqlValue, Connection};
use serde_json::{json, Value};
use subc_client_rs::HandlerOutcome;
use subc_protocol::{
    manifest::{Concurrency, ManagementOperationKind, ProviderRole},
    session::HealthStatus,
    ModuleHelloAckBody, Principal,
};
use tempfile::TempDir;
use tokio::sync::{watch, Semaphore};

struct TestClock {
    time: watch::Sender<i64>,
    sleeps: watch::Sender<Vec<i64>>,
}
impl TestClock {
    fn new() -> Self {
        Self {
            time: watch::channel(1_000).0,
            sleeps: watch::channel(vec![]).0,
        }
    }
    fn set(&self, ms: i64) {
        self.time.send_replace(ms);
    }
    async fn sleeping_at(&self, target: i64) {
        let mut receiver = self.sleeps.subscribe();
        bounded(async {
            loop {
                if receiver.borrow_and_update().contains(&target) {
                    break;
                }
                receiver.changed().await.unwrap();
            }
        })
        .await;
    }
}
#[async_trait]
impl Clock for TestClock {
    fn now_ms(&self) -> i64 {
        *self.time.borrow()
    }
    async fn sleep_until(&self, ms: i64) {
        let mut receiver = self.time.subscribe();
        self.sleeps.send_modify(|sleeps| sleeps.push(ms));
        loop {
            if *receiver.borrow_and_update() >= ms {
                return;
            }
            receiver.changed().await.unwrap();
        }
    }
}

#[derive(Default)]
struct TestResolver {
    reply: Mutex<Option<Result<Option<ResolvedAgent>, String>>>,
}
#[async_trait]
impl Resolver for TestResolver {
    async fn resolve(&self, _: &str) -> Result<Option<ResolvedAgent>, String> {
        self.reply.lock().unwrap().clone().unwrap_or(Ok(None))
    }
}

struct Recorder {
    path: String,
    payloads: Mutex<Vec<Value>>,
    fail: AtomicBool,
    gate: Mutex<Option<Arc<Semaphore>>>,
    entered: watch::Sender<usize>,
}
#[async_trait]
impl Notifier for Recorder {
    async fn notify(&self, pointer: Value) -> Result<(), String> {
        // Inspect the actual committed tables through another connection, not
        // the handler's projection, to prove notification follows the commit.
        let db = Connection::open(&self.path).unwrap();
        let id = pointer["elicitationId"].as_str().unwrap();
        let count: i64 = db
            .query_row(
                "SELECT count(*) FROM elicitation_records WHERE elicitation_id=?1",
                [id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        if pointer["kind"] == "retraction" {
            let count: i64 = db
                .query_row(
                    "SELECT count(*) FROM elicitation_answers WHERE elicitation_id=?1",
                    [id],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(count, 1);
        }
        self.payloads.lock().unwrap().push(pointer);
        self.entered.send_modify(|n| *n += 1);
        let gate = self.gate.lock().unwrap().clone();
        if let Some(gate) = gate {
            gate.acquire().await.unwrap().forget();
        }
        if self.fail.load(Ordering::Relaxed) {
            Err("offline".into())
        } else {
            Ok(())
        }
    }
}

enum HookAction {
    Answer,
    Expire,
    Projection {
        refresh: Option<Value>,
        expected: i64,
    },
}
struct Hook {
    clock: Arc<TestClock>,
    action: Mutex<Option<HookAction>>,
    registered: watch::Sender<usize>,
}
#[async_trait]
impl AwaitHook for Hook {
    async fn after_register(&self, handler: &Handler, id: &str) {
        self.registered.send_modify(|n| *n += 1);
        let action = self.action.lock().unwrap().take();
        match action {
            Some(HookAction::Answer) => {
                let answer = ok(
                    handler,
                    &operator(),
                    "consent.answer",
                    json!({"elicitationId":id,"choiceId":"allow_once"}),
                )
                .await;
                assert_eq!(answer["ok"], true);
            }
            Some(HookAction::Expire) => {
                let record = ok(
                    handler,
                    &operator(),
                    "consent.get",
                    json!({"elicitationId":id}),
                )
                .await;
                self.clock.set(record["expiresAtMs"].as_i64().unwrap());
            }
            Some(HookAction::Projection { refresh, expected }) => {
                if let Some(refresh) = refresh {
                    assert_eq!(
                        ok(handler, &direct(), "consent.request", refresh).await["elicitation_id"],
                        id
                    );
                }
                let record = ok(
                    handler,
                    &operator(),
                    "consent.get",
                    json!({"elicitationId":id}),
                )
                .await;
                assert_eq!(record["blockUntilMs"], expected);
                let record = ok(
                    handler,
                    &direct(),
                    "consent.get",
                    json!({"elicitation_id":id}),
                )
                .await;
                assert_eq!(record["block_until_ms"], expected);
                self.clock.set(expected);
            }
            None => {}
        }
    }
}

struct Fixture {
    _dir: TempDir,
    descriptor: StorageDescriptor,
    clock: Arc<TestClock>,
    recorder: Arc<Recorder>,
    resolver: Arc<TestResolver>,
    hook: Arc<Hook>,
    handler: Handler,
}
impl Fixture {
    fn unopened() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cards.sqlite").to_str().unwrap().to_owned();
        let descriptor = StorageDescriptor {
            module_id: "cingulate".into(),
            storage_namespace: "main".into(),
            isolation: Isolation::Module,
            backend: StorageBackend::Sqlite { path: path.clone() },
        };
        let clock = Arc::new(TestClock::new());
        let recorder = Arc::new(Recorder {
            path,
            payloads: Mutex::new(vec![]),
            fail: AtomicBool::new(false),
            gate: Mutex::new(None),
            entered: watch::channel(0).0,
        });
        let resolver = Arc::new(TestResolver::default());
        let hook = Arc::new(Hook {
            clock: clock.clone(),
            action: Mutex::new(None),
            registered: watch::channel(0).0,
        });
        let handler = Self::build(
            &clock,
            &recorder,
            &resolver,
            &hook,
            dir.path().to_str().unwrap(),
        );
        Self {
            _dir: dir,
            descriptor,
            clock,
            recorder,
            resolver,
            hook,
            handler,
        }
    }
    fn build(
        clock: &Arc<TestClock>,
        recorder: &Arc<Recorder>,
        resolver: &Arc<TestResolver>,
        hook: &Arc<Hook>,
        data_home: &str,
    ) -> Handler {
        Handler::new(Options {
            clock: clock.clone(),
            resolver: resolver.clone(),
            notifier: recorder.clone(),
            await_hook: Some(hook.clone()),
            sweep_timer: false,
            data_home: data_home.into(),
        })
    }
    async fn open(machine: Option<&str>) -> Self {
        let fixture = Self::unopened();
        fixture.handler.hello_ack(&fixture.ack(machine)).await;
        assert_eq!(fixture.handler.health_report().status, HealthStatus::Ok);
        fixture
    }
    fn ack(&self, machine: Option<&str>) -> ModuleHelloAckBody {
        ModuleHelloAckBody {
            negotiated_ver: 1,
            subc_ops: vec![],
            subc_capabilities: vec![],
            storage: Some(serde_json::to_value(&self.descriptor).unwrap()),
            machine_id: machine.map(str::to_owned),
        }
    }
    async fn reopen(&mut self, machine: Option<&str>) {
        let replacement = Self::build(
            &self.clock,
            &self.recorder,
            &self.resolver,
            &self.hook,
            self._dir.path().to_str().unwrap(),
        );
        drop(std::mem::replace(&mut self.handler, replacement));
        self.handler.hello_ack(&self.ack(machine)).await;
        assert_eq!(self.handler.health_report().status, HealthStatus::Ok);
    }
    fn db(&self) -> Connection {
        Connection::open(&self.recorder.path).unwrap()
    }
    fn payloads(&self) -> Vec<Value> {
        self.recorder.payloads.lock().unwrap().clone()
    }
    fn clear(&self) {
        self.recorder.payloads.lock().unwrap().clear();
    }
    fn rows(&self) -> Value {
        let db = self.db();
        let tables: Vec<Value> = ["elicitation_records", "elicitation_answers"]
            .into_iter()
            .map(|table| {
                let mut stmt = db
                    .prepare(&format!("SELECT * FROM {table} ORDER BY elicitation_id"))
                    .unwrap();
                let columns = stmt.column_count();
                let rows: Vec<Value> = stmt
                    .query_map([], |row| {
                        Ok(Value::Array(
                            (0..columns)
                                .map(|i| match row.get::<_, SqlValue>(i).unwrap() {
                                    SqlValue::Null => Value::Null,
                                    SqlValue::Integer(v) => json!(v),
                                    SqlValue::Real(v) => json!(v),
                                    SqlValue::Text(v) => json!(v),
                                    SqlValue::Blob(v) => json!(v),
                                })
                                .collect(),
                        ))
                    })
                    .unwrap()
                    .map(Result::unwrap)
                    .collect();
                json!(rows)
            })
            .collect();
        json!(tables)
    }
    fn counts(&self) -> (i64, i64) {
        let db = self.db();
        (
            db.query_row("SELECT count(*) FROM elicitation_records", [], |r| r.get(0))
                .unwrap(),
            db.query_row("SELECT count(*) FROM elicitation_answers", [], |r| r.get(0))
                .unwrap(),
        )
    }
    async fn unchanged_error(
        &self,
        stamp: &Stamp,
        method: &str,
        params: Value,
        code: &str,
    ) -> String {
        let before = self.rows();
        let pointers = self.payloads();
        let message = error(&self.handler, stamp, method, params, code).await;
        assert_eq!(
            self.rows(),
            before,
            "refusal must not change either table: {method}"
        );
        assert_eq!(
            self.payloads(),
            pointers,
            "refusal must not notify: {method}"
        );
        message
    }
    async fn request(&self, stamp: &Stamp, body: Value) -> String {
        ok(&self.handler, stamp, "consent.request", body).await["elicitation_id"]
            .as_str()
            .unwrap()
            .into()
    }
    async fn get(&self, id: &str) -> Value {
        ok(
            &self.handler,
            &direct(),
            "consent.get",
            json!({"elicitation_id":id}),
        )
        .await
    }
    async fn answer(&self, id: &str, choice: &str) -> Value {
        ok(
            &self.handler,
            &operator(),
            "consent.answer",
            json!({"elicitationId":id,"choiceId":choice}),
        )
        .await
    }
}

fn stamp(label: &str) -> Stamp {
    Stamp {
        principal: Some(label.into()),
        bind_session: "session".into(),
        ..Stamp::default()
    }
}
fn direct() -> Stamp {
    stamp("direct")
}
fn operator() -> Stamp {
    Stamp {
        operator_surface: true,
        ..stamp("reserved:callosum")
    }
}
fn carrier() -> Stamp {
    Stamp {
        carried_agent_id: "agent-alf".into(),
        ..stamp("reserved:plexus")
    }
}
fn body() -> Value {
    serde_json::from_str(include_str!(
        "../../../test-vectors/elicitation-request-v1/valid/app-grant-once.json"
    ))
    .unwrap()
}
fn kind_body(kind: &str) -> Value {
    let mut body = body();
    body["kind"] = json!(kind);
    if kind != "consent" {
        body[kind] = json!({"opaque":true});
        body["options"] = json!([{"id":"allow_once","label":"yes","effect":if kind == "flow_decision" { "choose" } else { "grant" }},{"id":"deny","label":"no","effect":"decline"}]);
        body["default"] = json!("deny");
    }
    body
}
async fn bounded<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(1), future)
        .await
        .expect("in-process op exceeded 1 s")
}
async fn outcome(handler: &Handler, stamp: &Stamp, method: &str, params: Value) -> HandlerOutcome {
    bounded(handler.handle_stamped(
        stamp,
        &serde_json::to_vec(&json!({"method":method,"params":params})).unwrap(),
    ))
    .await
}
async fn ok(handler: &Handler, stamp: &Stamp, method: &str, params: Value) -> Value {
    match outcome(handler, stamp, method, params).await {
        HandlerOutcome::Response(bytes) => {
            let envelope: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(envelope.as_object().unwrap().len(), 1);
            envelope.get("result").expect("wrapped result").clone()
        }
        other => panic!("{method}: {other:?}"),
    }
}
async fn error(
    handler: &Handler,
    stamp: &Stamp,
    method: &str,
    params: Value,
    code: &str,
) -> String {
    match outcome(handler, stamp, method, params).await {
        HandlerOutcome::Error {
            code: actual,
            message,
        } => {
            assert_eq!(actual, code, "{method}: {message}");
            message
        }
        other => panic!("{method}: expected {code}, got {other:?}"),
    }
}

#[tokio::test]
async fn manifest_envelope_health_and_unknown_ops() {
    let manifest = ck_cingulate::manifest();
    assert_eq!(manifest.module_id, "cingulate");
    let capabilities = manifest.capabilities.unwrap();
    assert_eq!(capabilities.provides, ["consent/v1"]);
    assert!(capabilities.requires.is_empty() && capabilities.must_never_reach.is_empty());
    let ProviderRole::ManagementSurface {
        operations,
        concurrency,
        ..
    } = &manifest.provides[0]
    else {
        panic!("management surface")
    };
    assert_eq!(*concurrency, Concurrency::ModuleManaged);
    let expected = [
        ("consent.request", true),
        ("consent.get", false),
        ("consent.await", false),
        ("consent.answers", false),
        ("consent.ack", true),
        ("consent.report_execution", true),
        ("consent.withdraw", true),
        ("consent.answer", true),
        ("consent.list_pending", false),
        ("consent.outcomes", false),
    ];
    assert_eq!(operations.len(), expected.len());
    for (operation, (name, mutate)) in operations.iter().zip(expected) {
        assert_eq!(operation.name, name);
        assert_eq!(
            operation.kind,
            if mutate {
                ManagementOperationKind::Mutate
            } else {
                ManagementOperationKind::Query
            }
        );
    }
    let f = Fixture::unopened();
    assert_eq!(f.handler.health_report().status, HealthStatus::Failing);
    for (method, _) in expected {
        error(
            &f.handler,
            &Stamp::default(),
            method,
            Value::Null,
            "consent_unavailable",
        )
        .await;
    }
    let mut bad = f.ack(None);
    bad.storage = Some(json!({"invalid":"descriptor"}));
    f.handler.hello_ack(&bad).await;
    assert_eq!(f.handler.health_report().status, HealthStatus::Failing);
    let bad_path = f._dir.path().join("is-directory");
    std::fs::create_dir(&bad_path).unwrap();
    bad.storage = Some(
        json!({"module_id":"cingulate","storage_namespace":"main","isolation":"module","backend":{"kind":"sqlite","path":bad_path}}),
    );
    // Use the real descriptor serializer so this error reaches store open.
    let mut descriptor = f.descriptor.clone();
    descriptor.backend = StorageBackend::Sqlite {
        path: bad_path.to_str().unwrap().into(),
    };
    bad.storage = Some(serde_json::to_value(descriptor).unwrap());
    f.handler.hello_ack(&bad).await;
    assert_eq!(f.handler.health_report().status, HealthStatus::Failing);
    error(
        &f.handler,
        &direct(),
        "consent.request",
        body(),
        "consent_unavailable",
    )
    .await;
    f.handler.hello_ack(&f.ack(None)).await;
    assert_eq!(f.handler.health_report().status, HealthStatus::Ok);
    let mode: String = f
        .db()
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    for body in [b"not-json".as_slice(), br#"{}"#, br#"{"method":42}"#] {
        assert!(
            matches!(f.handler.handle_stamped(&direct(), body).await, HandlerOutcome::Error { code, .. } if code == "invalid_request")
        );
    }
    assert_eq!(
        f.handler
            .handle_stamped(&operator(), br#"{"method":"consent.list_pending"}"#)
            .await,
        HandlerOutcome::Response(br#"{"result":{"records":[]}}"#.to_vec())
    );
    for method in [
        "flow.revoke",
        "flow.install_status",
        "elicitation.request",
        "consent.unknown",
    ] {
        f.unchanged_error(&direct(), method, body(), "unknown_method")
            .await;
    }
}

#[test]
fn route_bind_decodes_flow_attribute_without_declaring_flow_capability() {
    // The SDK decodes this control shape before dispatching a bind. Recognizing
    // the daemon's wire vocabulary does not opt this module into flow behavior.
    let wire = json!({
        "op": "route.bind",
        "route_channel": 1,
        "epoch": 1,
        "target": {"kind": "management_surface", "module_id": "cingulate"},
        "identity": {"project_root": "/", "harness": "test", "session": "session"},
        "principal": {"kind": "unverified"},
        "scope": {
            "owner": {"kind": "reserved", "module_id": "prefrontal-core"},
            "ref": "flow-session",
            "scope_epoch": 1,
            "kind": "worker",
            "attributes": {"flow_id": "flow:7"},
            "owner_authorized": true
        }
    });
    let request: subc_protocol::session::ModuleControlRequest =
        serde_json::from_slice(&serde_json::to_vec(&wire).unwrap())
            .expect("current daemon flow attribute must decode");
    let subc_protocol::session::ModuleControlRequest::RouteBind {
        scope, principal, ..
    } = request
    else {
        panic!("route bind expected");
    };
    assert_eq!(
        serde_json::to_value(scope.unwrap()).unwrap()["attributes"]["flow_id"],
        "flow:7"
    );
    let stamp = Stamp::from_principal(principal.as_ref());
    assert_eq!(stamp.principal.as_deref(), Some("unverified"));
    assert!(!stamp.operator_surface);
    assert!(stamp.carried_agent_id.is_empty());
    assert_eq!(
        ck_cingulate::manifest().capabilities.unwrap().provides,
        ["consent/v1"]
    );
}

#[tokio::test]
async fn lease_retry_recovers_promptly_and_exhaustion_fails_closed() {
    let f = Fixture::unopened();
    let (holder, _) = ConsentStore::open(&f.descriptor, f.clock.now_ms()).unwrap();
    f.handler.hello_ack(&f.ack(None)).await;
    assert_eq!(f.handler.health_report().status, HealthStatus::Degraded);
    error(
        &f.handler,
        &direct(),
        "consent.request",
        body(),
        "consent_unavailable",
    )
    .await;
    f.clock.sleeping_at(1050).await;
    drop(holder);
    f.clock.set(1050);
    bounded(async {
        while f.handler.health_report().status != HealthStatus::Ok {
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert_eq!(f.clock.now_ms(), 1050);
    f.request(&direct(), body()).await;

    let g = Fixture::unopened();
    let (_holder, _) = ConsentStore::open(&g.descriptor, g.clock.now_ms()).unwrap();
    g.handler.hello_ack(&g.ack(None)).await;
    g.clock.sleeping_at(1050).await;
    g.clock.set(11_000);
    bounded(async {
        while g.handler.health_report().status == HealthStatus::Degraded {
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert_eq!(g.handler.health_report().status, HealthStatus::Failing);
}

#[tokio::test]
async fn request_settle_ack_and_withdraw_wire_contract() {
    let f = Fixture::open(None).await;
    let mut request = body();
    request["binding"] = json!({"k":1});
    let id = f.request(&direct(), request).await;
    assert!(id.starts_with("el_"));
    assert_eq!(
        f.payloads(),
        [json!({"elicitationId":id,"kind":"elicitation","eventAtMs":1000})]
    );
    let phone = ok(
        &f.handler,
        &operator(),
        "consent.get",
        json!({"elicitationId":id}),
    )
    .await;
    assert_eq!(phone["requesterLabel"], "direct");
    assert!(phone.get("binding").is_none() && phone.get("elicitation_id").is_none());
    assert_eq!(phone["elicitationId"], id);
    let owned = f.get(&id).await;
    assert!(owned.get("elicitationId").is_none());
    assert_eq!(owned["binding"], json!({"k":1}));
    f.unchanged_error(
        &operator(),
        "consent.get",
        json!({"elicitationId":"unknown"}),
        "elicitation_not_found",
    )
    .await;
    assert_eq!(f.answer(&id, "allow_once").await["alreadyAnswered"], false);
    let record = ok(
        &f.handler,
        &direct(),
        "consent.await",
        json!({"elicitation_id":id,"timeout_ms":120000}),
    )
    .await;
    assert_eq!(record["state"], "answered");
    assert_eq!(record["answered_choice_id"], "allow_once");
    assert_eq!(record["binding"], json!({"k":1}));
    assert_eq!(record["requester_label"], "direct");
    let feed = ok(&f.handler, &direct(), "consent.answers", json!({"since":0})).await;
    assert_eq!(feed["records"], json!([record]));
    assert_eq!(
        ok(
            &f.handler,
            &direct(),
            "consent.ack",
            json!({"cursor":feed["cursor"]})
        )
        .await,
        json!({"ok":true})
    );
    assert!(
        ok(&f.handler, &direct(), "consent.answers", json!({"since":0})).await["records"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    f.unchanged_error(
        &direct(),
        "consent.get",
        json!({"elicitation_id":id}),
        "elicitation_not_found",
    )
    .await;
    assert_eq!(f.counts(), (1, 1));
    let id = f.request(&direct(), body()).await;
    let withdrawn = ok(
        &f.handler,
        &direct(),
        "consent.withdraw",
        json!({"elicitation_id":id}),
    )
    .await;
    assert_eq!(withdrawn["state"], "withdrawn");
    assert!(withdrawn.get("answered_choice_id").is_none());
    let rows = f.rows();
    let pointers = f.payloads();
    assert_eq!(
        ok(
            &f.handler,
            &direct(),
            "consent.withdraw",
            json!({"elicitation_id":id})
        )
        .await,
        withdrawn
    );
    assert_eq!(f.rows(), rows);
    assert_eq!(f.payloads(), pointers);
}

#[tokio::test]
async fn owner_principal_and_operator_refusals_write_nothing() {
    let f = Fixture::open(None).await;
    let mut request = body();
    request["expires_in_ms"] = json!(1);
    let due = f.request(&direct(), request).await;
    f.clock.set(1001);
    for method in [
        "consent.get",
        "consent.await",
        "consent.withdraw",
        "consent.report_execution",
    ] {
        f.unchanged_error(
            &direct(),
            method,
            json!({"elicitation_id":"unknown","timeout_ms":0,"execution":"executed"}),
            "elicitation_not_owner",
        )
        .await;
    }
    for principal in [stamp("unverified"), Stamp::default()] {
        for method in [
            "consent.request",
            "consent.get",
            "consent.await",
            "consent.answers",
            "consent.ack",
            "consent.report_execution",
            "consent.withdraw",
        ] {
            let message = f
                .unchanged_error(
                    &principal,
                    method,
                    Value::Null,
                    "elicitation_requester_refused",
                )
                .await;
            assert!(message.contains(principal.principal.as_deref().unwrap_or("absent")));
        }
    }
    for params in [
        json!({"elicitationId":due,"choiceId":"allow_once"}),
        json!({"elicitationId":due}),
        Value::Null,
    ] {
        f.unchanged_error(
            &direct(),
            "consent.answer",
            params,
            "elicitation_operator_required",
        )
        .await;
    }
    f.unchanged_error(
        &direct(),
        "consent.list_pending",
        Value::Null,
        "elicitation_operator_required",
    )
    .await;
    let mut malformed = body();
    malformed["unknown"] = json!(true);
    for principal in [stamp("unverified"), Stamp::default()] {
        let message = f
            .unchanged_error(
                &principal,
                "consent.request",
                malformed.clone(),
                "elicitation_requester_refused",
            )
            .await;
        assert!(message.contains(principal.principal.as_deref().unwrap_or("absent")));
    }
    assert!(f
        .unchanged_error(
            &direct(),
            "consent.request",
            malformed,
            "elicitation_invalid_request"
        )
        .await
        .contains("unknown field `unknown`"));
    assert_eq!(f.get(&due).await["state"], "expired");
    assert_eq!(f.counts(), (1, 1));
}

#[tokio::test]
async fn allowlist_table_identity_and_order() {
    let f = Fixture::open(None).await;
    for (label, allowed, carried, expected_label) in [
        ("direct", Some("consent"), "", "an unverified local process"),
        (
            "reserved:cerebellum",
            Some("consent"),
            "agent-alf",
            "agent-alf",
        ),
        ("reserved:plexus", Some("consent"), "agent-alf", "agent-alf"),
        (
            "reserved:entorhinal",
            Some("consent"),
            "agent-alf",
            "agent-alf",
        ),
        (
            "reserved:prefrontal-core",
            Some("flow_install"),
            "",
            "reserved:prefrontal-core",
        ),
        (
            "reserved:basal",
            Some("flow_decision"),
            "",
            "operator decision",
        ),
        ("unverified", None, "", ""),
        ("reserved:other", None, "", ""),
    ] {
        let stamp = Stamp {
            carried_agent_id: carried.into(),
            ..stamp(label)
        };
        for kind in ["consent", "flow_install", "flow_decision"] {
            if allowed != Some(kind) {
                assert!(f
                    .unchanged_error(
                        &stamp,
                        "consent.request",
                        kind_body(kind),
                        "elicitation_requester_refused"
                    )
                    .await
                    .contains(label));
            } else {
                let id = f.request(&stamp, kind_body(kind)).await;
                let owner = ok(
                    &f.handler,
                    &stamp,
                    "consent.get",
                    json!({"elicitation_id":id}),
                )
                .await;
                assert_eq!(owner["subject"]["label"], expected_label);
                if carried.is_empty() {
                    assert!(owner["subject"].get("agent_id").is_none());
                    assert!(owner["subject"].get("head_agent_id").is_none());
                } else {
                    assert_eq!(owner["subject"]["agent_id"], carried);
                    assert_eq!(owner["subject"]["head_agent_id"], carried);
                }
                let stored: String = f
                    .db()
                    .query_row(
                        "SELECT record_json FROM elicitation_records WHERE elicitation_id=?1",
                        [&id],
                        |r| r.get(0),
                    )
                    .unwrap();
                assert_eq!(
                    serde_json::from_str::<Value>(&stored).unwrap()["requester"],
                    label
                );
                assert_eq!(
                    ok(
                        &f.handler,
                        &operator(),
                        "consent.get",
                        json!({"elicitationId":id})
                    )
                    .await["requesterLabel"],
                    label
                );
            }
        }
    }
    assert!(f
        .unchanged_error(
            &stamp("reserved:plexus"),
            "consent.request",
            body(),
            "elicitation_requester_refused"
        )
        .await
        .contains("reserved:plexus"));
    let mut request = body();
    request["kind"] = json!("unknown-kind");
    f.unchanged_error(
        &direct(),
        "consent.request",
        request,
        "elicitation_unknown_kind",
    )
    .await;
    let mut request = body();
    request["expires_in_ms"] = json!(0);
    f.unchanged_error(
        &stamp("reserved:plexus"),
        "consent.request",
        request,
        "elicitation_invalid_request",
    )
    .await;
    for key in ["agent_id", "subject", "requester"] {
        let mut request = body();
        request[key] = json!("spoofed");
        f.unchanged_error(
            &direct(),
            "consent.request",
            request,
            "elicitation_invalid_request",
        )
        .await;
    }
    *f.resolver.reply.lock().unwrap() = Some(Ok(Some(ResolvedAgent {
        name: Some("ALF".into()),
        retired: true,
    })));
    f.unchanged_error(&carrier(), "consent.request", body(), "consent_retired")
        .await;
    *f.resolver.reply.lock().unwrap() = Some(Err("offline".into()));
    let id = f.request(&carrier(), body()).await;
    assert_eq!(
        ok(
            &f.handler,
            &carrier(),
            "consent.get",
            json!({"elicitation_id":id})
        )
        .await["subject"]["label"],
        "agent-alf"
    );
    let production = Stamp::from_principal(Some(&Principal::Reserved {
        module_id: "plexus".into(),
    }));
    assert!(
        production.bind_session.is_empty()
            && production.carried_agent_id.is_empty()
            && production.console_machine_id.is_none()
    );
    assert!(!production.operator_surface);
    assert!(
        !Stamp::from_principal(Some(&Principal::Reserved {
            module_id: "callosum".into()
        }))
        .operator_surface
    );
}

#[tokio::test]
async fn console_machine_request_and_answer_gates() {
    for machine in [Some("m1"), None] {
        let f = Fixture::open(machine).await;
        f.request(&direct(), body()).await;
        let mut request = body();
        request["at_console"] = json!("m2");
        f.unchanged_error(
            &direct(),
            "consent.request",
            request,
            "elicitation_invalid_request",
        )
        .await;
        let mut request = body();
        request["at_console"] = json!("m1");
        if machine.is_none() {
            f.unchanged_error(
                &direct(),
                "consent.request",
                request,
                "elicitation_invalid_request",
            )
            .await;
            continue;
        }
        let id = f.request(&direct(), request).await;
        assert_eq!(
            ok(
                &f.handler,
                &operator(),
                "consent.get",
                json!({"elicitationId":id})
            )
            .await["atConsole"],
            "m1"
        );
        for console in [None, Some("m2")] {
            let answerer = Stamp {
                console_machine_id: console.map(str::to_owned),
                ..operator()
            };
            f.unchanged_error(
                &answerer,
                "consent.answer",
                json!({"elicitationId":id,"choiceId":"allow_once"}),
                "user_not_at_console",
            )
            .await;
        }
        assert_eq!(f.get(&id).await["state"], "pending");
        let answerer = Stamp {
            console_machine_id: Some("m1".into()),
            ..operator()
        };
        assert_eq!(
            ok(
                &f.handler,
                &answerer,
                "consent.answer",
                json!({"elicitationId":id,"choiceId":"allow_once"})
            )
            .await["elicitation"]["state"],
            "answered"
        );
        assert_eq!(f.counts(), (2, 1));
        assert_eq!(
            f.payloads().last().unwrap(),
            &json!({"elicitationId":id,"kind":"retraction","eventAtMs":1000,"machineId":"m1"})
        );
    }
}

#[tokio::test]
async fn answer_order_repeats_conflicts_and_expiry() {
    let f = Fixture::open(Some("m1")).await;
    let id = f.request(&direct(), body()).await;
    f.unchanged_error(
        &operator(),
        "consent.answer",
        json!({"elicitationId":id}),
        "elicitation_invalid_request",
    )
    .await;
    f.unchanged_error(
        &operator(),
        "consent.answer",
        json!({"elicitationId":id,"choiceId":"not-an-option"}),
        "elicitation_invalid_choice",
    )
    .await;
    assert_eq!(f.get(&id).await["state"], "pending");
    assert_eq!(f.answer(&id, "allow_once").await["alreadyAnswered"], false);
    let rows = f.rows();
    let pointers = f.payloads();
    assert_eq!(f.answer(&id, "allow_once").await["alreadyAnswered"], true);
    assert_eq!(f.answer(&id, "deny").await["code"], "conflict");
    assert_eq!(f.get(&id).await["answered_choice_id"], "allow_once");
    assert_eq!(
        f.answer("missing", "allow_once").await,
        json!({"ok":false,"code":"not_found"})
    );
    assert_eq!(f.rows(), rows);
    assert_eq!(f.payloads(), pointers);

    for case in ["flow_decision", "at_console", "non-option"] {
        let mut request = if case == "flow_decision" {
            kind_body(case)
        } else {
            body()
        };
        request["expires_in_ms"] = json!(10);
        if case == "at_console" {
            request["at_console"] = json!("m1");
        }
        let owner = if case == "flow_decision" {
            stamp("reserved:basal")
        } else {
            direct()
        };
        let id = f.request(&owner, request).await;
        f.clear();
        f.clock.set(f.clock.now_ms() + 10);
        let answer = f
            .answer(
                &id,
                if case == "flow_decision" {
                    "deny"
                } else if case == "non-option" {
                    "bogus"
                } else {
                    "allow_once"
                },
            )
            .await;
        assert_eq!(answer["code"], "conflict");
        assert!(answer.get("alreadyAnswered").is_none());
        assert_eq!(answer["elicitation"]["state"], "expired");
        assert_eq!(answer["elicitation"]["answeredAtMs"], f.clock.now_ms());
        if case == "flow_decision" {
            assert_eq!(answer["elicitation"]["answeredChoiceId"], "deny");
        } else {
            assert!(answer["elicitation"].get("answeredChoiceId").is_none());
        }
        assert_eq!(
            f.payloads(),
            [
                json!({"elicitationId":id,"kind":"retraction","eventAtMs":f.clock.now_ms(),"machineId":"m1"})
            ]
        );
    }
    assert_eq!(f.counts(), (4, 4));
    f.clock.set(1000 + RETENTION_MS + 1);
    ok(&f.handler, &direct(), "consent.answers", json!({"since":0})).await;
    let rows = f.rows();
    let pointers = f.payloads();
    assert_eq!(
        f.answer(&id, "allow_once").await,
        json!({"ok":false,"code":"not_found"})
    );
    assert_eq!(f.rows(), rows);
    assert_eq!(f.payloads(), pointers);
}

#[tokio::test]
async fn expiry_reads_restart_and_persistent_ids() {
    let mut f = Fixture::open(None).await;
    let mut request = body();
    request["expires_in_ms"] = json!(10);
    let id = f.request(&direct(), request).await;
    f.clock.set(1010);
    f.clear();
    f.reopen(None).await;
    assert_eq!(f.counts(), (1, 1));
    assert_eq!(
        f.payloads(),
        [json!({"elicitationId":id,"kind":"retraction","eventAtMs":1010})]
    );
    let record = f.get(&id).await;
    assert_eq!(record["state"], "expired");
    assert_eq!(record["answered_at_ms"], 1010);
    assert!(record.get("answered_choice_id").is_none());
    assert_eq!(
        ok(&f.handler, &operator(), "consent.list_pending", Value::Null).await,
        json!({"records":[]})
    );
    let rows = f.rows();
    let pointers = f.payloads();
    f.reopen(None).await;
    assert_eq!(f.rows(), rows);
    assert_eq!(f.payloads(), pointers);
    let next = f.request(&direct(), body()).await;
    assert_ne!(id, next);
    assert!(next.starts_with("el_"));
    assert_eq!(f.answer(&id, "allow_once").await["code"], "conflict");
    assert_eq!(f.get(&next).await["state"], "pending");
}

#[tokio::test]
async fn await_wakes_on_answer_without_blocking_other_requests() {
    let f = Fixture::open(None).await;
    let mut request = body();
    request["expires_in_ms"] = json!(240000);
    let id = f.request(&direct(), request).await;
    let handler = f.handler.clone();
    let awaited_id = id.clone();
    let task = tokio::spawn(async move {
        ok(
            &handler,
            &direct(),
            "consent.await",
            json!({"elicitation_id":awaited_id,"timeout_ms":120000}),
        )
        .await
    });
    f.clock.sleeping_at(121000).await;
    assert!(!task.is_finished());
    let other = Stamp {
        bind_session: "other".into(),
        ..direct()
    };
    f.request(&other, body()).await;
    let phone = ok(
        &f.handler,
        &operator(),
        "consent.get",
        json!({"elicitationId":id}),
    )
    .await;
    assert_eq!(phone["blockUntilMs"], 121000);
    f.answer(&id, "allow_once").await;
    let record = bounded(task).await.unwrap();
    assert_eq!(record["state"], "answered");
    assert!(record.get("block_until_ms").is_none());
    assert_eq!(f.clock.now_ms(), 1000);
}

#[tokio::test]
async fn await_hook_answer_expiry_projection_and_timeout() {
    for (expiry, expected) in [(60000, 61000), (240000, 121000)] {
        let f = Fixture::open(None).await;
        let mut request = body();
        request["expires_in_ms"] = json!(expiry);
        let id = f.request(&direct(), request).await;
        *f.hook.action.lock().unwrap() = Some(HookAction::Projection {
            refresh: None,
            expected,
        });
        let record = ok(
            &f.handler,
            &direct(),
            "consent.await",
            json!({"elicitation_id":id,"timeout_ms":120000}),
        )
        .await;
        assert!(record.get("block_until_ms").is_none());
        assert_eq!(
            record["state"],
            if expiry == 60000 {
                "expired"
            } else {
                "pending"
            }
        );
        assert!(ok(
            &f.handler,
            &operator(),
            "consent.get",
            json!({"elicitationId":id})
        )
        .await
        .get("blockUntilMs")
        .is_none());
    }
    for action in [HookAction::Answer, HookAction::Expire] {
        let f = Fixture::open(None).await;
        let mut request = body();
        request["expires_in_ms"] = json!(60000);
        let id = f.request(&direct(), request).await;
        let expires = matches!(action, HookAction::Expire);
        *f.hook.action.lock().unwrap() = Some(action);
        f.clear();
        let record = ok(
            &f.handler,
            &direct(),
            "consent.await",
            json!({"elicitation_id":id,"timeout_ms":120000}),
        )
        .await;
        assert_eq!(
            record["state"],
            if expires { "expired" } else { "answered" }
        );
        assert_eq!(record["answered_at_ms"], if expires { 61000 } else { 1000 });
        assert_eq!(f.counts(), (1, 1));
        assert_eq!(f.payloads().len(), 1);
        assert_eq!(f.payloads()[0]["eventAtMs"], record["answered_at_ms"]);
    }
    let f = Fixture::open(None).await;
    let mut request = body();
    request["dedup_key"] = json!("key");
    request["expires_in_ms"] = json!(60000);
    let id = f.request(&direct(), request.clone()).await;
    request["expires_in_ms"] = json!(1500);
    *f.hook.action.lock().unwrap() = Some(HookAction::Projection {
        refresh: Some(request),
        expected: 2500,
    });
    assert_eq!(
        ok(
            &f.handler,
            &direct(),
            "consent.await",
            json!({"elicitation_id":id,"timeout_ms":120000})
        )
        .await["state"],
        "expired"
    );
}

#[tokio::test]
async fn await_refresh_recalculates_injected_sleep_and_cancellation_unregisters() {
    let f = Fixture::open(None).await;
    let mut request = body();
    request["dedup_key"] = json!("key");
    request["expires_in_ms"] = json!(60000);
    let id = f.request(&direct(), request.clone()).await;
    let handler = f.handler.clone();
    let awaited = id.clone();
    let task = tokio::spawn(async move {
        ok(
            &handler,
            &direct(),
            "consent.await",
            json!({"elicitation_id":awaited,"timeout_ms":120000}),
        )
        .await
    });
    f.clock.sleeping_at(61000).await;
    request["expires_in_ms"] = json!(1500);
    assert_eq!(f.request(&direct(), request).await, id);
    f.clock.sleeping_at(2500).await;
    assert_eq!(
        ok(
            &f.handler,
            &operator(),
            "consent.get",
            json!({"elicitationId":id})
        )
        .await["blockUntilMs"],
        2500
    );
    f.clock.set(2500);
    assert_eq!(bounded(task).await.unwrap()["state"], "expired");
    let id = f.request(&direct(), body()).await;
    let handler = f.handler.clone();
    let awaited = id.clone();
    let task = tokio::spawn(async move {
        ok(
            &handler,
            &direct(),
            "consent.await",
            json!({"elicitation_id":awaited,"timeout_ms":100000}),
        )
        .await
    });
    f.clock.sleeping_at(62500).await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(ok(
        &f.handler,
        &operator(),
        "consent.get",
        json!({"elicitationId":id})
    )
    .await
    .get("blockUntilMs")
    .is_none());
}

#[tokio::test]
async fn await_uses_greatest_live_deadline_caps_timeout_and_returns_pending() {
    let f = Fixture::open(None).await;
    let mut request = body();
    request["expires_in_ms"] = json!(300000);
    let id = f.request(&direct(), request).await;
    let handler = f.handler.clone();
    let awaited = id.clone();
    let short = tokio::spawn(async move {
        ok(
            &handler,
            &direct(),
            "consent.await",
            json!({"elicitation_id":awaited,"timeout_ms":10000}),
        )
        .await
    });
    f.clock.sleeping_at(11000).await;
    let handler = f.handler.clone();
    let awaited = id.clone();
    let long = tokio::spawn(async move {
        ok(
            &handler,
            &direct(),
            "consent.await",
            json!({"elicitation_id":awaited,"timeout_ms":200000}),
        )
        .await
    });
    f.clock.sleeping_at(121000).await;
    assert_eq!(
        ok(
            &f.handler,
            &operator(),
            "consent.get",
            json!({"elicitationId":id})
        )
        .await["blockUntilMs"],
        121000
    );
    f.clock.set(11000);
    let record = bounded(short).await.unwrap();
    assert_eq!(record["state"], "pending");
    assert_eq!(record["block_until_ms"], 121000);
    assert!(!long.is_finished());
    f.clock.set(121000);
    let record = bounded(long).await.unwrap();
    assert_eq!(record["state"], "pending");
    assert!(record.get("block_until_ms").is_none());
    assert_eq!(f.counts(), (1, 0));
    assert_eq!(f.payloads().len(), 1);
}

#[tokio::test]
async fn invalid_params_do_not_sweep_and_extra_nonrequest_keys_are_accepted() {
    let f = Fixture::open(None).await;
    let mut request = body();
    request["expires_in_ms"] = json!(1);
    let id = f.request(&direct(), request).await;
    f.clock.set(1001);
    for (method, params) in [
        ("consent.get", json!({"elicitation_id":42})),
        (
            "consent.await",
            json!({"elicitation_id":id,"timeout_ms":-1}),
        ),
        (
            "consent.await",
            json!({"elicitation_id":id,"timeout_ms":1.5}),
        ),
        ("consent.answers", json!({"since":-1})),
        ("consent.answers", json!({"since":"0"})),
        ("consent.ack", json!({"cursor":null})),
        (
            "consent.report_execution",
            json!({"elicitation_id":id,"execution":42}),
        ),
        ("consent.withdraw", json!({})),
    ] {
        f.unchanged_error(&direct(), method, params, "elicitation_invalid_request")
            .await;
    }
    f.unchanged_error(
        &operator(),
        "consent.answer",
        json!({"elicitationId":id,"choiceId":false}),
        "elicitation_invalid_request",
    )
    .await;
    f.unchanged_error(
        &stamp("reserved:prefrontal-core"),
        "consent.outcomes",
        json!({"since":-1}),
        "elicitation_invalid_request",
    )
    .await;
    assert_eq!(
        ok(
            &f.handler,
            &direct(),
            "consent.get",
            json!({"elicitation_id":id,"ignored":true})
        )
        .await["state"],
        "expired"
    );
    assert_eq!(
        ok(
            &f.handler,
            &operator(),
            "consent.list_pending",
            json!({"ignored":true})
        )
        .await,
        json!({"records":[]})
    );
    assert_eq!(f.counts(), (1, 1));
}

#[tokio::test]
async fn dedup_refresh_restart_expiry_and_resolver_labels() {
    let mut f = Fixture::open(None).await;
    let mut request: Value = serde_json::from_str(include_str!(
        "../../../test-vectors/elicitation-request-v1/valid/app-grant-always.json"
    ))
    .unwrap();
    request["dedup_key"] = json!("key");
    request["expires_in_ms"] = json!(60000);
    let id = f.request(&direct(), request.clone()).await;
    let created = f.get(&id).await["created_at_ms"].clone();
    f.clear();
    assert_eq!(f.request(&direct(), request.clone()).await, id);
    f.reopen(None).await;
    assert_eq!(f.request(&direct(), request.clone()).await, id);
    let ids: Vec<Value> = request["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["id"].clone())
        .collect();
    request["requestedSchema"] =
        json!({"properties":{"decision":{"enum":ids}},"uncompared":"schema"});
    assert_eq!(f.request(&direct(), request.clone()).await, id);
    request["expires_in_ms"] = json!(120000);
    f.clock.set(2000);
    assert_eq!(f.request(&direct(), request.clone()).await, id);
    let record = f.get(&id).await;
    assert_eq!(record["created_at_ms"], created);
    assert_eq!(record["expires_in_ms"], 120000);
    assert_eq!(record["expires_at_ms"], 122000);
    assert_eq!(
        ok(
            &f.handler,
            &operator(),
            "consent.get",
            json!({"elicitationId":id})
        )
        .await["expiresInMs"],
        120000
    );
    assert!(f.payloads().is_empty());
    f.clock.set(122000);
    let next = f.request(&direct(), request).await;
    assert_ne!(next, id);
    assert_eq!(f.get(&id).await["state"], "expired");
    assert_eq!(
        f.payloads(),
        [
            json!({"elicitationId":id,"kind":"retraction","eventAtMs":122000}),
            json!({"elicitationId":next,"kind":"elicitation","eventAtMs":122000})
        ]
    );
    assert_eq!(f.counts(), (2, 1));
    assert_ne!(
        f.request(&direct(), body()).await,
        f.request(&direct(), body()).await
    );

    let mut request = body();
    request["dedup_key"] = json!("carrier");
    *f.resolver.reply.lock().unwrap() = Some(Err("offline".into()));
    let id = f.request(&carrier(), request.clone()).await;
    f.clear();
    *f.resolver.reply.lock().unwrap() = Some(Ok(Some(ResolvedAgent {
        name: Some("ALF".into()),
        retired: false,
    })));
    assert_eq!(f.request(&carrier(), request.clone()).await, id);
    assert_eq!(
        ok(
            &f.handler,
            &carrier(),
            "consent.get",
            json!({"elicitation_id":id})
        )
        .await["subject"]["label"],
        "ALF"
    );
    *f.resolver.reply.lock().unwrap() = Some(Err("offline".into()));
    assert_eq!(f.request(&carrier(), request.clone()).await, id);
    assert_eq!(
        ok(
            &f.handler,
            &carrier(),
            "consent.get",
            json!({"elicitation_id":id})
        )
        .await["subject"]["label"],
        "ALF"
    );
    assert!(f.payloads().is_empty());
    f.answer(&id, "allow_once").await;
    let previous = ok(
        &f.handler,
        &carrier(),
        "consent.get",
        json!({"elicitation_id":id}),
    )
    .await;
    assert_ne!(f.request(&carrier(), request).await, id);
    assert_eq!(
        ok(
            &f.handler,
            &carrier(),
            "consent.get",
            json!({"elicitation_id":id})
        )
        .await,
        previous
    );
}

#[tokio::test]
async fn dedup_content_replacements_retract_before_delivery() {
    for key in [
        "scope",
        "args_digest",
        "flow_install",
        "binding",
        "at_console",
    ] {
        let f = Fixture::open(Some("m1")).await;
        let owner = if key == "flow_install" {
            stamp("reserved:prefrontal-core")
        } else {
            direct()
        };
        let mut request = kind_body(if key == "flow_install" {
            "flow_install"
        } else {
            "consent"
        });
        request["dedup_key"] = json!("key");
        let id = f.request(&owner, request.clone()).await;
        f.clear();
        request[key] = match key {
            "args_digest" => json!("changed-digest"),
            "at_console" => json!("m1"),
            _ => json!({"changed":true}),
        };
        let next = f.request(&owner, request).await;
        assert_ne!(id, next);
        assert_eq!(
            f.payloads(),
            [
                json!({"elicitationId":id,"kind":"retraction","eventAtMs":1000,"machineId":"m1"}),
                json!({"elicitationId":next,"kind":"elicitation","eventAtMs":1000,"machineId":"m1"})
            ]
        );
        assert_eq!(f.answer(&id, "allow_once").await["code"], "conflict");
        assert_eq!(f.counts(), (2, 1));
    }
}

#[tokio::test]
async fn execution_reports_and_outcomes_survive_ack_and_restart() {
    let mut f = Fixture::open(None).await;
    let mut request = body();
    request["binding"] = json!({"private":true});
    let answered = f.request(&direct(), request).await;
    f.unchanged_error(
        &direct(),
        "consent.report_execution",
        json!({"elicitation_id":answered,"execution":"executed"}),
        "elicitation_store_error",
    )
    .await;
    f.unchanged_error(
        &direct(),
        "consent.report_execution",
        json!({"elicitation_id":answered,"execution":"not_run"}),
        "elicitation_invalid_execution",
    )
    .await;
    f.answer(&answered, "allow_once").await;
    let counts = f.counts();
    let pointers = f.payloads();
    assert_eq!(
        ok(
            &f.handler,
            &direct(),
            "consent.report_execution",
            json!({"elicitation_id":answered,"execution":"executed"})
        )
        .await["execution"],
        "executed"
    );
    assert_eq!(f.counts(), counts);
    assert_eq!(f.payloads(), pointers);
    let mut request = kind_body("flow_decision");
    request["expires_in_ms"] = json!(10);
    let expired = f.request(&stamp("reserved:basal"), request).await;
    f.unchanged_error(
        &stamp("reserved:basal"),
        "consent.report_execution",
        json!({"elicitation_id":expired,"execution":"executed"}),
        "elicitation_invalid_execution",
    )
    .await;
    let withdrawn = f.request(&carrier(), body()).await;
    ok(
        &f.handler,
        &carrier(),
        "consent.withdraw",
        json!({"elicitation_id":withdrawn}),
    )
    .await;
    f.clock.set(1010);
    let reader = stamp("reserved:prefrontal-core");
    let feed = ok(&f.handler, &reader, "consent.outcomes", json!({"since":0})).await;
    assert_eq!(feed["records"].as_array().unwrap().len(), 3);
    for record in feed["records"].as_array().unwrap() {
        assert!(record.get("binding").is_none());
        assert!(record.get("requester_label").is_some());
    }
    let cursor = feed["cursor"].clone();
    assert_eq!(
        ok(
            &f.handler,
            &reader,
            "consent.outcomes",
            json!({"since":cursor})
        )
        .await["records"],
        json!([])
    );
    for owner in [direct(), carrier(), stamp("reserved:basal")] {
        ok(&f.handler, &owner, "consent.ack", json!({"cursor":cursor})).await;
    }
    f.reopen(None).await;
    assert_eq!(
        ok(&f.handler, &reader, "consent.outcomes", json!({"since":0})).await,
        feed
    );
    for principal in [
        direct(),
        carrier(),
        stamp("reserved:basal"),
        stamp("reserved:callosum"),
        Stamp::default(),
    ] {
        f.unchanged_error(
            &principal,
            "consent.outcomes",
            Value::Null,
            "elicitation_requester_refused",
        )
        .await;
    }
    f.clock.set(1010 + RETENTION_MS + 1);
    ok(&f.handler, &direct(), "consent.answers", json!({"since":0})).await;
    f.reopen(None).await;
    let empty = ok(
        &f.handler,
        &reader,
        "consent.outcomes",
        json!({"since":cursor}),
    )
    .await;
    assert_eq!(empty["records"], json!([]));
    assert!(empty["cursor"].as_i64().unwrap() >= cursor.as_i64().unwrap());
    let next = f.request(&direct(), body()).await;
    f.answer(&next, "allow_once").await;
    let feed = ok(
        &f.handler,
        &reader,
        "consent.outcomes",
        json!({"since":cursor}),
    )
    .await;
    assert_eq!(feed["records"].as_array().unwrap().len(), 1);
    assert_eq!(feed["records"][0]["elicitation_id"], next);
    assert_eq!(
        ok(
            &f.handler,
            &reader,
            "consent.outcomes",
            json!({"since":feed["cursor"]})
        )
        .await["records"],
        json!([])
    );
}

#[tokio::test]
async fn retention_sweep_ops_and_timer_tick_preserve_tombstones() {
    for timer in [false, true] {
        let f = Fixture::open(None).await;
        let answered = f.request(&direct(), body()).await;
        f.answer(&answered, "allow_once").await;
        let withdrawn = f.request(&direct(), body()).await;
        ok(
            &f.handler,
            &direct(),
            "consent.withdraw",
            json!({"elicitation_id":withdrawn}),
        )
        .await;
        let acknowledged = f.request(&carrier(), body()).await;
        f.answer(&acknowledged, "allow_once").await;
        let cursor = ok(
            &f.handler,
            &carrier(),
            "consent.answers",
            json!({"since":0}),
        )
        .await["cursor"]
            .clone();
        ok(
            &f.handler,
            &carrier(),
            "consent.ack",
            json!({"cursor":cursor}),
        )
        .await;
        let log_before = f.rows()[1]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row[1] != acknowledged)
            .cloned()
            .collect::<Vec<_>>();
        f.clock.set(1000 + RETENTION_MS + 1);
        if timer {
            f.handler.timer_tick().await.unwrap();
        } else {
            ok(&f.handler, &direct(), "consent.answers", json!({"since":0})).await;
        }
        assert_eq!(f.counts(), (2, 2));
        assert_eq!(f.rows()[1], json!(log_before));
        let full: i64 = f
            .db()
            .query_row(
                "SELECT count(*) FROM elicitation_records WHERE record_json IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(full, 0);
        let feed = ok(&f.handler, &direct(), "consent.answers", json!({"since":0})).await;
        assert_eq!(
            feed["records"],
            json!([{"elicitation_id":answered,"state":"expired","settled_at":1000}])
        );
        let feed = ok(
            &f.handler,
            &stamp("reserved:prefrontal-core"),
            "consent.outcomes",
            json!({"since":0}),
        )
        .await;
        assert_eq!(
            feed["records"],
            json!([{"elicitation_id":answered,"state":"expired","settled_at":1000},{"elicitation_id":withdrawn,"state":"expired","settled_at":1000}])
        );
        f.unchanged_error(
            &direct(),
            "consent.get",
            json!({"elicitation_id":answered}),
            "elicitation_not_found",
        )
        .await;
    }
}

#[tokio::test]
async fn notifier_is_awaited_after_commit_and_errors_do_not_fail_ops() {
    let f = Fixture::open(None).await;
    let gate = Arc::new(Semaphore::new(0));
    *f.recorder.gate.lock().unwrap() = Some(gate.clone());
    let mut entered = f.recorder.entered.subscribe();
    let handler = f.handler.clone();
    let task =
        tokio::spawn(async move { ok(&handler, &direct(), "consent.request", body()).await });
    bounded(entered.changed()).await.unwrap();
    assert!(!task.is_finished());
    assert_eq!(f.counts(), (1, 0));
    gate.add_permits(1);
    let id = bounded(task).await.unwrap()["elicitation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    *f.recorder.gate.lock().unwrap() = None;
    f.recorder.fail.store(true, Ordering::Relaxed);
    assert_eq!(f.answer(&id, "allow_once").await["ok"], true);
    f.request(&direct(), body()).await;
    assert_eq!(f.counts(), (2, 1));
    assert_eq!(f.payloads().len(), 3);
}

fn package_body() -> Value {
    serde_json::from_str::<Value>(include_str!(
        "../../../test-vectors/consent-package-card-v1/request.json"
    ))
    .unwrap()["params"]
        .clone()
}

#[tokio::test]
async fn package_request_is_restricted_to_prefrontal_core() {
    let f = Fixture::open(None).await;
    for label in [
        "direct",
        "reserved:plexus",
        "reserved:cerebellum",
        "reserved:entorhinal",
        "reserved:basal",
        "reserved:callosum",
        "reserved:unknown",
    ] {
        let mut requester = stamp(label);
        requester.carried_agent_id = "agent-alf".into();
        f.unchanged_error(
            &requester,
            "consent.request",
            package_body(),
            "elicitation_requester_refused",
        )
        .await;
    }
    f.request(&stamp("reserved:prefrontal-core"), package_body())
        .await;
    assert_eq!(f.counts(), (1, 0));
    f.unchanged_error(
        &stamp("reserved:prefrontal-core"),
        "consent.request",
        body(),
        "elicitation_requester_refused",
    )
    .await;
}

#[tokio::test]
async fn request_accepts_exact_size_cap_and_refuses_one_byte_over_without_writes() {
    let f = Fixture::open(None).await;
    let requester = stamp("reserved:prefrontal-core");
    let mut request = package_body();
    request["package"]["script"] = json!("");
    let framing = serde_json::to_vec(&json!({"method":"consent.request","params":request.clone()}))
        .unwrap()
        .len();
    let script = "s".repeat(1_310_720 - framing);
    request["package"]["script"] = json!(script);
    let mut bytes =
        serde_json::to_vec(&json!({"method":"consent.request","params":request})).unwrap();
    assert_eq!(bytes.len(), 1_310_720);
    let HandlerOutcome::Response(reply) = f.handler.handle_stamped(&requester, &bytes).await else {
        panic!("exact size cap refused")
    };
    let reply: Value = serde_json::from_slice(&reply).unwrap();
    let record = ok(
        &f.handler,
        &requester,
        "consent.get",
        json!({"elicitation_id":reply["result"]["elicitation_id"]}),
    )
    .await;
    assert_eq!(record["package"]["script"], script);
    assert_eq!(f.counts(), (1, 0));
    let rows = f.rows();
    let pointers = f.payloads();
    bytes.push(b' ');
    assert!(
        matches!(f.handler.handle_stamped(&requester, &bytes).await, HandlerOutcome::Error { code, .. } if code == "elicitation_request_too_large")
    );
    assert_eq!(f.rows(), rows);
    assert_eq!(f.payloads(), pointers);
}

#[tokio::test]
async fn package_pending_answers_and_outcomes_page_below_three_mib_and_get_keeps_bodies() {
    let f = Fixture::open(None).await;
    let requester = stamp("reserved:prefrontal-core");
    let mut ids = Vec::new();
    // Large display text survives projection, so omitting source alone is not a page limit.
    for i in 0..10 {
        let mut request = package_body();
        request["dedup_key"] = json!(format!("package-{i}"));
        request["prompt"] = json!("p".repeat(700_000));
        request["package"]["script"] = json!("s".repeat(300_000));
        request["package"]["manifest_json"] = json!(format!("\"{}\"", "m".repeat(290_000)));
        let encoded =
            serde_json::to_vec(&json!({"method":"consent.request","params":request.clone()}))
                .unwrap();
        assert!((1_280_000..=1_310_720).contains(&encoded.len()));
        ids.push(f.request(&requester, request).await);
    }
    let full = ok(
        &f.handler,
        &requester,
        "consent.get",
        json!({"elicitation_id":ids[0]}),
    )
    .await;
    assert_eq!(full["package"]["script"].as_str().unwrap().len(), 300_000);
    assert_eq!(
        full["package"]["manifest_json"].as_str().unwrap().len(),
        290_002
    );
    let phone = ok(
        &f.handler,
        &operator(),
        "consent.get",
        json!({"elicitationId":ids[0]}),
    )
    .await;
    assert_eq!(phone["package"], full["package"]);
    let mut params = json!({});
    let mut seen = std::collections::BTreeSet::new();
    let mut pages = 0;
    loop {
        let reply = ok(&f.handler, &operator(), "consent.list_pending", params).await;
        assert!(
            serde_json::to_vec(&json!({"result":reply.clone()}))
                .unwrap()
                .len()
                <= 3 * 1024 * 1024
        );
        for record in reply["records"].as_array().unwrap() {
            assert!(record["package"].get("script").is_none());
            assert!(record["package"].get("manifest_json").is_none());
            assert_eq!(record["package"]["version"], 4);
            assert!(seen.insert(record["elicitationId"].as_str().unwrap().to_owned()));
        }
        pages += 1;
        if let Some(next) = reply.get("continuation") {
            params = json!({"continuation":next});
        } else {
            break;
        }
        assert!(pages <= 10);
    }
    assert!(pages >= 3);
    assert_eq!(seen, ids.iter().cloned().collect());
    for id in &ids {
        f.answer(id, "allow").await;
    }
    for method in ["consent.answers", "consent.outcomes"] {
        let mut since = 0;
        let mut seen = std::collections::BTreeSet::new();
        let mut pages = 0;
        loop {
            let reply = ok(&f.handler, &requester, method, json!({"since":since})).await;
            assert!(
                serde_json::to_vec(&json!({"result":reply.clone()}))
                    .unwrap()
                    .len()
                    <= 3 * 1024 * 1024
            );
            for record in reply["records"].as_array().unwrap() {
                assert!(record["package"].get("script").is_none());
                assert!(record["package"].get("manifest_json").is_none());
                assert!(seen.insert(record["elicitation_id"].as_str().unwrap().to_owned()));
            }
            pages += 1;
            if let Some(next) = reply.get("continuation") {
                since = next.as_i64().unwrap();
                assert_eq!(reply["cursor"], since);
                if method == "consent.answers" {
                    ok(
                        &f.handler,
                        &requester,
                        "consent.ack",
                        json!({"cursor":since}),
                    )
                    .await;
                }
            } else {
                break;
            }
            assert!(pages <= 10);
        }
        assert!(pages >= 3);
        assert_eq!(seen, ids.iter().cloned().collect());
    }
}

#[tokio::test]
async fn attested_carrier_routes_store_subject_and_isolate_scope_epochs() {
    use subc_client_rs::{RouteBindRequest, RouteHandle};
    use subc_protocol::{BindIdentity, RouteTarget};
    let f = Fixture::open(None).await;
    for principal in ["cerebellum", "plexus", "entorhinal"] {
        let scope = serde_json::from_value(json!({
            "owner":{"kind":"reserved","module_id":"prefrontal-core"}, "ref":"head:alf", "scope_epoch":7,
            "kind":"head", "attributes":{"agent_id":"agent-alf"}, "owner_authorized":true
        })).unwrap();
        let req = RouteBindRequest::new(
            RouteHandle::detached(1, 1),
            RouteTarget::ManagementSurface {
                module_id: "cingulate".into(),
            },
            BindIdentity::new("/scratch", "ignored", "forged-session"),
        )
        .with_principal(Principal::Reserved {
            module_id: principal.into(),
        })
        .with_scope(scope);
        let requester = Stamp::from_bind(&req);
        let id = f.request(&requester, body()).await;
        let record = ok(
            &f.handler,
            &requester,
            "consent.get",
            json!({"elicitation_id":id}),
        )
        .await;
        assert_eq!(record["subject"]["agent_id"], "agent-alf");
        let mut changed = req.clone();
        changed.scope.as_mut().unwrap().scope_epoch += 1;
        f.unchanged_error(
            &Stamp::from_bind(&changed),
            "consent.get",
            json!({"elicitation_id":id}),
            "elicitation_not_owner",
        )
        .await;
        changed.scope = None;
        f.unchanged_error(
            &Stamp::from_bind(&changed),
            "consent.request",
            body(),
            "elicitation_requester_refused",
        )
        .await;
    }
}
