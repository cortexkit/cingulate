//! Best-effort display identity reads. Only an explicit terminal status refuses a card.
use super::*;
use cortexkit_agent_identity::{
    AgentFleetIdentityParams, AgentFleetIdentityReply, AgentResolveParams, AgentResolveReply,
    AgentStatus, Outcome, Presence, Request,
};
use subc_client_rs::consumer::{CallError, CallOptions, CloseRouteOptions, SubcConsumer};
use subc_protocol::{BindIdentity, RouteTarget};

mod projects;

const LOOKUP_TIMEOUT: Duration = Duration::from_millis(500);
// Leave half the overall deadline for status when a name read stalls.
const NAME_LOOKUP_TIMEOUT: Duration = Duration::from_millis(250);
const CACHE_MS: i64 = 60_000;
const CACHE_CAPACITY: usize = 1_024;
static NEXT_ROUTE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum Failure {
    Timeout,
    Route,
    Transport,
    Refused,
    Decode,
    Unknown,
    UnknownStatus,
    Unavailable,
}

#[async_trait]
trait AgentRuntime: Send + Sync {
    async fn resolve(&self, params: AgentResolveParams) -> Result<AgentResolveReply, Failure>;
    async fn fleet_identity(
        &self,
        params: AgentFleetIdentityParams,
    ) -> Result<AgentFleetIdentityReply, Failure>;
}

enum RosterReply {
    Snapshot {
        token: String,
        names: HashMap<String, String>,
    },
    Unchanged,
}

#[async_trait]
trait NameSource: Send + Sync {
    async fn refresh(&self, token: Option<&str>) -> Result<RosterReply, Failure>;
}

type RuntimeSlot = Arc<StdMutex<Option<Arc<dyn AgentRuntime>>>>;

struct DaemonNames {
    runtime: RuntimeSlot,
}

#[async_trait]
impl NameSource for DaemonNames {
    async fn refresh(&self, token: Option<&str>) -> Result<RosterReply, Failure> {
        let runtime = self
            .runtime
            .lock()
            .expect("identity runtime lock")
            .clone()
            .ok_or(Failure::Unavailable)?;
        let reply = runtime
            .fleet_identity(AgentFleetIdentityParams::new(
                token.map_or(Presence::Missing, |token| {
                    Presence::Present(Value::String(token.to_owned()))
                }),
            ))
            .await?;
        if reply.unchanged {
            return Ok(RosterReply::Unchanged);
        }
        let Presence::Present(agents) = reply.agents else {
            return Err(Failure::Decode);
        };
        Ok(RosterReply::Snapshot {
            token: reply.token,
            names: agents
                .into_iter()
                .map(|agent| (agent.agent_id, agent.display_name))
                .collect(),
        })
    }
}

#[derive(Default)]
struct Roster {
    token: Option<String>,
    names: HashMap<String, String>,
    refreshed_at: Option<i64>,
}

struct CachedAgent {
    at: i64,
    agent: ResolvedAgent,
}

struct Lookup {
    agent: ResolvedAgent,
    cache_at: Option<i64>,
}

/// Production resolver with a late-bound daemon runtime, like the access handler.
pub struct EntorhinalResolver {
    clock: Arc<dyn Clock>,
    runtime: RuntimeSlot,
    names: Arc<dyn NameSource>,
    roster: Mutex<Roster>,
    cache: StdMutex<HashMap<String, CachedAgent>>,
    warnings: StdMutex<HashMap<Failure, i64>>,
    projects: projects::ProjectNames,
}

impl Default for EntorhinalResolver {
    fn default() -> Self {
        Self::new(Arc::new(SystemClock))
    }
}

impl EntorhinalResolver {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        let runtime = Arc::new(StdMutex::new(None));
        Self {
            projects: projects::ProjectNames::new(clock.clone()),
            clock,
            runtime: runtime.clone(),
            names: Arc::new(DaemonNames { runtime }),
            roster: Mutex::new(Roster::default()),
            cache: StdMutex::new(HashMap::new()),
            warnings: StdMutex::new(HashMap::new()),
        }
    }

    /// Bind after serving starts, since the daemon supplies the module handle then.
    pub fn set_runtime(&self, runtime: Arc<DaemonAccessRuntime>) {
        self.projects.set_runtime(runtime.clone());
        *self.runtime.lock().expect("identity runtime lock") = Some(runtime);
    }

    fn warn(&self, kind: Failure) {
        let now = self.clock.now_ms();
        let mut warnings = self.warnings.lock().expect("identity warning lock");
        if warnings
            .get(&kind)
            .is_none_or(|last| now.saturating_sub(*last) >= CACHE_MS)
        {
            warnings.insert(kind, now);
            tracing::warn!(kind = ?kind, "agent identity lookup fell back to the id");
        }
    }

    async fn name(&self, agent_id: &str) -> Result<Option<(String, i64)>, Failure> {
        let mut roster = self.roster.lock().await;
        let now = self.clock.now_ms();
        let stale = roster
            .refreshed_at
            .is_none_or(|at| now.saturating_sub(at) >= CACHE_MS);
        // One refresh serves both expiry and a miss; do not refresh twice when
        // a newly fetched roster still has no entry for the requested agent.
        if stale || !roster.names.contains_key(agent_id) {
            match self.names.refresh(roster.token.as_deref()).await? {
                RosterReply::Snapshot { token, names } => {
                    roster.token = Some(token);
                    roster.names = names;
                }
                RosterReply::Unchanged => {}
            }
            roster.refreshed_at = Some(self.clock.now_ms());
        }
        Ok(roster
            .names
            .get(agent_id)
            .map(|name| (name.clone(), roster.refreshed_at.expect("fetched roster"))))
    }

    /// A roster hit is trusted because the roster lists only live agents, and an
    /// agent retired since the last fetch can pass for up to the 60 s cache window.
    async fn lookup(&self, agent_id: &str) -> Result<Lookup, Failure> {
        let (name, name_failed) =
            match tokio::time::timeout(NAME_LOOKUP_TIMEOUT, self.name(agent_id)).await {
                Ok(Ok(name)) => (name, false),
                result => {
                    self.warn(match result {
                        Ok(Err(kind)) => kind,
                        Err(_) => Failure::Timeout,
                        Ok(Ok(_)) => unreachable!(),
                    });
                    (None, true)
                }
            };
        if let Some((name, at)) = name {
            return Ok(Lookup {
                agent: ResolvedAgent {
                    name: Some(name),
                    retired: false,
                },
                cache_at: Some(at),
            });
        }
        let runtime = self
            .runtime
            .lock()
            .expect("identity runtime lock")
            .clone()
            .ok_or(Failure::Unavailable)?;
        let reply = runtime
            .resolve(AgentResolveParams::new(agent_id.to_owned()))
            .await?;
        if reply.agent.agent_id != agent_id {
            return Err(Failure::Decode);
        }
        let retired = match reply.agent.status {
            AgentStatus::Live => false,
            AgentStatus::Retired | AgentStatus::Merged => true,
            AgentStatus::Unknown => return Err(Failure::Unknown),
            _ => return Err(Failure::UnknownStatus),
        };
        Ok(Lookup {
            agent: ResolvedAgent {
                name: None,
                retired,
            },
            // Retry a failed display read on the next card rather than retaining
            // an incomplete result for the successful-result cache window.
            cache_at: (!name_failed).then(|| self.clock.now_ms()),
        })
    }
}

#[async_trait]
impl Resolver for EntorhinalResolver {
    async fn project_name(&self, project_id: &str) -> Option<String> {
        self.projects.name(project_id).await
    }

    async fn resolve(&self, agent_id: &str) -> Result<Option<ResolvedAgent>, String> {
        let now = self.clock.now_ms();
        {
            let mut cache = self.cache.lock().expect("identity cache lock");
            cache.retain(|_, entry| now.saturating_sub(entry.at) < CACHE_MS);
            if let Some(entry) = cache.get(agent_id) {
                return Ok(Some(entry.agent.clone()));
            }
        }
        match tokio::time::timeout(LOOKUP_TIMEOUT, self.lookup(agent_id)).await {
            Ok(Ok(lookup)) => {
                if let Some(at) = lookup.cache_at {
                    let mut cache = self.cache.lock().expect("identity cache lock");
                    if cache.len() >= CACHE_CAPACITY {
                        if let Some(oldest) = cache
                            .iter()
                            .min_by_key(|(_, entry)| entry.at)
                            .map(|(id, _)| id.clone())
                        {
                            cache.remove(&oldest);
                        }
                    }
                    cache.insert(
                        agent_id.to_owned(),
                        CachedAgent {
                            at,
                            agent: lookup.agent.clone(),
                        },
                    );
                }
                Ok(Some(lookup.agent))
            }
            result => {
                self.warn(match result {
                    Err(_) => Failure::Timeout,
                    Ok(Err(kind)) => kind,
                    Ok(Ok(_)) => unreachable!(),
                });
                Ok(None)
            }
        }
    }
}

struct AgentRoute {
    consumer: Arc<SubcConsumer>,
    identity: BindIdentity,
    closed: bool,
}

fn target() -> RouteTarget {
    RouteTarget::ManagementSurface {
        module_id: "entorhinal".into(),
    }
}

impl Drop for AgentRoute {
    fn drop(&mut self) {
        if !self.closed {
            // A lookup timeout cancels opening or reading the route. Close it
            // independently so cancellation does not leak daemon routes.
            let consumer = self.consumer.clone();
            let identity = self.identity.clone();
            tokio::spawn(async move {
                consumer
                    .close_route(target(), identity, CloseRouteOptions::default())
                    .await;
            });
        }
    }
}

#[async_trait]
impl AgentRuntime for DaemonAccessRuntime {
    async fn resolve(&self, params: AgentResolveParams) -> Result<AgentResolveReply, Failure> {
        self.identity_call("agent.resolve", params).await
    }

    async fn fleet_identity(
        &self,
        params: AgentFleetIdentityParams,
    ) -> Result<AgentFleetIdentityReply, Failure> {
        self.identity_call("agent.fleet_identity", params).await
    }
}

fn decode_reply<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, Failure> {
    match serde_json::from_slice::<Outcome<T>>(bytes).map_err(|_| Failure::Decode)? {
        Outcome::Success(success) => Ok(success.result),
        Outcome::Refusal(_) => Err(Failure::Refused),
        _ => Err(Failure::Decode),
    }
}

impl DaemonAccessRuntime {
    async fn identity_call<P: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: P,
    ) -> Result<T, Failure> {
        decode_reply(&self.identity_request(Request::new(method, params)).await?)
    }

    async fn identity_request<P: serde::Serialize>(&self, request: P) -> Result<Vec<u8>, Failure> {
        let consumer = self.consumer().await.map_err(|_| Failure::Transport)?;
        let options = CallOptions {
            timeout: LOOKUP_TIMEOUT,
            route_retry_deadline: LOOKUP_TIMEOUT,
            ..Default::default()
        };
        let mut route = AgentRoute {
            consumer: consumer.clone(),
            identity: BindIdentity::new(
                "/",
                "cingulate",
                format!(
                    "agent-identity:{}",
                    NEXT_ROUTE.fetch_add(1, Ordering::Relaxed)
                ),
            ),
            closed: false,
        };
        let result = async {
            let handle = consumer
                .open_route(target(), route.identity.clone(), options.clone())
                .await
                .map_err(|_| Failure::Route)?;
            let bytes = consumer
                .request(
                    &handle,
                    serde_json::to_vec(&request).map_err(|_| Failure::Decode)?,
                    options,
                )
                .await
                .map_err(|error| match error {
                    CallError::Module(_) => Failure::Refused,
                    _ => Failure::Transport,
                })?;
            Ok(bytes)
        }
        .await;
        consumer
            .close_route(
                target(),
                route.identity.clone(),
                CloseRouteOptions::default(),
            )
            .await;
        route.closed = true;
        result
    }
}

#[cfg(test)]
mod tests;
