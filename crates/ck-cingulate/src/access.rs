//! Daemon scope liveness is independent of route connectivity: a reconnecting
//! carrier must not make a live head look gone. Notices use a durable outbox and
//! run off the request path so core outages never delay committing an answer.
use super::*;
use cingulate_decode::{validate_note, AccessRequest};
use cingulate_store::{AccessAnswer, AccessIdentity, AccessMutation, AnswerAttempt, ScopeIdentity};
use subc_client_rs::{
    consumer::{CallError, CallOptions, ConsumerOptions, SubcConsumer},
    ModuleHandle,
};
use subc_protocol::{
    scope::{ScopeKind, ScopeStatus},
    BindIdentity, RouteTarget,
};

#[derive(Debug, Clone)]
pub enum ScopeLife {
    Live(ScopeIdentity),
    Gone,
    Unknown,
}
#[derive(Debug, Clone)]
pub enum NoticeResult {
    Delivered,
    Refused(String),
    Transport(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RelayIdentity {
    agent_id: String,
    scope_ref: String,
    scope_epoch: u64,
}
#[async_trait]
pub trait AccessRuntime: Send + Sync {
    async fn describe(&self, scope: &ScopeIdentity) -> Result<ScopeLife, String>;
    async fn notice(&self, body: Value) -> NoticeResult;
    /// Send a body already serialized as JSON. The delivery loop calls this
    /// with the queued bytes so pinned key order reaches core unchanged; the
    /// default parses them and calls `notice`.
    async fn notice_json(&self, body: &str) -> NoticeResult {
        match serde_json::from_str(body) {
            Ok(body) => self.notice(body).await,
            Err(_) => NoticeResult::Refused("invalid_params".into()),
        }
    }
    fn is_macos(&self) -> bool {
        cfg!(target_os = "macos")
    }
}
pub(super) struct UnavailableAccessRuntime;
#[async_trait]
impl AccessRuntime for UnavailableAccessRuntime {
    async fn describe(&self, _: &ScopeIdentity) -> Result<ScopeLife, String> {
        Err("daemon scope.describe unavailable".into())
    }
    async fn notice(&self, _: Value) -> NoticeResult {
        NoticeResult::Transport("core unavailable".into())
    }
}
pub struct DaemonAccessRuntime {
    module: ModuleHandle,
    consumer: Mutex<Option<SubcConsumer>>,
}
impl DaemonAccessRuntime {
    pub fn new(module: ModuleHandle) -> Self {
        Self {
            module,
            consumer: Mutex::new(None),
        }
    }
}
#[async_trait]
impl AccessRuntime for DaemonAccessRuntime {
    async fn describe(&self, scope: &ScopeIdentity) -> Result<ScopeLife, String> {
        let reply = self
            .module
            .scope_describe(
                Principal::Reserved {
                    module_id: "prefrontal-core".into(),
                },
                scope.scope_ref.clone(),
            )
            .await
            .map_err(|e| e.to_string())?;
        match reply.status {
            ScopeStatus::Live => {
                let stamp = reply.scope.ok_or("scope.describe omitted live stamp")?;
                if stamp.scope_epoch != scope.scope_epoch || stamp.kind != ScopeKind::Head {
                    return Ok(ScopeLife::Gone);
                }
                Ok(ScopeLife::Live(ScopeIdentity {
                    scope_ref: stamp.scope_ref,
                    scope_epoch: stamp.scope_epoch,
                    agent_id: stamp.attributes.agent_id,
                }))
            }
            ScopeStatus::Ended => Ok(ScopeLife::Gone),
            ScopeStatus::NotLive if reply.owner_synced || !reply.owner_configured => {
                Ok(ScopeLife::Gone)
            }
            ScopeStatus::NotLive => Ok(ScopeLife::Unknown),
        }
    }
    async fn notice(&self, body: Value) -> NoticeResult {
        self.notice_json(&body.to_string()).await
    }
    async fn notice_json(&self, body: &str) -> NoticeResult {
        if serde_json::from_str::<serde::de::IgnoredAny>(body).is_err() {
            return NoticeResult::Refused("invalid_params".into());
        }
        // Splice the queued body in verbatim: re-encoding it as a JSON value
        // would sort its keys, and the late-answer notice's key order is fixed
        // by the files in test-vectors/consent-late-answer-v1/.
        let request = format!(r#"{{"method":"session.notice","params":{body}}}"#);
        let mut consumer = self.consumer.lock().await;
        if consumer.is_none() {
            match SubcConsumer::connect_default(ConsumerOptions::default()).await {
                Ok(client) => *consumer = Some(client),
                Err(e) => return NoticeResult::Transport(e.to_string()),
            }
        }
        let reply = consumer
            .as_ref()
            .expect("connected consumer")
            .call(
                RouteTarget::ManagementSurface {
                    module_id: "prefrontal-core".into(),
                },
                BindIdentity::new("/", "cingulate", "permission-notices"),
                request.into_bytes(),
                CallOptions {
                    timeout: Duration::from_secs(5),
                    route_retry_deadline: Duration::from_secs(5),
                    ..Default::default()
                },
            )
            .await;
        match reply {
            Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                Ok(value) if value.get("result").unwrap_or(&value)["ok"] == true => {
                    NoticeResult::Delivered
                }
                _ => NoticeResult::Transport("unreadable session.notice reply".into()),
            },
            Err(CallError::Module(error))
                if matches!(
                    error.code.as_str(),
                    "unknown_session"
                        | "session_not_live"
                        | "unauthorized_caller"
                        | "invalid_params"
                ) =>
            {
                NoticeResult::Refused(error.code)
            }
            Err(e) => NoticeResult::Transport(e.to_string()),
        }
    }
}
impl Handler {
    pub fn set_access_runtime(&self, runtime: Arc<dyn AccessRuntime>) {
        *self
            .inner
            .access_runtime
            .lock()
            .expect("access runtime lock") = runtime;
    }
    fn access_runtime(&self) -> Arc<dyn AccessRuntime> {
        self.inner
            .access_runtime
            .lock()
            .expect("access runtime lock")
            .clone()
    }
    pub(super) async fn worker_head(
        &self,
        scope: &subc_protocol::scope::ScopeStamp,
    ) -> Result<ScopeIdentity, Refusal> {
        let parent = scope
            .parent
            .as_ref()
            .filter(|p| p.owner == scope.owner)
            .ok_or_else(|| Refusal::new("access_no_head", "worker has no stamped parent head"))?;
        let target = ScopeIdentity {
            scope_ref: parent.scope_ref.clone(),
            scope_epoch: parent.scope_epoch,
            agent_id: None,
        };
        match self.access_runtime().describe(&target).await {
            Ok(ScopeLife::Live(head)) => Ok(head),
            Ok(ScopeLife::Gone) => Err(Refusal::new(
                "access_no_head",
                "stamped parent head is not live",
            )),
            Ok(ScopeLife::Unknown) | Err(_) => Err(Refusal::new(
                "access_unavailable",
                "parent head liveness is unavailable",
            )),
        }
    }

    async fn access_identity(
        &self,
        stamp: &Stamp,
        request: &AccessRequest,
    ) -> Result<AccessIdentity, Refusal> {
        if stamp.label() != "reserved:aft" {
            return Err(stamp.refuse());
        }
        let scope = stamp.scope.as_ref().ok_or_else(|| {
            Refusal::new(
                "access_not_alfonso",
                "ask requires a daemon-stamped scope owned by reserved:prefrontal-core",
            )
        })?;
        if scope.owner
            != (Principal::Reserved {
                module_id: "prefrontal-core".into(),
            })
        {
            return Err(Refusal::new(
                "access_not_alfonso",
                format!(
                    "scope owner {:?} is not reserved:prefrontal-core",
                    scope.owner
                ),
            ));
        }
        let head = if scope.kind == ScopeKind::Worker {
            Some(self.worker_head(scope).await?)
        } else {
            None
        };
        if !request.informational() && ((head.is_some()) != request.head_policy().is_some()) {
            return Err(invalid("head_policy is required on worker asks only"));
        }
        if request.informational() && !self.access_runtime().is_macos() {
            return Err(Refusal::new(
                "access_macos_only",
                "macos_permission is refused on this platform",
            ));
        }
        Ok(AccessIdentity {
            principal: stamp.label().into(),
            asker: ScopeIdentity {
                scope_ref: scope.scope_ref.clone(),
                scope_epoch: scope.scope_epoch,
                agent_id: scope.attributes.agent_id.clone(),
            },
            scope_kind: match scope.kind {
                ScopeKind::Head => "head",
                ScopeKind::Worker => "worker",
                ScopeKind::Ephemeral => "ephemeral",
            }
            .into(),
            harness: stamp.harness.clone(),
            head,
        })
    }
    pub(super) async fn access_dispatch(
        &self,
        stamp: &Stamp,
        method: &str,
        params: &Value,
    ) -> Option<Reply> {
        let new_kind = matches!(
            params.get("kind").and_then(Value::as_str),
            Some("tool_permission" | "macos_permission")
        );
        let id = if stamp.operator_surface {
            params.get("elicitationId")
        } else {
            params
                .get("elicitation_id")
                .or_else(|| params.get("elicitationId"))
        }
        .and_then(Value::as_str);
        let access_id = id.is_some_and(|id| id.starts_with("acc_"));
        if method == "consent.request" && new_kind {
            return Some(
                async {
                    if stamp.label() != "reserved:aft" {
                        return Err(stamp.refuse());
                    }
                    let scope = stamp.scope.as_ref().ok_or_else(|| {
                        Refusal::new("access_not_alfonso", "ask requires a daemon-stamped scope")
                    })?;
                    if scope.owner
                        != (Principal::Reserved {
                            module_id: "prefrontal-core".into(),
                        })
                    {
                        return Err(Refusal::new(
                            "access_not_alfonso",
                            "scope is not owned by reserved:prefrontal-core",
                        ));
                    }
                    if scope.kind == ScopeKind::Worker && scope.parent.is_none() {
                        return Err(Refusal::new(
                            "access_no_head",
                            "worker has no stamped parent head",
                        ));
                    }
                    let request = AccessRequest::decode(params.clone())?;
                    // Recover the original record before asking the daemon whether
                    // the worker's supervising session still exists. Its ending must
                    // not prevent a restarted requester from recovering a settled answer.
                    if let Some(key) = request.call_key() {
                        if let Some(record) = self
                            .with_store(|store| store.access_by_call_key(stamp.label(), key))
                            .await?
                        {
                            return Ok(json!({"elicitation_id":record.elicitation_id}));
                        }
                    }
                    let identity = self.access_identity(stamp, &request).await?;
                    let mutation = self
                        .with_store(|store| {
                            store.access_create(
                                identity,
                                request,
                                self.inner.options.clock.now_ms(),
                            )
                        })
                        .await?;
                    self.access_changed(&mutation).await;
                    Ok(json!({"elicitation_id":mutation.record.elicitation_id}))
                }
                .await,
            );
        }
        if !access_id {
            return None;
        }
        let id = id.expect("access id");
        Some(
            async {
                match method {
                    "consent.answer" => {
                        let surface = if stamp.operator_surface {
                            "user"
                        } else if stamp.label() == "reserved:prefrontal-core" {
                            "head"
                        } else {
                            "unauthorized"
                        };
                        let parsed = (|| -> Result<AccessAnswer, Refusal> {
                            let answer_key = if surface == "head" {
                                "answer"
                            } else {
                                "choiceId"
                            };
                            let answer = text(params, answer_key)?.to_owned();
                            let note = params
                                .get("note")
                                .filter(|v| !v.is_null())
                                .map(|v| v.as_str().ok_or_else(|| invalid("note must be a string")))
                                .transpose()?
                                .map(str::to_owned);
                            if let Some(note) = &note {
                                validate_note(note, true)?;
                            }
                            let by = if surface == "head" {
                                let by: RelayIdentity = serde_json::from_value(
                                    params
                                        .get("answered_by")
                                        .cloned()
                                        .ok_or_else(|| invalid("answered_by is required"))?,
                                )
                                .map_err(|e| invalid(e.to_string()))?;
                                if by.agent_id.trim().is_empty() {
                                    return Err(invalid("answered_by.agent_id must be nonblank"));
                                }
                                Some(ScopeIdentity {
                                    scope_ref: by.scope_ref,
                                    scope_epoch: by.scope_epoch,
                                    agent_id: Some(by.agent_id),
                                })
                            } else {
                                None
                            };
                            Ok(AccessAnswer {
                                surface: surface.into(),
                                principal: stamp.label().into(),
                                answer,
                                note,
                                answered_by: by,
                            })
                        })();
                        let answer = match parsed {
                            Ok(answer) => answer,
                            Err(error) => {
                                self.with_store(|store| {
                                    store.access_rejected_attempt(
                                        id,
                                        AnswerAttempt {
                                            malformed_params: Some(params.clone()),
                                            surface: surface.into(),
                                            principal: stamp.label().into(),
                                            at_ms: self.inner.options.clock.now_ms(),
                                            answer: String::new(),
                                            note: None,
                                            answered_by: None,
                                            result: error.code.into(),
                                        },
                                    )
                                })
                                .await?;
                                return Err(error);
                            }
                        };
                        let mutation = self
                            .with_store(|store| {
                                store.access_answer(id, answer, self.inner.options.clock.now_ms())
                            })
                            .await?;
                        if surface == "unauthorized" {
                            return Err(Refusal::new(
                                "elicitation_operator_required",
                                "answer requires a phone operator route or core relay",
                            ));
                        }
                        let Some(mutation) = mutation else {
                            return Ok(json!({"ok":false,"code":"not_found"}));
                        };
                        self.access_changed(&mutation).await;
                        if stamp.operator_surface && mutation.record.state == "pending:head" {
                            return Ok(json!({"ok":false,"code":"access_wrong_leg"}));
                        }
                        let value = mutation
                            .record
                            .wire(stamp.operator_surface, self.block_until(id));
                        if let Some(code) = &mutation.code {
                            Ok(json!({"ok":false,"code":code,"elicitation":value}))
                        } else {
                            Ok(json!({"ok":true,"alreadyAnswered":false,"elicitation":value}))
                        }
                    }
                    "consent.get" | "consent.await" | "consent.withdraw" | "consent.detach" => {
                        if !stamp.operator_surface {
                            stamp.authorize_owner()?;
                        }
                        let record = self
                            .with_store(|store| store.access_get(id))
                            .await?
                            .ok_or_else(|| {
                                Refusal::new("elicitation_not_found", "ask not found")
                            })?;
                        if stamp.operator_surface {
                            if method != "consent.get" || record.state != "pending:user" {
                                return Err(Refusal::new(
                                    "elicitation_not_found",
                                    "ask is not on the user surface",
                                ));
                            }
                        } else if stamp.label() != record.identity.principal {
                            return Err(Refusal::new(
                                "elicitation_not_owner",
                                "ask has no matching principal",
                            ));
                        }
                        match method {
                            // Detach applies only to cards whose `kind` is
                            // `consent`. A permission ask already reaches its
                            // agent through its own session notices (user_ask,
                            // settled, withdrawn), so it has no late-answer mark.
                            "consent.detach" => Err(Refusal::new(
                                "elicitation_requester_refused",
                                "consent.detach is admitted only for kind consent",
                            )),
                            "consent.withdraw" => {
                                let mutation = self
                                    .with_store(|store| {
                                        store.access_withdraw(
                                            stamp.label(),
                                            id,
                                            self.inner.options.clock.now_ms(),
                                        )
                                    })
                                    .await?;
                                self.access_changed(&mutation).await;
                                Ok(mutation.record.wire(false, None))
                            }
                            "consent.await" if record.request.informational() => Err(Refusal::new(
                                "access_informational",
                                "macos_permission has no waiting call",
                            )),
                            "consent.await" => {
                                self.await_access(id, integer(params, "timeout_ms")?.min(120_000))
                                    .await
                            }
                            _ => Ok(record.wire(stamp.operator_surface, self.block_until(id))),
                        }
                    }
                    _ => Err(Refusal::new(
                        "elicitation_invalid_request",
                        "operation is not supported for permission asks",
                    )),
                }
            }
            .await,
        )
    }
    async fn access_changed(&self, mutation: &AccessMutation) {
        if !mutation.changed {
            return;
        }
        let record = &mutation.record;
        self.wake(&record.elicitation_id);
        if record.surfaces.iter().any(|s| s["surface"] == "user") {
            let mut value = json!({"elicitationId":record.elicitation_id,"kind":if record.pending() { "elicitation" } else { "retraction" },"eventAtMs":record.settled_at_ms.unwrap_or(record.created_at_ms)});
            if let Some(machine) = self.inner.machine_id.lock().expect("machine lock").clone() {
                value["machineId"] = json!(machine);
            }
            if let Err(error) = self.inner.options.notifier.notify(value).await {
                tracing::warn!(%error, "permission phone notification failed after commit");
            }
        }
        self.spawn_notice_delivery();
    }
    pub(super) fn spawn_notice_delivery(&self) {
        let handler = self.clone();
        tokio::spawn(async move {
            if let Err(error) = handler.deliver_access_notices().await {
                tracing::warn!(%error, "notice delivery failed");
            }
        });
    }
    pub async fn deliver_access_notices(&self) -> Result<(), Refusal> {
        let _delivery = self.inner.notice_delivery.lock().await;
        let notices = self
            .with_store(|store| store.access_notices(self.inner.options.clock.now_ms()))
            .await?;
        for notice in notices {
            if !self
                .with_store(|store| store.access_notice_pending(&notice))
                .await?
            {
                continue;
            }
            let result = self.access_runtime().notice_json(&notice.body_json).await;
            let code = match &result {
                NoticeResult::Delivered => "ok",
                NoticeResult::Refused(code) => {
                    tracing::warn!(%code, purpose=%notice.purpose, "core refused permission notice");
                    code.as_str()
                }
                NoticeResult::Transport(error) => {
                    tracing::warn!(%error, purpose=%notice.purpose, "permission notice transport failed; delivery will retry");
                    "transport"
                }
            };
            let record = self
                .with_store(|store| {
                    store.access_notice_result(&notice, code, self.inner.options.clock.now_ms())
                })
                .await?;
            if let Some(record) = record {
                self.wake(&record.elicitation_id);
            }
        }
        Ok(())
    }
    pub(super) async fn access_maintenance(&self) -> Result<(), Refusal> {
        for record in self.with_store(ConsentStore::access_pending).await? {
            if record.state == "pending:head" {
                if let Some(head) = &record.identity.head {
                    if matches!(
                        self.access_runtime().describe(head).await,
                        Ok(ScopeLife::Gone)
                    ) {
                        if let Some(record) = self
                            .with_store(|store| {
                                store.access_head_gone(
                                    &record.elicitation_id,
                                    self.inner.options.clock.now_ms(),
                                )
                            })
                            .await?
                        {
                            self.wake(&record.elicitation_id);
                        }
                    }
                }
            }
        }
        self.deliver_access_notices().await
    }
    async fn await_access(&self, id: &str, timeout: i64) -> Reply {
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
            receiver.borrow_and_update();
            let record = self
                .with_store(|store| store.access_get(id))
                .await?
                .ok_or_else(|| Refusal::new("elicitation_not_found", "ask not found"))?;
            if !record.pending() || self.inner.options.clock.now_ms() >= deadline {
                drop(registration);
                return Ok(record.wire(false, None));
            }
            tokio::select! { _ = receiver.changed() => {}, _ = self.inner.options.clock.sleep_until(deadline) => {} }
        }
    }
}
