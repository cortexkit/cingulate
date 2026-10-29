#![forbid(unsafe_code)]

//! Consent lifecycle handler. The route stamp is the only identity input; card
//! operations authorize before doing any sweep, and notify only after commit.

use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        atomic::{AtomicU64, AtomicU8, Ordering},
        Arc, Mutex as StdMutex,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use cingulate_decode::{ConsentRequest, OptionGroup, Refusal};
use cingulate_store::{
    storage_descriptor, AnswerOutcome, ConsentStore, ElicitationRecord, RequestIdentity,
    ScopeIdentity, State, StorageDescriptor, Subject,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use subc_client_rs::{
    BindDecision, ConnectionEnd, HandlerOutcome, ModuleHandler, RequestCtx, RouteBindRequest,
    RouteCloseReason, RouteHandle,
};
use subc_protocol::{
    manifest::{
        CapabilityDeclarations, Concurrency, ManagementOperation, ManagementOperationKind,
        ModuleManifest, OperationDelivery, ProviderRole,
    },
    session::{HealthReport, HealthStatus},
    ModuleHelloAckBody, Principal,
};
use tokio::sync::{watch, Mutex};

mod access;
pub use access::{AccessRuntime, DaemonAccessRuntime, NoticeResult, ScopeLife};
pub mod engram;
mod execution;
mod identity;
pub use identity::EntorhinalResolver;
mod permissions;
pub use permissions::{PermissionsCallError, PermissionsRuntime};
mod pending_watch;
pub use pending_watch::{WatchSink, ITEM_LIMIT_BYTES, MAX_BACKLOG, MAX_WATCHES, WATCH_PENDING};

/// The module id the subc daemon registers. The backup catalog records the same
/// id, so Engram backs up the store of the module that actually wrote it.
pub const MODULE_ID: &str = "cingulate";

pub const OPERATIONS: [(&str, ManagementOperationKind); 15] = [
    ("consent.request", ManagementOperationKind::Mutate),
    ("consent.get", ManagementOperationKind::Query),
    ("consent.await", ManagementOperationKind::Query),
    ("consent.answers", ManagementOperationKind::Query),
    ("consent.ack", ManagementOperationKind::Mutate),
    ("consent.report_execution", ManagementOperationKind::Mutate),
    ("consent.withdraw", ManagementOperationKind::Mutate),
    ("consent.detach", ManagementOperationKind::Mutate),
    ("consent.answer", ManagementOperationKind::Mutate),
    ("consent.revoke", ManagementOperationKind::Mutate),
    ("consent.list_pending", ManagementOperationKind::Query),
    (WATCH_PENDING, ManagementOperationKind::Query),
    ("consent.outcomes", ManagementOperationKind::Query),
    ("permissions.list_all", ManagementOperationKind::Query),
    ("permissions.revoke", ManagementOperationKind::Mutate),
];

/// Where a requester's card gets the agent it is about. The agent always comes
/// from the daemon's route stamp, never from the request body.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SubjectAgent {
    /// The requester acts for an agent on every card: a stamp without one is refused.
    Carried,
    /// The requester acts for an agent when it files the card on that agent's
    /// scope (an agent's flow install); without one the card is the requester's own.
    FromScope,
    /// The card is always the requester's own.
    Never,
}

#[derive(Clone, Copy)]
struct RequesterAdmission {
    principal: &'static str,
    requester_name: Option<&'static str>,
    kinds: &'static [&'static str],
    agent: SubjectAgent,
}

// Keep every requester admission beside its phone display name. `direct` is a
// legacy generic-consent caller without a stable human identity, so it has no
// display name and must never be shown as a raw principal.
const REQUESTER_ADMISSIONS: &[RequesterAdmission] = &[
    RequesterAdmission {
        principal: "reserved:occipital",
        requester_name: Some("Occipital"),
        kinds: &["rich_ui_approval"],
        agent: SubjectAgent::Never,
    },
    RequesterAdmission {
        principal: "reserved:cerebellum",
        requester_name: Some("Cerebellum"),
        kinds: &["consent", "secret_use"],
        agent: SubjectAgent::Carried,
    },
    RequesterAdmission {
        principal: "reserved:plexus",
        requester_name: Some("Plexus"),
        kinds: &["consent"],
        agent: SubjectAgent::Carried,
    },
    RequesterAdmission {
        principal: "reserved:broca",
        requester_name: Some("Broca"),
        kinds: &["consent"],
        agent: SubjectAgent::Carried,
    },
    RequesterAdmission {
        principal: "reserved:entorhinal",
        requester_name: Some("Entorhinal"),
        kinds: &["consent"],
        agent: SubjectAgent::Carried,
    },
    RequesterAdmission {
        principal: "direct",
        requester_name: None,
        kinds: &["consent"],
        agent: SubjectAgent::Never,
    },
    RequesterAdmission {
        principal: "reserved:basal",
        requester_name: Some("Basal"),
        kinds: &["flow_decision"],
        agent: SubjectAgent::Never,
    },
    RequesterAdmission {
        principal: "reserved:prefrontal-core",
        requester_name: Some("Prefrontal"),
        kinds: &["flow_install", "package"],
        agent: SubjectAgent::FromScope,
    },
    RequesterAdmission {
        principal: "reserved:aft",
        requester_name: Some("AFT"),
        kinds: &["tool_permission", "macos_permission", "secret_use"],
        agent: SubjectAgent::Never,
    },
];

fn requester_admission(principal: &str) -> Option<&'static RequesterAdmission> {
    REQUESTER_ADMISSIONS
        .iter()
        .find(|admission| admission.principal == principal)
}

fn requester_name(principal: &str) -> Option<&'static str> {
    requester_admission(principal).and_then(|admission| admission.requester_name)
}

pub fn manifest() -> ModuleManifest {
    ModuleManifest::builder(MODULE_ID, env!("CARGO_PKG_VERSION"))
        .capabilities(Some(CapabilityDeclarations {
            provides: vec![
                "consent/v1".into(),
                "agent-run-scopes/v1".into(),
                "consent-card-text/v1".into(),
                "consent-option-group/v1".into(),
                "consent-single-use/v1".into(),
                "consent-outcomes-late/v1".into(),
                "consent-no-expiry/v1".into(),
                "rich-ui-approval/v1".into(),
                "secret-use-approval/v1".into(),
                "flow-install-secrets/v1".into(),
                "scope-workdir/v1".into(),
            ],
            requires: vec![],
            must_never_reach: vec![],
        }))
        .provides(vec![ProviderRole::ManagementSurface {
            operations: OPERATIONS
                .into_iter()
                .map(|(name, kind)| ManagementOperation {
                    name: name.into(),
                    kind,
                    description: None,
                })
                .collect(),
            config_schema: json!({"type":"object"}),
            observability: vec![],
            identity_scope: vec![],
            concurrency: Concurrency::ModuleManaged,
        }])
        // The watch holds its request open and streams card changes until the
        // caller unsubscribes, the route closes or the module stops.
        .operation_delivery(BTreeMap::from([(
            WATCH_PENDING.to_owned(),
            OperationDelivery::WATCH,
        )]))
        .build()
}

/// During development, no caller check may block an app or module being built,
/// so plain local programs (`Principal::Direct`) may act as the operator surface.
/// Set to false before public release.
pub const DEVELOPMENT_LOCAL_OPERATOR: bool = true;

/// Requester, session and subject identity taken from the daemon's route bind,
/// never from a consent request body.
#[derive(Debug, Default, Clone)]
pub struct Stamp {
    pub scope: Option<subc_protocol::scope::ScopeStamp>,
    pub harness: String,
    pub principal: Option<String>,
    pub bind_session: String,
    pub carried_agent_id: String,
    pub operator_surface: bool,
    /// Operator authority supplied by the development opening, not by a phone.
    pub local_operator: bool,
    pub sim_surface: bool,
    pub console_machine_id: Option<String>,
}

impl Stamp {
    pub fn from_principal(principal: Option<&Principal>) -> Self {
        let principal = principal.map(|principal| match principal {
            Principal::Direct => "direct".to_owned(),
            Principal::Unverified => "unverified".to_owned(),
            Principal::Reserved { module_id } => format!("reserved:{module_id}"),
        });
        Self {
            operator_surface: false,
            principal,
            ..Self::default()
        }
    }

    pub fn from_bind(req: &RouteBindRequest) -> Self {
        Self::from_bind_with_local_operator(req, DEVELOPMENT_LOCAL_OPERATOR)
    }

    fn from_bind_with_local_operator(req: &RouteBindRequest, local_operator: bool) -> Self {
        let mut stamp = Self::from_principal(req.principal.as_ref());
        stamp.scope = req.scope.clone();
        stamp.harness = req.identity.harness.clone();
        if let Some(scope) = &req.scope {
            // A scope reference can be reused for a new session. Including its
            // owner and epoch prevents that session from reading the previous session's cards.
            stamp.bind_session =
                serde_json::to_string(&(&scope.owner, &scope.scope_ref, scope.scope_epoch))
                    .expect("scope identity serialization");
            stamp.carried_agent_id = scope.attributes.agent_id.clone().unwrap_or_default();
        }
        // Callosum's phone routes retain operator authority independently of the
        // development opening for daemon-stamped plain local programs.
        stamp.local_operator = local_operator && matches!(req.principal, Some(Principal::Direct));
        stamp.operator_surface = (stamp.label() == "reserved:callosum"
            && req.identity.harness.starts_with("fed-phone:"))
            || stamp.local_operator;
        // Simulators can read the permissions inventory, but cannot narrow grants
        // or answer cards. Keep this separate from consent's phone authority.
        stamp.sim_surface =
            stamp.label() == "reserved:callosum" && req.identity.harness.starts_with("fed-sim:");
        stamp
    }

    // Local programs retain their requester operations. Only the camelCase get
    // form selects the operator projection; phone routes keep their dispatch.
    fn operator_handles(&self, method: &str, params: &Value) -> bool {
        self.operator_surface
            && (!self.local_operator
                || matches!(
                    method,
                    "consent.answer" | "consent.list_pending" | WATCH_PENDING
                )
                || (method == "consent.get" && params.get("elicitationId").is_some()))
    }

    fn label(&self) -> &str {
        self.principal.as_deref().unwrap_or("absent")
    }
    fn answering_device(&self) -> Option<&str> {
        if self.label() != "reserved:callosum" {
            return None;
        }
        let (class, fingerprint) = self.harness.strip_prefix("fed-")?.split_once(':')?;
        // Callosum names the computer class `computer`; builds before that rename
        // stamp `mac`, so both are read until no computer runs one.
        (matches!(class, "phone" | "sim" | "computer" | "mac")
            && cingulate_decode::lowercase_hex(fingerprint, 32))
        .then_some(fingerprint)
    }

    fn rich_ui_device_matches(&self, record: &ElicitationRecord) -> bool {
        record
            .request
            .rich_ui_approval
            .as_ref()
            .is_none_or(|body| self.answering_device() == Some(body.device.as_str()))
    }
    fn secret_use_phone(&self) -> Option<&str> {
        let fingerprint = self.harness.strip_prefix("fed-phone:")?;
        (self.label() == "reserved:callosum" && cingulate_decode::lowercase_hex(fingerprint, 32))
            .then_some(fingerprint)
    }
    fn card_visible(&self, record: &ElicitationRecord, params: &Value) -> bool {
        self.rich_ui_device_matches(record)
            && (record.request.kind != "rich_ui_approval"
                || feature_declared(params, "rich-ui-approval"))
            && (!record.request.is_secret_card()
                || (self.secret_use_phone().is_some()
                    && feature_declared(params, "secret-use-approval")))
    }
    fn owner(&self) -> String {
        json!([self.label(), self.bind_session]).to_string()
    }
    fn allowed_kind(&self) -> Option<&'static [&'static str]> {
        requester_admission(self.label()).map(|admission| admission.kinds)
    }
    fn admits_kind(&self, kind: &str) -> bool {
        self.allowed_kind()
            .is_some_and(|kinds| kinds.contains(&kind))
    }
    fn refuse(&self) -> Refusal {
        Refusal::new(
            "elicitation_requester_refused",
            format!("requester {} is refused", self.label()),
        )
    }
    fn authorize_owner(&self) -> Result<(), Refusal> {
        self.allowed_kind().map(|_| ()).ok_or_else(|| self.refuse())
    }
}

#[async_trait]
pub trait Clock: Send + Sync {
    fn now_ms(&self) -> i64;
    async fn sleep_until(&self, ms: i64);
}

pub struct SystemClock;
#[async_trait]
impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(i64::MAX as u128) as i64
    }
    async fn sleep_until(&self, ms: i64) {
        tokio::time::sleep(Duration::from_millis(
            ms.saturating_sub(self.now_ms()).max(0) as u64,
        ))
        .await;
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedAgent {
    pub name: Option<String>,
    pub retired: bool,
}
#[async_trait]
pub trait Resolver: Send + Sync {
    async fn resolve(&self, agent_id: &str) -> Result<Option<ResolvedAgent>, String>;

    /// Best-effort display name for a daemon-stamped project id; not an authorization input.
    async fn project_name(&self, _project_id: &str) -> Option<String> {
        None
    }
}
pub struct UnavailableResolver;
#[async_trait]
impl Resolver for UnavailableResolver {
    async fn resolve(&self, _: &str) -> Result<Option<ResolvedAgent>, String> {
        Ok(None)
    }
}

#[async_trait]
pub trait Notifier: Send + Sync {
    async fn notify(&self, pointer: Value) -> Result<(), String>;
}
pub struct NoopNotifier;
#[async_trait]
impl Notifier for NoopNotifier {
    async fn notify(&self, _: Value) -> Result<(), String> {
        Ok(())
    }
}

/// Runs once after await registration, with no store lock or transaction open.
#[async_trait]
pub trait AwaitHook: Send + Sync {
    async fn after_register(&self, handler: &Handler, id: &str);
}

pub struct Options {
    pub clock: Arc<dyn Clock>,
    pub resolver: Arc<dyn Resolver>,
    pub notifier: Arc<dyn Notifier>,
    pub await_hook: Option<Arc<dyn AwaitHook>>,
    pub sweep_timer: bool,
    pub data_home: String,
    /// Change-log rows the store keeps for consent.watch_pending replays.
    pub change_log_rows: i64,
}
impl Default for Options {
    fn default() -> Self {
        let data_home = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| {
            format!("{}/.local/share", std::env::var("HOME").unwrap_or_default())
        });
        Self {
            clock: Arc::new(SystemClock),
            resolver: Arc::new(UnavailableResolver),
            notifier: Arc::new(NoopNotifier),
            await_hook: None,
            sweep_timer: true,
            data_home,
            change_log_rows: cingulate_store::CHANGE_LOG_ROWS,
        }
    }
}

struct Waiter {
    deadline: i64,
    wake: watch::Sender<()>,
}
#[derive(Default)]
struct Waiters {
    next: u64,
    cards: HashMap<String, HashMap<u64, Waiter>>,
}
struct Registration {
    inner: Arc<Inner>,
    id: String,
    token: u64,
}
impl Drop for Registration {
    fn drop(&mut self) {
        let mut waiters = self.inner.waiters.lock().expect("waiter lock");
        if let Some(card) = waiters.cards.get_mut(&self.id) {
            card.remove(&self.token);
            if card.is_empty() {
                waiters.cards.remove(&self.id);
            }
        }
    }
}

struct Inner {
    access_runtime: StdMutex<Arc<dyn AccessRuntime>>,
    permissions_runtime: StdMutex<Arc<dyn PermissionsRuntime>>,
    notice_delivery: Mutex<()>,
    execution_maintenance: Mutex<execution::Maintenance>,
    options: Options,
    store: Mutex<Option<ConsentStore>>,
    health: AtomicU8,
    machine_id: StdMutex<Option<String>>,
    waiters: StdMutex<Waiters>,
    routes: StdMutex<HashMap<RouteHandle, Stamp>>,
    timer: StdMutex<Option<tokio::task::AbortHandle>>,
    retry: StdMutex<Option<tokio::task::AbortHandle>>,
    /// The store connection's write count at the last check; see `note_writes`.
    writes: AtomicU64,
    /// Ticks after any store write, waking open watches to read the change log.
    changes: watch::Sender<()>,
    /// Set once the module is stopping; open watches end and new ones are refused.
    stopping: watch::Sender<bool>,
    watch_slots: Arc<tokio::sync::Semaphore>,
}
impl Drop for Inner {
    fn drop(&mut self) {
        for task in [&self.timer, &self.retry] {
            if let Some(task) = task.lock().expect("task lock").take() {
                task.abort();
            }
        }
    }
}

#[derive(Clone)]
pub struct Handler {
    inner: Arc<Inner>,
}
type Reply = Result<Value, Refusal>;

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new("elicitation_invalid_request", message)
}

fn option_group_declared(params: &Value) -> Result<bool, Refusal> {
    let Some(features) = params.get("clientFeatures") else {
        return Ok(false);
    };
    let features = features
        .as_array()
        .filter(|features| features.iter().all(Value::is_string))
        .ok_or_else(|| invalid("clientFeatures must be an array of strings"))?;
    Ok(features.iter().any(|feature| feature == "option-group"))
}

fn feature_declared(params: &Value, feature: &str) -> bool {
    params
        .get("clientFeatures")
        .and_then(Value::as_array)
        .is_some_and(|features| features.iter().any(|value| value == feature))
}

fn surface_options(mut value: Value, option_group: bool) -> Value {
    if !option_group {
        if let Some(options) = value.get_mut("options").and_then(Value::as_array_mut) {
            options.retain(|option| option.get("group").and_then(Value::as_str) != Some("danger"));
        }
    }
    value
}
fn store_error(error: cingulate_store::Error) -> Refusal {
    Refusal::new(error.code(), error.to_string())
}
fn text<'a>(params: &'a Value, key: &str) -> Result<&'a str, Refusal> {
    params
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(format!("{key} must be a string")))
}
fn integer(params: &Value, key: &str) -> Result<i64, Refusal> {
    params
        .get(key)
        .and_then(Value::as_i64)
        .filter(|v| *v >= 0)
        .ok_or_else(|| invalid(format!("{key} must be a nonnegative integer")))
}
fn operator(stamp: &Stamp) -> Result<(), Refusal> {
    if stamp.operator_surface {
        Ok(())
    } else {
        Err(Refusal::new(
            "elicitation_operator_required",
            "operator surface required",
        ))
    }
}

impl Handler {
    pub fn new(options: Options) -> Self {
        Self {
            inner: Arc::new(Inner {
                access_runtime: StdMutex::new(Arc::new(access::UnavailableAccessRuntime)),
                permissions_runtime: StdMutex::new(Arc::new(
                    permissions::UnavailablePermissionsRuntime,
                )),
                notice_delivery: Mutex::new(()),
                execution_maintenance: Mutex::new(execution::Maintenance::default()),
                options,
                store: Mutex::new(None),
                health: AtomicU8::new(0),
                machine_id: StdMutex::new(None),
                waiters: StdMutex::new(Waiters::default()),
                routes: StdMutex::new(HashMap::new()),
                timer: StdMutex::new(None),
                retry: StdMutex::new(None),
                writes: AtomicU64::new(0),
                changes: watch::channel(()).0,
                stopping: watch::channel(false).0,
                watch_slots: pending_watch::watch_slots(),
            }),
        }
    }

    /// Atomic-only: even a blocked card mutation cannot delay health.
    pub fn health_report(&self) -> HealthReport {
        HealthReport {
            status: match self.inner.health.load(Ordering::Acquire) {
                2 => HealthStatus::Ok,
                1 => HealthStatus::Degraded,
                _ => HealthStatus::Failing,
            },
            detail: None,
            metrics: None,
        }
    }

    pub async fn hello_ack(&self, ack: &ModuleHelloAckBody) {
        *self.inner.machine_id.lock().expect("machine lock") = ack.machine_id.clone();
        let descriptor = match ack
            .storage
            .as_ref()
            .map(|v| serde_json::from_value::<StorageDescriptor>(v.clone()))
            .transpose()
        {
            Ok(descriptor) => {
                storage_descriptor(descriptor.as_ref(), &self.inner.options.data_home)
            }
            Err(error) => {
                tracing::error!(%error, "invalid storage descriptor");
                self.inner.health.store(0, Ordering::Release);
                return;
            }
        };
        // Write the backup catalog before the store opens, so a first start on
        // a fresh machine has it even if the store is briefly held elsewhere.
        engram::bootstrap_catalog(&descriptor);
        match self.try_open(&descriptor).await {
            Ok(()) => {}
            Err(error) if error.is_lease_held() => {
                self.inner.health.store(1, Ordering::Release);
                let weak = Arc::downgrade(&self.inner);
                let clock = self.inner.options.clock.clone();
                let budget = clock.now_ms().saturating_add(10_000);
                let task = tokio::spawn(async move {
                    let mut delay = 50;
                    loop {
                        clock
                            .sleep_until(clock.now_ms().saturating_add(delay).min(budget))
                            .await;
                        let Some(inner) = weak.upgrade() else { return };
                        let handler = Handler { inner };
                        if clock.now_ms() >= budget {
                            handler.inner.health.store(0, Ordering::Release);
                            return;
                        }
                        match handler.try_open(&descriptor).await {
                            Ok(()) => return,
                            Err(error) if error.is_lease_held() => delay = (delay * 2).min(1000),
                            Err(error) => {
                                tracing::error!(%error, "consent store open failed");
                                handler.inner.health.store(0, Ordering::Release);
                                return;
                            }
                        }
                    }
                });
                *self.inner.retry.lock().expect("retry lock") = Some(task.abort_handle());
            }
            Err(error) => {
                tracing::error!(%error, "consent store open failed");
                self.inner.health.store(0, Ordering::Release);
            }
        }
    }

    async fn try_open(&self, descriptor: &StorageDescriptor) -> Result<(), cingulate_store::Error> {
        let (mut store, settlements) =
            ConsentStore::open(descriptor, self.inner.options.clock.now_ms())?;
        store.set_change_log_rows(self.inner.options.change_log_rows);
        self.settlements(&settlements).await;
        self.note_writes(&store);
        *self.inner.store.lock().await = Some(store);
        self.inner.health.store(2, Ordering::Release);
        self.spawn_pending_notices().await;
        if self.inner.options.sweep_timer {
            self.start_timer();
        }
        Ok(())
    }

    fn start_timer(&self) {
        self.stop_timer();
        let weak = Arc::downgrade(&self.inner);
        let clock = self.inner.options.clock.clone();
        let task = tokio::spawn(async move {
            loop {
                clock.sleep_until(clock.now_ms().saturating_add(1000)).await;
                let Some(inner) = weak.upgrade() else { return };
                if let Err(error) = (Handler { inner }).timer_tick().await {
                    tracing::error!(%error, "consent sweep failed");
                }
            }
        });
        *self.inner.timer.lock().expect("timer lock") = Some(task.abort_handle());
    }

    pub fn stop_timer(&self) {
        if let Some(timer) = self.inner.timer.lock().expect("timer lock").take() {
            timer.abort();
        }
    }

    /// The timer and deterministic tests use exactly the same sweep path.
    pub async fn timer_tick(&self) -> Result<(), Refusal> {
        self.available()?;
        let settlements = self
            .with_store(|store| store.sweep(self.inner.options.clock.now_ms()))
            .await?;
        self.settlements(&settlements).await;
        self.execution_maintenance().await?;
        self.access_maintenance().await?;
        Ok(())
    }

    fn available(&self) -> Result<(), Refusal> {
        if self.inner.health.load(Ordering::Acquire) == 2 {
            Ok(())
        } else {
            Err(Refusal::new(
                "consent_unavailable",
                "consent store is not open",
            ))
        }
    }

    async fn with_store<T>(
        &self,
        f: impl FnOnce(&ConsentStore) -> Result<T, cingulate_store::Error>,
    ) -> Result<T, Refusal> {
        let guard = self.inner.store.lock().await;
        let store = guard
            .as_ref()
            .ok_or_else(|| Refusal::new("consent_unavailable", "consent store is not open"))?;
        let result = f(store).map_err(store_error);
        self.note_writes(store);
        result
    }

    fn block_until(&self, id: &str) -> Option<i64> {
        self.inner
            .waiters
            .lock()
            .expect("waiter lock")
            .cards
            .get(id)
            .and_then(|waiters| waiters.values().map(|w| w.deadline).max())
    }
    fn project(&self, record: &ElicitationRecord, phone: bool) -> Value {
        let block = self.block_until(&record.elicitation_id);
        if phone {
            let mut value = record.phone(block);
            if let Some(name) = requester_name(&record.requester) {
                value["requesterName"] = json!(name);
            }
            value
        } else {
            record.requester_wire(block)
        }
    }
    fn wake(&self, id: &str) {
        if let Some(waiters) = self
            .inner
            .waiters
            .lock()
            .expect("waiter lock")
            .cards
            .get(id)
        {
            for waiter in waiters.values() {
                waiter.wake.send_replace(());
            }
        }
    }
    async fn pointer(&self, record: &ElicitationRecord, retraction: bool) {
        if record.request.kind == "rich_ui_approval" {
            return;
        }
        let mut value = json!({"elicitationId": record.elicitation_id,
            "kind": if retraction { "retraction" } else { "elicitation" },
            "eventAtMs": if retraction { record.answered_at_ms.expect("settlement time") } else { record.created_at_ms }});
        if let Some(machine) = self.inner.machine_id.lock().expect("machine lock").clone() {
            value["machineId"] = json!(machine);
        }
        if let Err(error) = self.inner.options.notifier.notify(value).await {
            tracing::warn!(%error, "consent notification failed after commit");
        }
    }
    async fn settlements(&self, settlements: &[ElicitationRecord]) {
        for record in settlements {
            self.wake(&record.elicitation_id);
            self.pointer(record, true).await;
        }
        // Settlement notices share the durable outbox with late answers. Send
        // them off the request path; the maintenance tick retries failures.
        if !settlements.is_empty() {
            self.spawn_pending_notices().await;
        }
    }

    async fn spawn_pending_notices(&self) {
        let notices = {
            let store = self.inner.store.lock().await;
            let Some(store) = store.as_ref() else {
                return;
            };
            store.access_notices(self.inner.options.clock.now_ms())
        };
        // An empty delivery task would keep the handler and its store lease
        // alive after shutdown, despite having no durable work to finish.
        match notices {
            Ok(notices) if !notices.is_empty() => self.spawn_notice_delivery(),
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(%error, "notice outbox read failed; maintenance will retry")
            }
        }
    }

    async fn identity(
        &self,
        stamp: &Stamp,
        request: &ConsentRequest,
    ) -> Result<RequestIdentity, Refusal> {
        if !stamp
            .allowed_kind()
            .is_some_and(|kinds| kinds.contains(&request.kind.as_str()))
        {
            return Err(stamp.refuse());
        }
        if let Some(body) = &request.secret_use {
            let scope = stamp.scope.as_ref().ok_or_else(|| stamp.refuse())?;
            if body
                .statements()?
                .iter()
                .any(|s| s.agent_id != scope.attributes.agent_id)
            {
                return Err(invalid(
                    "secret use agent_id must match the stamped agent_id",
                ));
            }
        }
        let agent_source = if request.kind == "secret_use" {
            SubjectAgent::FromScope
        } else {
            requester_admission(stamp.label())
                .map_or(SubjectAgent::Never, |admission| admission.agent)
        };
        let carrier = agent_source == SubjectAgent::Carried;
        let from_scope = agent_source == SubjectAgent::FromScope;
        let worker_head = if request.kind != "secret_use"
            && (request.kind == "consent" || from_scope)
            && stamp.carried_agent_id.trim().is_empty()
        {
            match &stamp.scope {
                Some(scope)
                    if scope.kind == subc_protocol::scope::ScopeKind::Worker
                        && scope.owner
                            == (Principal::Reserved {
                                module_id: "prefrontal-core".into(),
                            }) =>
                {
                    Some(self.worker_head(scope).await?)
                }
                _ => None,
            }
        } else {
            None
        };
        let stamped_secret_agent = stamp
            .scope
            .as_ref()
            .and_then(|s| s.attributes.agent_id.as_deref())
            .unwrap_or_default();
        let agent_id = match &worker_head {
            Some(head) => head
                .agent_id
                .as_deref()
                .filter(|id| !id.trim().is_empty())
                .ok_or_else(|| {
                    Refusal::new("access_no_head", "stamped parent head has no agent_id")
                })?,
            None if request.kind == "secret_use" => stamped_secret_agent,
            None => &stamp.carried_agent_id,
        };
        if carrier && agent_id.trim().is_empty() {
            return Err(stamp.refuse());
        }
        if let Some(console) = &request.at_console {
            if self.inner.machine_id.lock().expect("machine lock").as_ref() != Some(console) {
                return Err(invalid("at_console must match the daemon's machine_id"));
            }
        }
        let has_agent = carrier
            || worker_head.is_some()
            || (from_scope && !agent_id.trim().is_empty())
            || (request.kind == "secret_use"
                && stamp
                    .scope
                    .as_ref()
                    .is_some_and(|s| s.attributes.agent_id.is_some()));
        // Resolve both display names together so project reads add no serial timeout.
        let (agent, project) = tokio::join!(
            async {
                if has_agent {
                    self.inner
                        .options
                        .resolver
                        .resolve(agent_id)
                        .await
                        .ok()
                        .flatten()
                } else {
                    None
                }
            },
            async {
                let project_id = stamp.scope.as_ref()?.project_id.as_deref()?;
                Some(
                    self.inner
                        .options
                        .resolver
                        .project_name(project_id)
                        .await
                        .unwrap_or_else(|| project_id.to_owned()),
                )
            }
        );
        let mut resolved_name = None;
        let mut subject = if has_agent {
            if let Some(agent) = agent {
                if agent.retired {
                    return Err(Refusal::new("consent_retired", "subject agent is retired"));
                }
                resolved_name = agent.name;
            }
            Subject {
                agent_id: Some(agent_id.to_owned()),
                head_agent_id: agent_id.to_owned(),
                label: resolved_name.clone().unwrap_or_else(|| agent_id.to_owned()),
                via_worker: worker_head.is_some(),
            }
        } else {
            Subject {
                agent_id: None,
                head_agent_id: String::new(),
                via_worker: false,
                label: match stamp.label() {
                    "direct" => "an unverified local process",
                    "reserved:basal" => "operator decision",
                    label => label,
                }
                .into(),
            }
        };
        if let Some(project) = project {
            subject.label = format!("{} in project {project}", subject.label);
            // The store uses this display value for both new cards and dedup refreshes.
            resolved_name = Some(subject.label.clone());
        }
        Ok(RequestIdentity {
            owner: stamp.owner(),
            requester: stamp.label().into(),
            subject,
            resolved_name,
            asker: stamp.scope.as_ref().map(|scope| ScopeIdentity {
                scope_ref: scope.scope_ref.clone(),
                scope_epoch: scope.scope_epoch,
                agent_id: scope.attributes.agent_id.clone(),
            }),
            head: stamp.scope.as_ref().and_then(|scope| {
                // Attribution only: an unconfirmed parent could name the wrong head, so attribute the card to its asker.
                (scope.kind == subc_protocol::scope::ScopeKind::Worker
                    && scope.parent_state == Some(subc_protocol::scope::ParentState::Linked))
                .then_some(scope.parent.as_ref())
                .flatten()
                .filter(|parent| parent.owner == scope.owner)
                .map(|parent| ScopeIdentity {
                    scope_ref: parent.scope_ref.clone(),
                    scope_epoch: parent.scope_epoch,
                    agent_id: None,
                })
            }),
        })
    }

    /// In-process envelope entry point, also used by the production adapter.
    pub async fn handle_stamped(&self, stamp: &Stamp, body: &[u8]) -> HandlerOutcome {
        if body.len() > 1_310_720 {
            return HandlerOutcome::Error {
                code: "elicitation_request_too_large".into(),
                message: "consent envelope exceeds 1310720 bytes".into(),
            };
        }
        #[derive(Deserialize)]
        struct WireRequest {
            method: String,
            #[serde(default)]
            params: Value,
        }
        let reply = match serde_json::from_slice::<WireRequest>(body) {
            Ok(request) => self.dispatch(stamp, &request.method, request.params).await,
            Err(error) => Err(Refusal::new("invalid_request", error.to_string())),
        };
        match reply {
            Ok(reply) => HandlerOutcome::Response(
                serde_json::to_vec(&json!({"result":reply})).expect("wire serialization"),
            ),
            Err(error) => HandlerOutcome::Error {
                code: error.code.into(),
                message: error.message,
            },
        }
    }

    async fn dispatch(&self, stamp: &Stamp, method: &str, params: Value) -> Reply {
        if method == "health.check" {
            return Ok(serde_json::to_value(self.health_report()).expect("health serialization"));
        }
        if !OPERATIONS.iter().any(|(name, _)| *name == method) {
            return Err(Refusal::new(
                "unknown_method",
                format!("unknown method {method}"),
            ));
        }
        if let Some(reply) = self.permissions_dispatch(stamp, method, &params).await {
            return reply;
        }
        self.available()?;
        if let Some(reply) = self.access_dispatch(stamp, method, &params).await {
            return reply;
        }
        let operator_surface = stamp.operator_handles(method, &params);
        match method {
            "consent.answer" | "consent.list_pending" | "consent.watch_pending" => operator(stamp)?,
            "consent.get" if operator_surface => {}
            "consent.outcomes" => {
                if stamp.label() != "reserved:prefrontal-core" {
                    return Err(stamp.refuse());
                }
            }
            _ => stamp.authorize_owner()?,
        }
        if method == WATCH_PENDING {
            // The watch streams, so it is served by handle_watch_stamped. A
            // unary call reaching here is refused only after the same
            // admission as consent.list_pending.
            return Err(invalid(
                "consent.watch_pending is a watch; subscribe to it as a stream",
            ));
        }
        let option_group = if matches!(method, "consent.answer" | "consent.list_pending")
            || (method == "consent.get" && operator_surface)
        {
            option_group_declared(&params)?
        } else {
            false
        };
        if method == "consent.request" {
            if params.get("kind").and_then(Value::as_str) == Some("secret_use")
                && (!stamp.admits_kind("secret_use") || stamp.scope.is_none())
            {
                return Err(stamp.refuse());
            }
            let request = ConsentRequest::decode(params)?;
            let identity = self.identity(stamp, &request).await?;
            let result = self
                .with_store(|store| {
                    store.create(&identity, request, self.inner.options.clock.now_ms())
                })
                .await?;
            self.settlements(&result.settlements).await;
            self.wake(&result.record.elicitation_id);
            if result.created {
                if stamp.scope.is_none() && result.record.request.at_console.is_none() {
                    tracing::info!(elicitation_id=%result.record.elicitation_id, principal=%stamp.label(), "person card filed without an attested asker; no core notice queued");
                }
                self.pointer(&result.record, false).await;
                self.spawn_pending_notices().await;
            }
            return Ok(json!({"elicitation_id": result.record.elicitation_id}));
        }
        let id = match method {
            "consent.get" if operator_surface => Some(text(&params, "elicitationId")?),
            "consent.answer" | "consent.revoke" => Some(text(&params, "elicitationId")?),
            "consent.get"
            | "consent.await"
            | "consent.withdraw"
            | "consent.detach"
            | "consent.report_execution" => Some(text(&params, "elicitation_id")?),
            _ => None,
        };
        let choice = if method == "consent.answer" {
            Some(text(&params, "choiceId")?)
        } else {
            None
        };
        let cursor = match method {
            "consent.answers" | "consent.outcomes" => integer(&params, "since")?,
            "consent.ack" => integer(&params, "cursor")?,
            "consent.await" => integer(&params, "timeout_ms")?.min(120_000),
            _ => 0,
        };
        // A nonexistent owner's id must not expire unrelated cards or leak
        // whether another owner has it, even when supplied params are malformed.
        if matches!(
            method,
            "consent.get"
                | "consent.await"
                | "consent.withdraw"
                | "consent.revoke"
                | "consent.detach"
                | "consent.report_execution"
        ) && !(method == "consent.get" && operator_surface)
        {
            let owner = self
                .with_store(|store| store.owner(id.expect("owner id")))
                .await?;
            if owner.as_deref() != Some(stamp.owner().as_str()) {
                return Err(Refusal::new(
                    "elicitation_not_owner",
                    "elicitation has no matching owner",
                ));
            }
        }
        if method == "consent.await" {
            return self
                .await_card(&stamp.owner(), id.expect("await id"), cursor)
                .await;
        }
        let execution = if method == "consent.report_execution" {
            Some(text(&params, "execution").map_err(|error| {
                if params.get("reason").is_some() {
                    Refusal::new(
                        "elicitation_invalid_execution",
                        "reason requires execution approved_not_executed",
                    )
                } else {
                    error
                }
            })?)
        } else {
            None
        };
        let reason = if method == "consent.report_execution" {
            params
                .get("reason")
                .map(|value| {
                    value.as_str().ok_or_else(|| {
                        Refusal::new(
                            "elicitation_invalid_execution",
                            "reason must be a lowercase snake_case code of 1 to 64 bytes",
                        )
                    })
                })
                .transpose()?
        } else {
            None
        };
        let mut access_labels = HashMap::new();
        if method == "consent.list_pending" {
            // Resolve display names without holding the store lock, using the
            // same agent resolver as generic worker consent subjects.
            for record in self.with_store(|store| store.access_pending()).await? {
                if record.state == "pending:user" {
                    access_labels.insert(
                        record.elicitation_id.clone(),
                        self.access_phone_label(&record).await,
                    );
                }
            }
        }
        let guard = self.inner.store.lock().await;
        let store = guard
            .as_ref()
            .ok_or_else(|| Refusal::new("consent_unavailable", "consent store is not open"))?;
        let now = self.inner.options.clock.now_ms();
        let rich_ui = feature_declared(&params, "rich-ui-approval");
        let secret_use = feature_declared(&params, "secret-use-approval");
        if method == "consent.answer" || (method == "consent.get" && operator_surface) {
            if let Some(record) = store
                .get_phone(id.expect("phone id"), now)
                .map_err(store_error)?
            {
                if !stamp.rich_ui_device_matches(&record) {
                    return if method == "consent.answer" {
                        Ok(json!({"ok":false,"code":"not_found"}))
                    } else {
                        Err(Refusal::new(
                            "elicitation_not_found",
                            "elicitation not found",
                        ))
                    };
                }
                if record.request.is_secret_card()
                    && (!secret_use || stamp.secret_use_phone().is_none())
                {
                    return Err(Refusal::new(
                        if method == "consent.answer" {
                            "elicitation_secret_use_approval_required"
                        } else {
                            "elicitation_not_found"
                        },
                        if method == "consent.answer" {
                            "secret use requires a phone with clientFeatures secret-use-approval"
                        } else {
                            "elicitation not found"
                        },
                    ));
                }
                if record.request.kind == "rich_ui_approval" && !rich_ui {
                    return if method == "consent.answer" {
                        Err(Refusal::new(
                            "elicitation_rich_ui_approval_required",
                            "declare clientFeatures rich-ui-approval",
                        ))
                    } else {
                        Err(Refusal::new(
                            "elicitation_not_found",
                            "elicitation not found",
                        ))
                    };
                }
                if method == "consent.answer" {
                    if let Some(body) = &record.request.rich_ui_approval {
                        let binding = params.get("bindDigest");
                        let grant = record.request.options.iter().any(|option| {
                            Some(option.id.as_str()) == choice && option.effect == "grant"
                        });
                        if (grant || binding.is_some())
                            && binding.and_then(Value::as_str) != Some(&body.approval_digest)
                        {
                            return Err(Refusal::new(
                                "approval_digest_mismatch",
                                "bindDigest must equal approvalDigest",
                            ));
                        }
                    } else if params.get("bindDigest").is_some() {
                        return Err(Refusal::new(
                            "invalid_params",
                            "bindDigest is only valid for rich_ui_approval",
                        ));
                    }
                }
            }
        }
        if method == "consent.answer" && !option_group {
            if let Some(record) = store
                .get(id.expect("answer id"), now)
                .map_err(store_error)?
            {
                let choice = choice.expect("choice");
                if record
                    .request
                    .options
                    .iter()
                    .any(|option| option.id == choice && option.group == Some(OptionGroup::Danger))
                {
                    return Err(Refusal::new(
                        "elicitation_option_group_required",
                        format!("choice {choice} is in the danger option group; declare clientFeatures option-group"),
                    ));
                }
            }
        }
        let mut events = if matches!(
            method,
            "consent.get"
                | "consent.answers"
                | "consent.outcomes"
                | "consent.list_pending"
                | "consent.answer"
                | "consent.withdraw"
                | "consent.revoke"
                | "consent.detach"
        ) {
            store.sweep(now).map_err(store_error)?
        } else {
            vec![]
        };
        let result: Reply = (|| {
            Ok(match method {
                "consent.get" if operator_surface => {
                    let record = store
                        .get_phone(id.expect("get id"), now)
                        .map_err(store_error)?
                        .ok_or_else(|| {
                            Refusal::new("elicitation_not_found", "elicitation not found")
                        })?;
                    surface_options(self.project(&record, true), option_group)
                }
                "consent.get" => self.project(
                    &store
                        .get_owned(&stamp.owner(), id.expect("get id"), now)
                        .map_err(store_error)?,
                    false,
                ),
                "consent.answers" => serde_json::to_value(
                    store
                        .answers(&stamp.owner(), cursor, now)
                        .map_err(store_error)?,
                )
                .expect("feed serialization"),
                "consent.outcomes" => {
                    serde_json::to_value(store.outcomes(cursor, now).map_err(store_error)?)
                        .expect("feed serialization")
                }
                "consent.ack" => {
                    store.ack(&stamp.owner(), cursor).map_err(store_error)?;
                    json!({"ok":true})
                }
                "consent.report_execution" => self.project(
                    &store
                        .report_execution(
                            &stamp.owner(),
                            id.expect("report id"),
                            execution.expect("execution"),
                            reason,
                            now,
                        )
                        .map_err(store_error)?,
                    false,
                ),
                "consent.withdraw" => {
                    let mutation = store
                        .withdraw(&stamp.owner(), id.expect("withdraw id"), now)
                        .map_err(store_error)?;
                    events.extend(mutation.settlements);
                    self.project(&mutation.value, false)
                }
                "consent.revoke" => {
                    let mutation = store
                        .revoke(&stamp.owner(), id.expect("revoke id"), now)
                        .map_err(store_error)?;
                    events.extend(mutation.settlements);
                    self.project(&mutation.value, false)
                }
                "consent.detach" => {
                    let mutation = store
                        .detach(&stamp.owner(), id.expect("detach id"), now)
                        .map_err(store_error)?;
                    events.extend(mutation.settlements);
                    let mut value = self.project(&mutation.value, false);
                    // Only the consent.detach reply carries `detached: true`; get,
                    // await, answers, withdraw and outcomes never show it. It is
                    // absent when the card settled before it was ever detached,
                    // which means no late-answer notice will follow.
                    if mutation.value.detached {
                        value["detached"] = json!(true);
                    }
                    value
                }
                "consent.list_pending" => {
                    let after = params
                        .get("continuation")
                        .filter(|value| !value.is_null())
                        .map(|value| -> Result<(i64, String), Refusal> {
                            let id = text(value, "elicitation_id")?.to_owned();
                            let created = if value.get("created_at_ms").is_some() {
                                integer(value, "created_at_ms")?
                            } else {
                                // Accept the previous release's expiry cursor, but
                                // resume at that card's position in creation order.
                                integer(value, "expires_at_ms")?;
                                if let Some(record) = store.get_phone(&id, now).map_err(store_error)?.filter(|record| {
                                    stamp.card_visible(record, &params)
                                }) {
                                    record.created_at_ms
                                } else if let Some(record) = store.access_get(&id).map_err(store_error)? {
                                    record.created_at_ms
                                } else {
                                    return Err(invalid("legacy continuation card is no longer retained; restart the list"));
                                }
                            };
                            Ok((created, id))
                        })
                        .transpose()?;
                    // Snapshot deadlines once so the revision and every rendered
                    // blockUntilMs describe the same waiter state.
                    let blocks: std::collections::BTreeMap<_, _> = self
                        .inner
                        .waiters
                        .lock()
                        .expect("waiter lock")
                        .cards
                        .iter()
                        .filter_map(|(id, waiters)| {
                            waiters
                                .values()
                                .map(|w| w.deadline)
                                .max()
                                .map(|until| (id.clone(), until))
                        })
                        .collect();
                    let mut revision = format!(
                        "pending-v1:{}:{:x}",
                        store.pending_revision().map_err(store_error)?,
                        Sha256::digest(
                            serde_json::to_vec(&blocks).expect("deadline serialization")
                        )
                    );
                    // Switching option-group support changes the visible options,
                    // so a cached list with danger options hidden cannot stand in
                    // for a fresh list that includes them (or vice versa).
                    if option_group {
                        revision.push_str(":option-group");
                    }
                    if rich_ui {
                        revision.push_str(&format!(
                            ":rich-ui:device={}",
                            stamp.answering_device().unwrap_or("none")
                        ));
                    }
                    if secret_use && stamp.secret_use_phone().is_some() {
                        revision.push_str(":secret-use");
                    }
                    if after.is_none()
                        && params.get("if_revision").and_then(Value::as_str) == Some(&revision)
                    {
                        return Ok(json!({"unchanged":true,"revision":revision}));
                    }
                    let mut records = Vec::new();
                    let mut bytes = 0;
                    let mut last = None;
                    let mut continuation = None;
                    let mut cards: Vec<_> = store
                        .pending()
                        .map_err(store_error)?
                        .into_iter()
                        .filter(|record| stamp.card_visible(record, &params))
                        .map(|record| {
                            let key = (record.created_at_ms, record.elicitation_id.clone());
                            let mut value =
                                record.phone(blocks.get(&record.elicitation_id).copied());
                            if let Some(name) = requester_name(&record.requester) {
                                value["requesterName"] = json!(name);
                            }
                            (key, surface_options(value, option_group))
                        })
                        .collect();
                    for record in store.access_pending().map_err(store_error)? {
                        if record.state == "pending:user" {
                            // Permission asks share the same creation-order cursor
                            // as generic cards; neither kind needs an expiry to sort.
                            let mut value =
                                record.wire(true, blocks.get(&record.elicitation_id).copied());
                            if let Some(label) = access_labels.get(&record.elicitation_id) {
                                value["subject"]["label"] = json!(label);
                            }
                            if let Some(name) = requester_name(&record.identity.principal) {
                                value["requesterName"] = json!(name);
                            }
                            cards.push((
                                (record.created_at_ms, record.elicitation_id.clone()),
                                value,
                            ));
                        }
                    }
                    cards.sort_by(|a, b| a.0.cmp(&b.0));
                    for (key, mut value) in cards {
                        if after.as_ref().is_some_and(|after| &key <= after) {
                            continue;
                        }
                        cingulate_store::omit_package_bodies(&mut value);
                        let size = serde_json::to_vec(&value)
                            .expect("projection serialization")
                            .len()
                            + 1;
                        if bytes + size > cingulate_store::PAGE_RECORD_BYTES {
                            if records.is_empty() {
                                return Err(invalid("card projection exceeds page budget"));
                            }
                            continuation = last;
                            break;
                        }
                        records.push(value);
                        bytes += size;
                        last = Some(json!({"created_at_ms":key.0,"elicitation_id":key.1}));
                    }
                    let mut reply = json!({"records":records,"revision":revision});
                    if let Some(continuation) = continuation {
                        reply["continuation"] = continuation;
                    }
                    reply
                }
                "consent.answer" => {
                    let mutation = store
                        .answer_with_envelope(
                            id.expect("answer id"),
                            choice.expect("choice"),
                            stamp.console_machine_id.as_deref(),
                            stamp.answering_device(),
                            params
                                .get("envelope_b64url")
                                .map(|v| {
                                    v.as_str()
                                        .ok_or_else(|| invalid("envelope_b64url must be a string"))
                                })
                                .transpose()?,
                            now,
                        )
                        .map_err(store_error)?;
                    events.extend(mutation.settlements);
                    match mutation.value {
                        AnswerOutcome::NotFound => json!({"ok":false,"code":"not_found"}),
                        AnswerOutcome::Conflict(record) => {
                            json!({"ok":false,"code":"conflict","elicitation":surface_options(self.project(&record, true), option_group)})
                        }
                        AnswerOutcome::Answered(record) => {
                            json!({"ok":true,"alreadyAnswered":false,"elicitation":surface_options(self.project(&record, true), option_group)})
                        }
                        AnswerOutcome::AlreadyAnswered(record) => {
                            json!({"ok":true,"alreadyAnswered":true,"elicitation":surface_options(self.project(&record, true), option_group)})
                        }
                    }
                }
                _ => unreachable!("known operation"),
            })
        })();
        self.note_writes(store);
        drop(guard);
        // A read/answer refusal can follow an independently committed sweep.
        // Its retractions must still be delivered before returning that refusal.
        self.settlements(&events).await;
        result
    }

    async fn await_card(&self, owner: &str, id: &str, timeout: i64) -> Reply {
        let deadline = self.inner.options.clock.now_ms().saturating_add(timeout);
        let (sender, mut receiver) = watch::channel(());
        let registration = {
            let mut waiters = self.inner.waiters.lock().expect("waiter lock");
            let token = waiters.next;
            waiters.next = token.checked_add(1).expect("waiter token exhaustion");
            waiters.cards.entry(id.into()).or_default().insert(
                token,
                Waiter {
                    deadline,
                    wake: sender,
                },
            );
            Registration {
                inner: self.inner.clone(),
                id: id.into(),
                token,
            }
        };
        if let Some(hook) = &self.inner.options.await_hook {
            hook.after_register(self, id).await;
        }
        loop {
            // Mark old wakes before reading, not after: a commit between this
            // read and sleeping must remain observable on the watch channel.
            receiver.borrow_and_update();
            let guard = self.inner.store.lock().await;
            let store = guard
                .as_ref()
                .ok_or_else(|| Refusal::new("consent_unavailable", "consent store is not open"))?;
            let now = self.inner.options.clock.now_ms();
            let events = store.sweep(now).map_err(store_error)?;
            let result = store.get_owned(owner, id, now).map_err(store_error);
            self.note_writes(store);
            drop(guard);
            self.settlements(&events).await;
            let record = result?;
            let until = record
                .expires_at_ms
                .map_or(deadline, |expiry| deadline.min(expiry));
            if record.state != State::Pending || now >= until {
                drop(registration);
                return Ok(self.project(&record, false));
            }
            tokio::select! {
                _ = receiver.changed() => {},
                _ = self.inner.options.clock.sleep_until(until) => {},
            }
        }
    }
}

#[async_trait]
impl ModuleHandler for Handler {
    async fn handle(&self, ctx: RequestCtx, body: Vec<u8>) -> HandlerOutcome {
        let stamp = self
            .inner
            .routes
            .lock()
            .expect("route lock")
            .get(&ctx.route_handle())
            .cloned()
            .unwrap_or_default();
        #[derive(Deserialize)]
        struct Method {
            method: String,
        }
        if serde_json::from_slice::<Method>(&body)
            .is_ok_and(|request| request.method == WATCH_PENDING)
        {
            // The watch watches ctx.cancelled() itself and ends with Streamed.
            return self.handle_watch_stamped(&stamp, &body, &ctx).await;
        }
        tokio::select! {
            result = self.handle_stamped(&stamp, &body) => result,
            _ = ctx.cancelled() => HandlerOutcome::Error { code: "cancelled".into(), message: "request cancelled".into() },
        }
    }
    async fn on_hello_ack(&self, ack: &ModuleHelloAckBody) {
        self.hello_ack(ack).await;
    }
    async fn on_bind(&self, req: &RouteBindRequest) -> BindDecision {
        self.inner
            .routes
            .lock()
            .expect("route lock")
            .insert(req.handle, Stamp::from_bind(req));
        BindDecision::accept()
    }
    async fn on_route_gone(&self, handle: &RouteHandle) {
        self.inner.routes.lock().expect("route lock").remove(handle);
    }
    async fn health(&self) -> HealthReport {
        self.health_report()
    }
    async fn on_draining(&self, _reason: RouteCloseReason, _deadline: SystemTime) {
        self.stop_watches();
    }
    async fn on_connection_end(&self, _end: ConnectionEnd) {
        self.stop_watches();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use subc_protocol::{BindIdentity, RouteTarget};

    #[test]
    fn every_admitted_requester_has_its_phone_display_name() {
        let admissions: Vec<_> = REQUESTER_ADMISSIONS
            .iter()
            .map(|admission| {
                (
                    admission.principal,
                    admission.requester_name,
                    admission.kinds.to_vec(),
                )
            })
            .collect();
        assert_eq!(
            admissions,
            [
                (
                    "reserved:occipital",
                    Some("Occipital"),
                    vec!["rich_ui_approval"]
                ),
                (
                    "reserved:cerebellum",
                    Some("Cerebellum"),
                    vec!["consent", "secret_use"]
                ),
                ("reserved:plexus", Some("Plexus"), vec!["consent"]),
                ("reserved:broca", Some("Broca"), vec!["consent"]),
                ("reserved:entorhinal", Some("Entorhinal"), vec!["consent"]),
                ("direct", None, vec!["consent"]),
                ("reserved:basal", Some("Basal"), vec!["flow_decision"]),
                (
                    "reserved:prefrontal-core",
                    Some("Prefrontal"),
                    vec!["flow_install", "package"]
                ),
                (
                    "reserved:aft",
                    Some("AFT"),
                    vec!["tool_permission", "macos_permission", "secret_use"]
                ),
            ]
        );
        for admission in REQUESTER_ADMISSIONS {
            if admission.principal == "direct" {
                assert_eq!(requester_name(admission.principal), None);
                continue;
            }
            assert!(
                requester_name(admission.principal).is_some(),
                "admitted principal {} lacks a phone display name",
                admission.principal
            );
        }
        assert_eq!(requester_name("unexpected"), None);
    }

    #[test]
    fn generic_consent_requesters_match_the_contract_operation_list() {
        let contract = include_str!("../../../docs/contracts/consent-requester-v2.md");
        let paragraph = contract
            .split("\n\n")
            .find(|p| p.contains("may call `consent.request`"))
            .unwrap();
        let documented: Vec<_> = paragraph
            .split("may call")
            .next()
            .unwrap()
            .split('`')
            .filter(|name| name.starts_with("reserved:"))
            .collect();
        let implemented: Vec<_> = REQUESTER_ADMISSIONS
            .iter()
            .filter(|admission| {
                admission.kinds.contains(&"consent") && admission.principal != "direct"
            })
            .map(|admission| admission.principal)
            .collect();
        assert_eq!(
            documented,
            [
                "reserved:cerebellum",
                "reserved:plexus",
                "reserved:broca",
                "reserved:entorhinal"
            ]
        );
        assert_eq!(implemented, documented);
        for name in implemented {
            // Cerebellum also files secret_use cards for the vault's site
            // logins; every other generic requester files consent cards only.
            let expected: &[&str] = if name == "reserved:cerebellum" {
                &["consent", "secret_use"]
            } else {
                &["consent"]
            };
            assert_eq!(
                Stamp {
                    principal: Some(name.into()),
                    ..Default::default()
                }
                .allowed_kind(),
                Some(expected)
            );
        }
    }

    fn bind(principal: &str, harness: &str) -> RouteBindRequest {
        RouteBindRequest::new(
            RouteHandle::detached(1, 1),
            RouteTarget::ManagementSurface {
                module_id: "cingulate".into(),
            },
            BindIdentity::new("/scratch", harness, "untrusted-session"),
        )
        .with_principal(Principal::Reserved {
            module_id: principal.into(),
        })
    }

    #[test]
    fn development_local_operator_bind_switch_preserves_phone_boundaries() {
        let phone_harness = "fed-phone:0123456789abcdef0123456789abcdef";
        for harness in ["local-desktop", phone_harness, "fed-sim:ignored"] {
            let req = bind("unused", harness).with_principal(Principal::Direct);
            for enabled in [false, true] {
                let stamp = Stamp::from_bind_with_local_operator(&req, enabled);
                assert_eq!(stamp.operator_surface, enabled);
                assert_eq!(stamp.local_operator, enabled);
                assert_eq!(operator(&stamp).is_ok(), enabled);
                assert!(!stamp.sim_surface);
                assert_eq!(stamp.answering_device(), None);
                assert_eq!(stamp.secret_use_phone(), None);
            }
        }
        for req in [
            bind("plexus", phone_harness),
            bind("unused", phone_harness).with_principal(Principal::Unverified),
            {
                let mut req = bind("unused", phone_harness);
                req.principal = None;
                req
            },
            bind("callosum", "fed-sim:0123456789abcdef0123456789abcdef"),
        ] {
            for enabled in [false, true] {
                let stamp = Stamp::from_bind_with_local_operator(&req, enabled);
                assert!(!stamp.operator_surface);
                assert!(!stamp.local_operator);
                assert_eq!(
                    stamp.sim_surface,
                    stamp.label() == "reserved:callosum" && stamp.harness.starts_with("fed-sim:")
                );
            }
        }
        for enabled in [false, true] {
            let stamp =
                Stamp::from_bind_with_local_operator(&bind("callosum", phone_harness), enabled);
            assert!(stamp.operator_surface);
            assert!(!stamp.local_operator);
            assert!(!stamp.sim_surface);
            assert_eq!(
                stamp.secret_use_phone(),
                Some("0123456789abcdef0123456789abcdef")
            );
        }
    }

    #[tokio::test]
    async fn production_bind_uses_attested_scope_and_agent() {
        let handler = Handler::new(Options {
            sweep_timer: false,
            ..Options::default()
        });
        let scope = serde_json::from_value(json!({
            "owner":{"kind":"reserved","module_id":"prefrontal-core"},
            "ref":"head:alf", "scope_epoch":7, "kind":"head",
            "attributes":{"agent_id":"agent-alf"}, "owner_authorized":true,
            "project_id":"pj-cingulate"
        }))
        .unwrap();
        let req = bind("plexus", "fed-mac:ignored").with_scope(scope);
        handler.on_bind(&req).await;
        let stamp = handler.inner.routes.lock().unwrap()[&req.handle].clone();
        assert_eq!(
            stamp.bind_session,
            json!([{"kind":"reserved","module_id":"prefrontal-core"}, "head:alf", 7]).to_string()
        );
        assert_eq!(stamp.carried_agent_id, "agent-alf");
        assert_eq!(
            stamp.scope.as_ref().unwrap().project_id.as_deref(),
            Some("pj-cingulate")
        );
        assert!(stamp.console_machine_id.is_none());
        let mut changed = req.clone();
        changed.scope.as_mut().unwrap().scope_epoch += 1;
        assert_ne!(Stamp::from_bind(&changed).owner(), stamp.owner());
        changed = req.clone();
        changed.scope.as_mut().unwrap().scope_ref.push('x');
        assert_ne!(Stamp::from_bind(&changed).owner(), stamp.owner());
        changed = req.clone();
        changed.scope.as_mut().unwrap().owner = Principal::Direct;
        assert_ne!(Stamp::from_bind(&changed).owner(), stamp.owner());
        changed = req.clone();
        changed.identity = BindIdentity::new("/different", "other", "forged-session");
        assert_eq!(Stamp::from_bind(&changed).owner(), stamp.owner());
        handler.on_route_gone(&req.handle).await;
        assert!(!handler
            .inner
            .routes
            .lock()
            .unwrap()
            .contains_key(&req.handle));
    }

    #[tokio::test]
    async fn production_bind_without_scope_keeps_identity_empty() {
        let handler = Handler::new(Options {
            sweep_timer: false,
            ..Options::default()
        });
        let req = bind("plexus", "fed-phone:0123456789abcdef0123456789abcdef");
        handler.on_bind(&req).await;
        let stamp = handler.inner.routes.lock().unwrap()[&req.handle].clone();
        assert!(stamp.bind_session.is_empty());
        assert!(stamp.carried_agent_id.is_empty());
        assert!(stamp.console_machine_id.is_none());
    }

    macro_rules! phone_case {
        ($name:ident, $principal:expr, $harness:expr, $allowed:expr) => {
            #[tokio::test]
            async fn $name() {
                let handler = Handler::new(Options {
                    sweep_timer: false,
                    ..Options::default()
                });
                let req = bind($principal, $harness);
                handler.on_bind(&req).await;
                let stamp = handler.inner.routes.lock().unwrap()[&req.handle].clone();
                assert_eq!(stamp.operator_surface, $allowed);
                assert_eq!(operator(&stamp).is_ok(), $allowed);
                let dir = tempfile::tempdir().unwrap();
                let descriptor = storage_descriptor(None, dir.path().to_str().unwrap());
                handler.hello_ack(&ModuleHelloAckBody {
                    negotiated_ver: 1,
                    subc_ops: vec![],
                    subc_capabilities: vec![],
                    storage: Some(serde_json::to_value(descriptor).unwrap()),
                    machine_id: None,
                }).await;
                let request: Value = serde_json::from_str(include_str!("../../../test-vectors/elicitation-request-v1/valid/app-grant-once.json")).unwrap();
                let requester = Stamp::from_principal(Some(&Principal::Direct));
                let reply = handler.dispatch(&requester, "consent.request", request).await.unwrap();
                let id = reply["elicitation_id"].as_str().unwrap();
                let answer = handler.dispatch(&stamp, "consent.answer", json!({"elicitationId":id,"choiceId":"allow_once"})).await;
                let list = handler.dispatch(&stamp, "consent.list_pending", json!({})).await;
                if $allowed {
                    assert_eq!(answer.unwrap()["ok"], true);
                    assert!(list.is_ok());
                } else {
                    assert_eq!(answer.unwrap_err().code, "elicitation_operator_required");
                    assert_eq!(list.unwrap_err().code, "elicitation_operator_required");
                }
                let record = handler.dispatch(&requester, "consent.get", json!({"elicitation_id":id})).await.unwrap();
                assert_eq!(record["state"], if $allowed { "answered" } else { "pending" });
            }
        };
    }
    phone_case!(
        phone_callosum_is_operator,
        "callosum",
        "fed-phone:0123456789abcdef0123456789abcdef",
        true
    );
    phone_case!(
        unknown_callosum_is_not_operator,
        "callosum",
        "fed-unknown:0123456789abcdef0123456789abcdef",
        false
    );
    phone_case!(
        sim_callosum_is_not_operator,
        "callosum",
        "fed-sim:0123456789abcdef0123456789abcdef",
        false
    );
    phone_case!(
        mac_callosum_is_not_operator,
        "callosum",
        "fed-mac:0123456789abcdef0123456789abcdef",
        false
    );
    phone_case!(
        computer_callosum_is_not_operator,
        "callosum",
        "fed-computer:0123456789abcdef0123456789abcdef",
        false
    );
    phone_case!(empty_harness_is_not_operator, "callosum", "", false);
    phone_case!(
        phone_other_principal_is_not_operator,
        "plexus",
        "fed-phone:0123456789abcdef0123456789abcdef",
        false
    );
    phone_case!(
        phone_literal_prefix_is_sufficient,
        "callosum",
        "fed-phone:",
        true
    );
    phone_case!(
        phone_prefix_is_case_sensitive,
        "callosum",
        "Fed-phone:0123456789abcdef0123456789abcdef",
        false
    );

    /// Callosum publishes the exact harness strings it stamps for each paired
    /// device class. Pinning them here means a change to Callosum's format turns
    /// this test red instead of silently locking the phone out, or letting
    /// another device class answer cards.
    #[test]
    fn callosum_harness_vectors_mark_only_the_phone_as_operator() {
        use sha2::{Digest, Sha256};
        let bytes = include_bytes!("../../../test-vectors/device-class-harness/harness.jsonl");
        assert_eq!(
            format!("{:x}", Sha256::digest(bytes)),
            "da9f5e4d09bcae2dc70045bf4e629b215a1407d8e13eb26275639568488d892c"
        );
        let mut operators = Vec::new();
        let mut rows = 0;
        for line in std::str::from_utf8(bytes).unwrap().lines() {
            let row: Value = serde_json::from_str(line).unwrap();
            let harness = row["harness"].as_str().unwrap();
            rows += 1;
            if Stamp::from_bind(&bind("callosum", harness)).operator_surface {
                operators.push(row["class"].clone());
            }
            assert!(!Stamp::from_bind(&bind("plexus", harness)).operator_surface);
        }
        assert_eq!(rows, 4);
        assert_eq!(operators, vec![json!("phone")]);
    }

    #[test]
    fn absent_harness_is_rejected_by_sdk_identity_decoder() {
        assert!(serde_json::from_value::<BindIdentity>(
            json!({"project_root":"/scratch", "session":"s"})
        )
        .is_err());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn health_does_not_acquire_store_lock() {
        let dir = tempfile::tempdir().expect("scratch directory");
        let descriptor = storage_descriptor(None, dir.path().to_str().expect("scratch path"));
        let handler = Handler::new(Options {
            sweep_timer: false,
            ..Options::default()
        });
        handler
            .hello_ack(&ModuleHelloAckBody {
                negotiated_ver: 1,
                subc_ops: vec![],
                subc_capabilities: vec![],
                storage: Some(serde_json::to_value(&descriptor).expect("descriptor")),
                machine_id: None,
            })
            .await;
        assert_eq!(handler.health_report().status, HealthStatus::Ok);
        let cingulate_store::StorageBackend::Sqlite { path } = descriptor.backend else {
            panic!("SQLite descriptor")
        };
        let blocker = rusqlite::Connection::open(path).expect("scratch store");
        blocker
            .execute_batch("BEGIN IMMEDIATE")
            .expect("hold writer lock");
        let request_handler = handler.clone();
        let request = tokio::spawn(async move {
            let body: Value = serde_json::from_str(include_str!(
                "../../../test-vectors/elicitation-request-v1/valid/app-grant-once.json"
            ))
            .expect("request fixture");
            request_handler
                .handle_stamped(
                    &Stamp {
                        principal: Some("direct".into()),
                        ..Stamp::default()
                    },
                    &serde_json::to_vec(&json!({"method":"consent.request","params":body}))
                        .expect("envelope"),
                )
                .await
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            while handler.inner.store.try_lock().is_ok() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("card op acquired store lock");
        assert!(!request.is_finished());
        let outcome = tokio::time::timeout(
            Duration::from_secs(1),
            handler.handle_stamped(&Stamp::default(), br#"{"method":"health.check"}"#),
        )
        .await
        .expect("lock-free health");
        assert_eq!(
            outcome,
            HandlerOutcome::Response(br#"{"result":{"status":"ok"}}"#.to_vec())
        );
        blocker
            .execute_batch("COMMIT")
            .expect("release writer lock");
        assert!(matches!(
            request.await.expect("request task"),
            HandlerOutcome::Response(_)
        ));
    }
}

#[cfg(test)]
mod rich_ui_device_tests {
    use super::*;

    #[test]
    fn fed_fingerprint_parser_matches_callosum_vectors() {
        let corpus = include_str!("../../../test-vectors/device-class-harness/harness.jsonl");
        for line in corpus.lines() {
            let vector: Value = serde_json::from_str(line).unwrap();
            let stamp = Stamp {
                principal: Some("reserved:callosum".into()),
                harness: vector["harness"].as_str().unwrap().into(),
                ..Stamp::default()
            };
            assert_eq!(
                stamp.answering_device().is_some(),
                !vector["class"].is_null()
            );
        }
        for harness in [
            "fed-phone:",
            "fed-phone:300C9c9603b92a4b39ed3958bf924011",
            "fed-phone:300c9c9603b92a4b39ed3958bf924011:extra",
            "fed-phone:gggggggggggggggggggggggggggggggg",
            "phone:300c9c9603b92a4b39ed3958bf924011",
        ] {
            let stamp = Stamp {
                principal: Some("reserved:callosum".into()),
                harness: harness.into(),
                ..Stamp::default()
            };
            assert_eq!(stamp.answering_device(), None, "{harness}");
        }
        let stamp = Stamp {
            principal: Some("reserved:occipital".into()),
            harness: "fed-phone:300c9c9603b92a4b39ed3958bf924011".into(),
            ..Stamp::default()
        };
        assert_eq!(stamp.answering_device(), None);
    }

    /// A computer answers as its fingerprint under both the `computer` class and
    /// the older `mac` spelling, so a card bound to it stays answerable across
    /// Callosum's rename; an unknown class still yields no device.
    #[test]
    fn the_computer_class_answers_under_both_spellings() {
        let fingerprint = "300c9c9603b92a4b39ed3958bf924011";
        for class in ["computer", "mac"] {
            let stamp = Stamp {
                principal: Some("reserved:callosum".into()),
                harness: format!("fed-{class}:{fingerprint}"),
                ..Stamp::default()
            };
            assert_eq!(stamp.answering_device(), Some(fingerprint), "{class}");
        }
        let stamp = Stamp {
            principal: Some("reserved:callosum".into()),
            harness: format!("fed-desktop:{fingerprint}"),
            ..Stamp::default()
        };
        assert_eq!(stamp.answering_device(), None);
    }
}
