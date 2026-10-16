#![forbid(unsafe_code)]

use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use async_trait::async_trait;
use ck_cingulate::{DaemonAccessRuntime, Handler, Options};
use cortexkit_test_support::{process_alive, ScratchDir};
use serde_json::{json, Value};
use subc_client_rs::{
    serve_with_handle, BindDecision, CallError, CallOptions, ConsumerIdentity, ConsumerOptions,
    HandlerOutcome, ModuleHandler, RequestCtx, RouteBindRequest, RouteHandle, ScopeSelector,
    SubcConsumer,
};
use subc_daemon::{
    handle_connection, ControlHandler, Registry, Router, ServerAuth, SupervisorHandle,
};
use subc_protocol::{
    manifest::{
        CapabilityDeclarations, Concurrency, ManagementOperation, ManagementOperationKind,
        ModuleManifest, ProviderRole,
    },
    scope::{ScopeAttributes, ScopeCarrier, ScopeKind, ScopeRecord},
    session::ModuleControlRequestFromModule,
    BindIdentity, Flags, Frame, FrameType, ModuleHelloBody, Principal, Priority, RouteTarget,
    PROTOCOL_VERSION,
};
use subc_transport::{
    authenticate_client,
    connection_file::{self, ConnectionInfo, Endpoint},
    read_frame, write_frame,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{watch, Notify},
    task::{JoinHandle, JoinSet},
};

const DEADLINE: Duration = Duration::from_secs(5);
const OWNER: &str = "prefrontal-core";

struct Task(JoinHandle<()>);

impl Drop for Task {
    fn drop(&mut self) {
        self.0.abort();
    }
}

// All peers run in the test process instead of spawned binaries. The fixture
// seeds supervisor spawn nonces for the scope owner, carrier and stranger, plus
// the owner's reserved nonce. Authentication, HELLO, scope.sync, route.open,
// route.bind and consent requests still cross authenticated loopback TCP.
struct Fixture {
    tasks: Vec<Task>,
    consumer: SubcConsumer,
    owner: TcpStream,
    project: PathBuf,
    connection_path: PathBuf,
    _scratch: ScratchDir,
}

async fn bounded<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(DEADLINE, future)
        .await
        .expect("live daemon operation exceeded deadline")
}

fn principal(module_id: &str) -> Principal {
    Principal::Reserved {
        module_id: module_id.into(),
    }
}

fn call_options(module_id: Option<&str>) -> CallOptions {
    CallOptions {
        timeout: DEADLINE,
        route_retry_deadline: DEADLINE,
        consumer_identity: module_id.map(|id| ConsumerIdentity {
            module_id: id.into(),
            launch_nonce: format!("test-launch-{id}"),
        }),
        ..CallOptions::default()
    }
}

fn target() -> RouteTarget {
    RouteTarget::ManagementSurface {
        module_id: "cingulate".into(),
    }
}

async fn control(
    stream: &mut TcpStream,
    ty: FrameType,
    corr: u64,
    body: impl serde::Serialize,
) -> Frame {
    let frame = Frame::build(
        ty,
        Flags::new(false, Priority::Passive, false),
        0,
        0,
        corr,
        serde_json::to_vec(&body).unwrap(),
    )
    .unwrap();
    bounded(async {
        write_frame(stream, &frame).await.unwrap();
        loop {
            let reply = read_frame(stream)
                .await
                .unwrap()
                .expect("owner connection ended");
            if reply.header.corr == corr {
                return reply;
            }
        }
    })
    .await
}

impl Fixture {
    async fn start() -> Self {
        let scratch = ScratchDir::new("cingulate-live-daemon");
        let project = scratch.path().join("project");
        let data = scratch.path().join("data");
        for path in [&project, &data] {
            std::fs::create_dir_all(path).unwrap();
        }
        let connection_path = scratch.path().join("runtime/subc.connection");
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let info = ConnectionInfo {
            schema: 1,
            wire_version: Some(PROTOCOL_VERSION),
            endpoints: vec![Endpoint {
                host: address.ip().to_string(),
                port: address.port(),
            }],
            key: connection_file::generate_key().unwrap(),
            daemon_id: connection_file::generate_daemon_id().unwrap(),
            pid: std::process::id(),
            daemon_ver: "0.33.0".into(),
        };
        connection_file::write_atomic(&connection_path, &info).unwrap();
        let published = connection_file::read_for_client(&connection_path).unwrap();
        assert_eq!(published.pid, std::process::id());
        assert!(process_alive(i32::try_from(published.pid).unwrap()));

        let supervisor = SupervisorHandle::new();
        for id in [OWNER, "plexus", "stranger", "callosum"] {
            supervisor.set_spawn_nonce(id, format!("test-launch-{id}"));
        }
        supervisor.set_reserved_nonce(OWNER, format!("test-launch-{OWNER}"));
        let control_handler = Arc::new(
            ControlHandler::new(Arc::new(Registry::default())).with_supervisor(supervisor),
        );
        let router = Arc::new(Router::with_control_handler(control_handler));
        let auth = ServerAuth::new(info.key.clone(), info.daemon_id, info.daemon_ver.clone());
        let mut tasks = vec![Task(tokio::spawn(async move {
            // Owning connection tasks here also cancels them if an assertion panics.
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (stream, _) = accepted.unwrap();
                        connections.spawn(handle_connection(stream, router.clone(), auth.clone()));
                    }
                    Some(result) = connections.join_next() => {
                        result.unwrap().unwrap();
                    }
                }
            }
        }))];

        let handler = Handler::new(Options {
            data_home: data.to_str().unwrap().into(),
            sweep_timer: false,
            ..Options::default()
        });
        let (module, serve) = bounded(serve_with_handle(
            &connection_path,
            ck_cingulate::manifest(),
            handler.clone(),
        ))
        .await
        .unwrap();
        let runtime_consumer = bounded(SubcConsumer::connect(
            &connection_path,
            ConsumerOptions {
                handshake_timeout: DEADLINE,
                call_timeout: DEADLINE,
                ..Default::default()
            },
        ))
        .await
        .unwrap();
        handler.set_permissions_runtime(Arc::new(DaemonAccessRuntime::with_consumer(
            module,
            runtime_consumer,
        )));
        tasks.push(Task(tokio::spawn(async move {
            serve.await.unwrap();
        })));
        let consumer = bounded(SubcConsumer::connect(
            &connection_path,
            ConsumerOptions {
                handshake_timeout: DEADLINE,
                call_timeout: DEADLINE,
                ..ConsumerOptions::default()
            },
        ))
        .await
        .unwrap();

        let mut owner = bounded(TcpStream::connect(address)).await.unwrap();
        authenticate_client(&mut owner, &published, DEADLINE)
            .await
            .unwrap();
        let ack = control(
            &mut owner,
            FrameType::Hello,
            1,
            ModuleHelloBody {
                manifest: ModuleManifest::builder(OWNER, "0.1.0").build(),
                protocol_ver: PROTOCOL_VERSION,
                control_ops: None,
                launch_nonce: Some(format!("test-launch-{OWNER}")),
            },
        )
        .await;
        assert_eq!(ack.header.ty, FrameType::HelloAck, "owner HELLO: {ack:?}");

        Self {
            tasks,
            consumer,
            owner,
            project,
            connection_path,
            _scratch: scratch,
        }
    }

    fn identity(&self, harness: &str) -> BindIdentity {
        BindIdentity::new(
            self.project.to_str().unwrap(),
            harness,
            format!("untrusted-{harness}"),
        )
    }

    async fn open(&self, caller: Option<&str>, harness: &str) -> RouteHandle {
        bounded(
            self.consumer
                .open_route(target(), self.identity(harness), call_options(caller)),
        )
        .await
        .unwrap()
    }

    async fn sync(&mut self, generation: u64, epoch: u64) {
        let record = ScopeRecord::new("head-session", epoch, ScopeKind::Head)
            .with_carriers(vec![
                ScopeCarrier::new(principal("plexus")).with_targets(Some(vec!["cingulate".into()]))
            ])
            .with_attributes(ScopeAttributes::new().with_agent_id(Some("agent-live".into())));
        let reply = control(
            &mut self.owner,
            FrameType::Request,
            generation + 1,
            ModuleControlRequestFromModule::ScopeSync {
                generation,
                scopes: vec![record],
            },
        )
        .await;
        assert_eq!(
            reply.header.ty,
            FrameType::Response,
            "scope.sync: {reply:?}"
        );
        let body: Value = serde_json::from_slice(&reply.body).unwrap();
        assert_eq!(body["generation"], generation);
        assert_eq!(body["results"].as_array().unwrap().len(), 1);
        assert!(
            matches!(
                body["results"][0]["outcome"].as_str(),
                Some("created" | "replaced")
            ),
            "scope.sync: {body}"
        );
    }

    async fn scoped(&self, epoch: u64, harness: &str) -> RouteHandle {
        bounded(self.consumer.open_route_scoped(
            target(),
            self.identity(harness),
            ScopeSelector {
                owner: principal(OWNER),
                scope_ref: "head-session".into(),
                scope_epoch: Some(epoch),
            },
            call_options(Some("plexus")),
        ))
        .await
        .unwrap()
    }

    async fn request(
        &self,
        route: &RouteHandle,
        method: &str,
        params: Value,
    ) -> Result<Value, CallError> {
        bounded(self.consumer.request(
            route,
            serde_json::to_vec(&json!({"method": method, "params": params})).unwrap(),
            call_options(None),
        ))
        .await
        .map(|bytes| serde_json::from_slice::<Value>(&bytes).unwrap()["result"].clone())
    }

    async fn stop(mut self) {
        bounded(self.consumer.close()).await;
        for task in &self.tasks {
            task.0.abort();
        }
        // Await cancellation before removing the scratch store and connection file.
        for task in &mut self.tasks {
            let result = bounded(&mut task.0).await;
            assert!(result.is_ok() || result.unwrap_err().is_cancelled());
        }
    }
}

fn card() -> Value {
    serde_json::from_str(include_str!(
        "../../../test-vectors/elicitation-request-v1/valid/app-grant-once.json"
    ))
    .unwrap()
}

fn refused(result: Result<Value, CallError>, code: &str) {
    match result {
        Err(CallError::Module(body)) => assert_eq!(body.code, code, "{body:?}"),
        other => panic!("expected module refusal {code}, got {other:?}"),
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct ProviderCounts {
    binds: usize,
    closes: usize,
}

struct PermissionsProvider {
    counts: watch::Sender<ProviderCounts>,
    hang: AtomicBool,
    entered: Notify,
}

impl PermissionsProvider {
    fn new() -> Self {
        Self {
            counts: watch::channel(ProviderCounts::default()).0,
            hang: AtomicBool::new(false),
            entered: Notify::new(),
        }
    }

    async fn wait_for_baseline(&self, active: usize) {
        let mut counts = self.counts.subscribe();
        let result = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let snapshot = *counts.borrow_and_update();
                if snapshot.binds - snapshot.closes == active {
                    break;
                }
                counts.changed().await.unwrap();
            }
        })
        .await;
        assert!(
            result.is_ok(),
            "provider routes did not return to baseline {active}: {:?}",
            *self.counts.borrow()
        );
    }
}

struct PermissionsProviderHandle(Arc<PermissionsProvider>);

impl std::ops::Deref for PermissionsProviderHandle {
    type Target = PermissionsProvider;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[async_trait]
impl ModuleHandler for PermissionsProviderHandle {
    async fn handle(&self, ctx: RequestCtx, body: Vec<u8>) -> HandlerOutcome {
        let request: Value = serde_json::from_slice(&body).unwrap();
        match request["method"].as_str().unwrap() {
            "permissions.list" => {
                if self.hang.load(Ordering::Acquire) {
                    self.entered.notify_one();
                    ctx.cancelled().await;
                }
                HandlerOutcome::Response(
                    serde_json::to_vec(&json!({"result":{"grants":[]}})).unwrap(),
                )
            }
            "permissions.revoke" => HandlerOutcome::Error {
                code: "grant_locked".into(),
                message: "grant is locked".into(),
            },
            other => panic!("unexpected permissions provider method {other}"),
        }
    }

    async fn on_bind(&self, _: &RouteBindRequest) -> BindDecision {
        BindDecision::accept()
    }

    async fn on_bound(&self, _: &RouteHandle) {
        self.counts.send_modify(|counts| counts.binds += 1);
    }

    async fn on_route_gone(&self, _: &RouteHandle) {
        self.counts.send_modify(|counts| {
            assert!(counts.closes < counts.binds);
            counts.closes += 1;
        });
    }
}

impl Fixture {
    async fn permissions_provider(&mut self) -> Arc<PermissionsProvider> {
        let provider = Arc::new(PermissionsProvider::new());
        let manifest = ModuleManifest::builder("permission-store", "0.1.0")
            .capabilities(Some(CapabilityDeclarations {
                provides: vec!["permissions/v1".into()],
                requires: vec![],
                must_never_reach: vec![],
            }))
            .provides(vec![ProviderRole::ManagementSurface {
                operations: vec![
                    ManagementOperation {
                        name: "permissions.list".into(),
                        kind: ManagementOperationKind::Query,
                        description: None,
                    },
                    ManagementOperation {
                        name: "permissions.revoke".into(),
                        kind: ManagementOperationKind::Mutate,
                        description: None,
                    },
                ],
                config_schema: json!({}),
                observability: vec![],
                identity_scope: vec![],
                concurrency: Concurrency::ModuleManaged,
            }])
            .build();
        let (_, serving) = bounded(serve_with_handle(
            &self.connection_path,
            manifest,
            PermissionsProviderHandle(provider.clone()),
        ))
        .await
        .unwrap();
        self.tasks.push(Task(tokio::spawn(async move {
            serving.await.unwrap();
        })));
        provider
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_daemon_permissions_routes_return_to_baseline() {
    let mut fixture = Fixture::start().await;
    let provider = fixture.permissions_provider().await;
    let baseline = *provider.counts.borrow();
    let active = baseline.binds - baseline.closes;
    let route = fixture.open(None, "local").await;
    for _ in 0..20 {
        let reply = fixture
            .request(&route, "permissions.list_all", json!({}))
            .await
            .unwrap();
        assert_eq!(
            reply["modules"],
            json!([{"module":"permission-store","status":"ok","grants":[]}])
        );
    }
    assert_eq!(
        fixture
            .request(
                &route,
                "permissions.revoke",
                json!({"module":"permission-store","id":"one"})
            )
            .await
            .unwrap(),
        json!({"outcome":"refused","code":"grant_locked"})
    );
    provider.wait_for_baseline(active).await;
    let after = *provider.counts.borrow();
    assert_eq!(after.binds - baseline.binds, 21);
    assert_eq!(after.closes - baseline.closes, 21);
    fixture.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_daemon_permissions_wait_does_not_block_consent_or_health() {
    let mut fixture = Fixture::start().await;
    let provider = fixture.permissions_provider().await;
    provider.hang.store(true, Ordering::Release);
    {
        let route = fixture.open(Some("callosum"), "fed-phone:device").await;
        let mut options = call_options(None);
        options.timeout = Duration::from_secs(10);
        let inventory = fixture.consumer.request(
            &route,
            serde_json::to_vec(&json!({"method":"permissions.list_all","params":{}})).unwrap(),
            options,
        );
        tokio::pin!(inventory);
        tokio::select! {
            _ = bounded(provider.entered.notified()) => {}
            result = &mut inventory => panic!("inventory completed before provider hung: {result:?}"),
        }
        let counts = *provider.counts.borrow();
        assert_eq!(counts.binds - counts.closes, 1);
        let prompt = tokio::time::timeout(Duration::from_secs(1), async {
            tokio::join!(
                fixture.request(&route, "consent.list_pending", json!({})),
                fixture.request(&route, "health.check", json!({})),
            )
        })
        .await
        .expect("hung provider blocked consent or health");
        assert!(prompt.0.unwrap()["records"].as_array().unwrap().is_empty());
        assert_eq!(prompt.1.unwrap()["status"], "ok");
        let bytes = tokio::time::timeout(Duration::from_secs(10), &mut inventory)
            .await
            .unwrap()
            .unwrap();
        let reply: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            reply["result"]["modules"],
            json!([{"module":"permission-store","status":"unreachable"}])
        );
        provider.wait_for_baseline(0).await;
        assert_eq!(provider.counts.borrow().binds, 1);
        assert_eq!(provider.counts.borrow().closes, 1);
    }
    fixture.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_daemon_catalog_contains_consent_manifest() {
    use subc_protocol::manifest::{Concurrency, ProviderRole};
    let fixture = Fixture::start().await;
    let catalog = bounded(fixture.consumer.catalog_list()).await.unwrap();
    let module = catalog
        .modules
        .iter()
        .find(|module| module.module_id == "cingulate")
        .expect("SDK registration must reach the daemon catalog");
    assert!(module.ready);
    assert_eq!(
        module.module_version.as_deref(),
        Some(env!("CARGO_PKG_VERSION"))
    );
    let capabilities = module.capabilities.as_ref().unwrap();
    assert_eq!(
        capabilities.provides,
        [
            "consent/v1",
            "agent-run-scopes/v1",
            "consent-card-text/v1",
            "consent-option-group/v1"
        ]
    );
    assert!(capabilities.requires.is_empty() && capabilities.must_never_reach.is_empty());
    assert_eq!(module.roles.len(), 1);
    let ProviderRole::ManagementSurface {
        operations,
        concurrency,
        ..
    } = &module.roles[0]
    else {
        panic!("consent must register a management surface");
    };
    assert_eq!(*concurrency, Concurrency::ModuleManaged);
    let mut names: Vec<_> = operations
        .iter()
        .map(|operation| operation.name.as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "consent.ack",
            "consent.answer",
            "consent.answers",
            "consent.await",
            "consent.detach",
            "consent.get",
            "consent.list_pending",
            "consent.outcomes",
            "consent.report_execution",
            "consent.request",
            "consent.withdraw",
            "permissions.list_all",
            "permissions.revoke",
        ]
    );
    let route = fixture.open(None, "local").await;
    assert_eq!(
        fixture
            .request(&route, "health.check", json!({}))
            .await
            .unwrap()["status"],
        "ok"
    );
    fixture.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_daemon_refuses_unlisted_requester() {
    let fixture = Fixture::start().await;
    let route = fixture.open(Some("stranger"), "requester").await;
    let result = fixture.request(&route, "consent.request", card()).await;
    match result {
        Err(CallError::Module(body)) => {
            assert_eq!(body.code, "elicitation_requester_refused");
            assert!(body.message.contains("reserved:stranger"), "{body:?}");
        }
        other => panic!("unlisted requester must be refused by cingulate, got {other:?}"),
    }
    fixture.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_daemon_scope_stamp_owns_card_and_separates_epochs() {
    let mut fixture = Fixture::start().await;
    fixture.sync(1, 7).await;
    let route = fixture.scoped(7, "carrier-a").await;
    let id = fixture
        .request(&route, "consent.request", card())
        .await
        .unwrap()["elicitation_id"]
        .clone();
    let params = json!({"elicitation_id": id});
    let record = fixture
        .request(&route, "consent.get", params.clone())
        .await
        .unwrap();
    assert_eq!(record["subject"]["agent_id"], "agent-live");
    // Client-supplied bind sessions differ, but both routes belong to the same scope.
    let same_scope = fixture.scoped(7, "carrier-b").await;
    assert_eq!(
        fixture
            .request(&same_scope, "consent.get", params.clone())
            .await
            .unwrap(),
        record
    );
    let unscoped = fixture.open(Some("plexus"), "carrier-a").await;
    refused(
        fixture
            .request(&unscoped, "consent.get", params.clone())
            .await,
        "elicitation_not_owner",
    );
    refused(
        fixture.request(&unscoped, "consent.request", card()).await,
        "elicitation_requester_refused",
    );

    fixture.sync(2, 8).await;
    let next_epoch = fixture.scoped(8, "carrier-a").await;
    refused(
        fixture.request(&next_epoch, "consent.get", params).await,
        "elicitation_not_owner",
    );
    fixture.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_daemon_direct_routes_cannot_answer_even_with_phone_harness() {
    let fixture = Fixture::start().await;
    let route = fixture.open(None, "plain-direct").await;
    let id = fixture
        .request(&route, "consent.request", card())
        .await
        .unwrap()["elicitation_id"]
        .clone();
    for harness in ["plain-direct", "fed-phone:forged"] {
        let answer_route = fixture.open(None, harness).await;
        refused(
            fixture
                .request(
                    &answer_route,
                    "consent.answer",
                    json!({"elicitationId": id, "choiceId": "allow_once"}),
                )
                .await,
            "elicitation_operator_required",
        );
    }
    assert_eq!(
        fixture
            .request(&route, "consent.get", json!({"elicitation_id": id}))
            .await
            .unwrap()["state"],
        "pending"
    );
    fixture.stop().await;
}
