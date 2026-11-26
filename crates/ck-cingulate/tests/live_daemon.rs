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
use cingulate_store::ScopeIdentity;
use ck_cingulate::{
    AccessRuntime, DaemonAccessRuntime, Handler, Options, ScopeLife, DEVELOPMENT_LOCAL_OPERATOR,
};
use cortexkit_test_support::ScratchDir;
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
    scope::{ScopeAttributes, ScopeCarrier, ScopeKind, ScopeParent, ScopeRecord},
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
const OWNER: &str = "prefrontal";

struct Task(JoinHandle<()>);

impl Drop for Task {
    fn drop(&mut self) {
        self.0.abort();
    }
}

// All peers run in the test process instead of spawned binaries. The fixture
// seeds supervisor spawn nonces for the scope owner and module callers, plus
// the owner's reserved nonce. Authentication, HELLO, scope.sync, route.open,
// route.bind and consent requests still cross authenticated loopback TCP.
struct Fixture {
    runtime: Arc<DaemonAccessRuntime>,
    tasks: Vec<Task>,
    consumer: SubcConsumer,
    owner: TcpStream,
    owner_id: &'static str,
    connection_path: PathBuf,
    store_path: PathBuf,
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
        Self::start_as(OWNER).await
    }

    async fn start_as(owner_id: &'static str) -> Self {
        Self::start_with_manifest(owner_id, ck_cingulate::manifest()).await
    }

    async fn start_with_manifest(owner_id: &'static str, manifest: ModuleManifest) -> Self {
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
            daemon_ver: "0.39.0".into(),
        };
        connection_file::write_atomic(&connection_path, &info).unwrap();
        let published = connection_file::read_for_client(&connection_path).unwrap();
        assert_eq!(published.pid, std::process::id());

        let supervisor = SupervisorHandle::new();
        for id in [
            owner_id,
            "plexus",
            "stranger",
            "callosum",
            "aft",
            "occipital",
        ] {
            supervisor.set_spawn_nonce(id, format!("test-launch-{id}"));
        }
        supervisor.set_reserved_nonce(owner_id, format!("test-launch-{owner_id}"));
        let control_handler = Arc::new(
            ControlHandler::new(Arc::new(Registry::default()))
                .with_scope_authority_owners(vec!["prefrontal-core".into(), "prefrontal".into()])
                .with_supervisor(supervisor),
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

        let cingulate_store::StorageBackend::Sqlite { path: store_path } =
            cingulate_store::storage_descriptor(None, data.to_str().unwrap()).backend
        else {
            panic!("scratch SQLite store")
        };
        let handler = Handler::new(Options {
            data_home: data.to_str().unwrap().into(),
            sweep_timer: false,
            ..Options::default()
        });
        let (module, serve) = bounded(serve_with_handle(
            &connection_path,
            manifest,
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
        let runtime = Arc::new(DaemonAccessRuntime::with_consumer(module, runtime_consumer));
        handler.set_permissions_runtime(runtime.clone());
        handler.set_access_runtime(runtime.clone());
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
                manifest: ModuleManifest::builder(owner_id, "0.1.0").build(),
                protocol_ver: PROTOCOL_VERSION,
                control_ops: None,
                launch_nonce: Some(format!("test-launch-{owner_id}")),
            },
        )
        .await;
        assert_eq!(ack.header.ty, FrameType::HelloAck, "owner HELLO: {ack:?}");

        Self {
            runtime,
            tasks,
            consumer,
            owner,
            owner_id,
            connection_path,
            store_path: store_path.into(),
            _scratch: scratch,
        }
    }

    fn identity(&self, harness: &str) -> BindIdentity {
        BindIdentity::new(harness, format!("untrusted-{harness}"))
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
        self.sync_project(generation, epoch, None).await;
    }

    async fn sync_project(&mut self, generation: u64, epoch: u64, project_id: Option<&str>) {
        let record = ScopeRecord::new("head-session", epoch, ScopeKind::Head)
            .with_carriers(vec![
                ScopeCarrier::new(principal("plexus")).with_targets(Some(vec!["cingulate".into()])),
                ScopeCarrier::new(principal("aft")).with_targets(Some(vec!["cingulate".into()])),
            ])
            .with_attributes(ScopeAttributes::new().with_agent_id(Some("agent-live".into())))
            .with_project_id(project_id.map(str::to_owned));
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
            ScopeSelector::new(principal(self.owner_id), "head-session", Some(epoch)),
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

    fn stored<T: serde::de::DeserializeOwned>(&self, table: &str, id: &str) -> T {
        let db = rusqlite::Connection::open_with_flags(
            &self.store_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let raw: String = db
            .query_row(
                &format!("SELECT record_json FROM {table} WHERE elicitation_id=?1"),
                [id],
                |row| row.get(0),
            )
            .unwrap();
        serde_json::from_str(&raw).unwrap()
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
            "presence/v1",
            "operator-app/v1",
            "peer-build/v1",
            "bind-no-project/v1",
            "consent/v1",
            "agent-run-scopes/v1",
            "consent-card-text/v1",
            "consent-card-explanations/v1",
            "consent-option-group/v1",
            "consent-single-use/v1",
            "consent-outcomes-late/v1",
            "consent-no-expiry/v1",
            "secret-use-approval/v1",
            "manifest-binding-approval/v1",
            "broker-action-approval/v1",
            "sandbox-escape-approval/v1",
            "consent-config-plan/v1",
            "flow-install-secrets/v1",
            "scope-workdir/v1",
            "scope-federated/v1"
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
            "consent.release_inputs",
            "consent.report_execution",
            "consent.report_unverified",
            "consent.request",
            "consent.watch_pending",
            "consent.withdraw",
            "permissions.list_all",
            "permissions.revoke",
            "presence.get",
            "presence.report",
            "presence.watch",
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
async fn live_daemon_presence_report_then_get() {
    let fixture = Fixture::start().await;
    let reporter = fixture.open(None, "plain-direct").await;
    let report = fixture
        .request(
            &reporter,
            "presence.report",
            json!({
                "app":"cktui", "device_kind":"phone", "state":"active", "ttl_ms":120_000,
                "focus":["agent-a"], "shows_inline":["ask"]
            }),
        )
        .await;
    if DEVELOPMENT_LOCAL_OPERATOR {
        let report = report.unwrap();
        assert_eq!(report["device_id"], "computer:cktui");
        let reader = fixture.open(Some(OWNER), "core").await;
        let get = fixture
            .request(&reader, "presence.get", json!({}))
            .await
            .unwrap();
        assert_eq!(get["where"], "computer");
        assert_eq!(get["devices"].as_array().unwrap().len(), 1);
        assert_eq!(get["devices"][0]["device_id"], "computer:cktui");
        assert_eq!(get["devices"][0]["device_kind"], "computer");
        assert_eq!(get["devices"][0]["expires_at_ms"], report["expires_at_ms"]);
        assert_eq!(get["watching"][0]["agent_id"], "agent-a");
        assert_eq!(get["watching"][0]["shows_inline"], json!(["ask"]));
    } else {
        refused(report, "presence_device_not_allowed");
    }
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
async fn live_daemon_forwards_project_stamp_to_consent_subject() {
    let mut fixture = Fixture::start().await;
    fixture.sync_project(1, 7, Some("pj-live")).await;
    let route = fixture.scoped(7, "project-carrier").await;
    let mut request = card();
    request["scope"] = json!({"project_id":"pj-forged", "name":"Forged project"});
    let id = fixture
        .request(&route, "consent.request", request)
        .await
        .unwrap()["elicitation_id"]
        .clone();
    let record = fixture
        .request(&route, "consent.get", json!({"elicitation_id": id}))
        .await
        .unwrap();
    assert_eq!(record["subject"]["label"], "agent-live in project pj-live");
    let stored: cingulate_store::ElicitationRecord =
        fixture.stored("elicitation_records", id.as_str().unwrap());
    assert_eq!(stored.subject.label, "agent-live in project pj-live");
    fixture.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_daemon_sandbox_run_once_requires_production_on_each_phone_route() {
    const PHONE: &str = "fed-phone:48ec112eb467c1045fbd00e05cc64f67";
    for opted_in in [true, false] {
        let mut manifest = ck_cingulate::manifest();
        if !opted_in {
            manifest
                .capabilities
                .as_mut()
                .unwrap()
                .provides
                .retain(|cap| cap != "peer-build/v1");
        }
        let mut fixture = Fixture::start_with_manifest(OWNER, manifest).await;
        fixture.sync(1, 1).await;
        let aft = bounded(fixture.consumer.open_route_scoped(
            target(),
            fixture.identity("aft"),
            ScopeSelector::new(principal(OWNER), "head-session", Some(1)),
            call_options(Some("aft")),
        ))
        .await
        .unwrap();
        let filed = fixture
            .request(
                &aft,
                "consent.request",
                json!({"kind": "sandbox_escape", "sandbox_escape": {
                    "call_key": "sandbox-test", "command": "printf example", "cwd": "/example",
                    "shell": "/bin/sh", "escapes_level": "standard", "background": false,
                    "reason": "Test approval record", "expires_in_ms": 600_000
                }}),
            )
            .await
            .unwrap();
        let id = filed["elicitation_id"].as_str().unwrap();
        let answer = json!({"elicitationId": id, "choiceId": "run_once", "clientFeatures": ["sandbox-escape-approval"]});

        let unverified = fixture.open(Some("callosum"), PHONE).await;
        refused(
            fixture
                .request(&unverified, "consent.answer", answer.clone())
                .await,
            "peer_build_not_production",
        );
        let open = fixture
            .request(&aft, "consent.get", json!({"elicitation_id": id}))
            .await
            .unwrap();
        assert_eq!(open["state"], "pending");
        assert!(open.get("answered_choice_id").is_none());
        let mut identity = BindIdentity::new(PHONE, "production-session");
        identity.build_class = Some("production".into());
        let claiming = bounded(fixture.consumer.open_route(
            target(),
            identity,
            call_options(Some("callosum")),
        ))
        .await
        .unwrap();
        let reply = fixture
            .request(&claiming, "consent.answer", answer.clone())
            .await;
        if opted_in {
            let reply = reply.unwrap();
            assert_eq!(reply["alreadyAnswered"], false);
            assert_eq!(reply["elicitation"]["answeredChoiceId"], "run_once");
            refused(
                fixture.request(&unverified, "consent.answer", answer).await,
                "peer_build_not_production",
            );
        } else {
            // The same production claim does not reach a target that hasn't opted in.
            refused(reply, "peer_build_not_production");
            let open = fixture
                .request(&aft, "consent.get", json!({"elicitation_id": id}))
                .await
                .unwrap();
            assert_eq!(open["state"], "pending");
            assert!(open.get("answered_choice_id").is_none());
        }
        fixture.stop().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_daemon_direct_operator_follows_development_switch_without_phone_identity() {
    let mut fixture = Fixture::start().await;
    fixture.sync(1, 1).await;
    let aft = bounded(fixture.consumer.open_route_scoped(
        target(),
        fixture.identity("aft"),
        ScopeSelector::new(principal(OWNER), "head-session", Some(1)),
        call_options(Some("aft")),
    ))
    .await
    .unwrap();
    let route = fixture.open(None, "plain-direct").await;
    let device = "300c9c9603b92a4b39ed3958bf924011";
    for harness in ["plain-direct", &format!("fed-phone:{device}")] {
        let id = fixture
            .request(&route, "consent.request", card())
            .await
            .unwrap()["elicitation_id"]
            .clone();
        let answer_route = fixture.open(None, harness).await;
        let answer = fixture
            .request(
                &answer_route,
                "consent.answer",
                json!({"elicitationId": id, "choiceId": "allow_once"}),
            )
            .await;
        if DEVELOPMENT_LOCAL_OPERATOR {
            assert_eq!(answer.unwrap()["ok"], true);
            let record: cingulate_store::ElicitationRecord =
                fixture.stored("elicitation_records", id.as_str().unwrap());
            assert_eq!(record.answered_choice_id.as_deref(), Some("allow_once"));
            assert_eq!(record.answered_device, None);

            let permission = fixture.request(&aft, "consent.request", json!({
                "kind":"tool_permission", "call_key":harness, "tool":"bash", "operation":"exec",
                "paths":[], "command":"pwd", "reason":"Inspect folder", "args_hash":"hash",
                "schema_pin":"schema-1", "pattern":"pwd"
            })).await.unwrap()["elicitation_id"].clone();
            assert_eq!(
                fixture
                    .request(
                        &answer_route,
                        "consent.answer",
                        json!({"elicitationId":permission,"choiceId":"allow_once"})
                    )
                    .await
                    .unwrap()["ok"],
                true
            );
            let audit: cingulate_store::AccessRecord =
                fixture.stored("access_asks", permission.as_str().unwrap());
            assert_eq!(audit.attempts.len(), 1);
            assert_eq!(audit.attempts[0].principal, "direct");
            assert_eq!(audit.attempts[0].device, None);
            assert_eq!(audit.attempts[0].result, "accepted");
        } else {
            refused(answer, "elicitation_operator_required");
        }
        assert_eq!(
            fixture
                .request(&route, "consent.get", json!({"elicitation_id": id}))
                .await
                .unwrap()["state"],
            if DEVELOPMENT_LOCAL_OPERATOR {
                "answered"
            } else {
                "pending"
            }
        );
    }
    fixture.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_daemon_asker_liveness_accepts_workers_and_preserves_epoch_checks() {
    let mut fixture = Fixture::start().await;
    let identity = ScopeIdentity {
        scope_ref: "worker-session".into(),
        scope_epoch: 3,
        agent_id: None,
    };
    let head = ScopeRecord::new("head-session", 7, ScopeKind::Head);
    let mut worker = ScopeRecord::new("worker-session", 3, ScopeKind::Worker);
    worker.parent = Some(ScopeParent::new(principal(OWNER), "head-session", 7));
    let synced = control(
        &mut fixture.owner,
        FrameType::Request,
        2,
        ModuleControlRequestFromModule::ScopeSync {
            generation: 1,
            scopes: vec![head, worker],
        },
    )
    .await;
    assert_eq!(
        synced.header.ty,
        FrameType::Response,
        "scope.sync: {synced:?}"
    );
    match bounded(fixture.runtime.describe_asker(&identity))
        .await
        .unwrap()
    {
        ScopeLife::Live(live) => assert_eq!(live, identity),
        other => panic!("a live worker must not look ended: {other:?}"),
    }
    assert!(matches!(
        bounded(fixture.runtime.describe(&identity)).await.unwrap(),
        ScopeLife::Gone
    ));
    let replaced = ScopeIdentity {
        scope_epoch: 2,
        ..identity.clone()
    };
    assert!(matches!(
        bounded(fixture.runtime.describe_asker(&replaced))
            .await
            .unwrap(),
        ScopeLife::Gone
    ));
    let ended = control(
        &mut fixture.owner,
        FrameType::Request,
        3,
        ModuleControlRequestFromModule::ScopeSync {
            generation: 2,
            scopes: vec![],
        },
    )
    .await;
    assert_eq!(
        ended.header.ty,
        FrameType::Response,
        "scope.sync: {ended:?}"
    );
    assert!(matches!(
        bounded(fixture.runtime.describe_asker(&identity))
            .await
            .unwrap(),
        ScopeLife::Gone
    ));
    fixture.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_daemon_prefrontal_aliases_own_permission_asks() {
    for owner in ["prefrontal", "prefrontal-core"] {
        let mut fixture = Fixture::start_as(owner).await;
        fixture.sync(1, 7).await;
        let route = bounded(fixture.consumer.open_route_scoped(
            target(),
            fixture.identity("aft-permission"),
            ScopeSelector::new(principal(owner), "head-session", Some(7)),
            call_options(Some("aft")),
        ))
        .await
        .unwrap();
        let result = fixture.request(&route, "consent.request", json!({
            "kind":"tool_permission", "call_key":"alias-ask", "tool":"bash", "operation":"exec",
            "paths":[], "command":"pwd", "reason":"Inspect folder", "args_hash":"hash",
            "schema_pin":"schema-1", "pattern":"pwd"
        })).await.unwrap();
        let record: cingulate_store::AccessRecord =
            fixture.stored("access_asks", result["elicitation_id"].as_str().unwrap());
        assert_eq!(
            record.identity.scope_owner.as_deref(),
            Some(format!("reserved:{owner}").as_str())
        );
        assert_eq!(record.state, "pending:user");
        fixture.stop().await;
    }
}
