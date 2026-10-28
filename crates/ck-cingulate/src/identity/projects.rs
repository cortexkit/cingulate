//! Project names are display-only and come from Entorhinal, never card scope text.
use super::*;
use cortexkit_project_identity::{
    EnumerateParams, EnumerateReply, Outcome, Request, ResolveProjectIdParams,
    ResolveProjectIdReply, Via,
};

#[async_trait]
pub(super) trait ProjectRuntime: Send + Sync {
    async fn resolve_project_id(
        &self,
        params: ResolveProjectIdParams,
    ) -> Result<ResolveProjectIdReply, Failure>;
    async fn enumerate(&self, params: EnumerateParams) -> Result<EnumerateReply, Failure>;
}

struct Registry {
    at: i64,
    names: HashMap<String, String>,
}

pub(super) struct ProjectNames {
    clock: Arc<dyn Clock>,
    pub(super) runtime: StdMutex<Option<Arc<dyn ProjectRuntime>>>,
    registry: Mutex<Option<Registry>>,
}

impl ProjectNames {
    pub(super) fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            clock,
            runtime: StdMutex::new(None),
            registry: Mutex::new(None),
        }
    }

    pub(super) fn set_runtime(&self, runtime: Arc<DaemonAccessRuntime>) {
        *self.runtime.lock().expect("project runtime lock") = Some(runtime);
    }

    pub(super) async fn name(&self, project_id: &str) -> Option<String> {
        tokio::time::timeout(LOOKUP_TIMEOUT, self.lookup(project_id))
            .await
            .ok()?
            .ok()?
    }

    async fn lookup(&self, project_id: &str) -> Result<Option<String>, Failure> {
        let runtime = self
            .runtime
            .lock()
            .expect("project runtime lock")
            .clone()
            .ok_or(Failure::Unavailable)?;
        let resolved = runtime
            .resolve_project_id(ResolveProjectIdParams::new(project_id.to_owned()))
            .await?;
        if resolved.gone {
            return Ok(None);
        }
        let current_id = match resolved.via {
            Via::Alias => resolved.project_id.as_str(),
            Via::Current if resolved.project_id == project_id => project_id,
            _ => return Err(Failure::Decode),
        };
        let mut registry = self.registry.lock().await;
        if registry
            .as_ref()
            .is_none_or(|registry| self.clock.now_ms().saturating_sub(registry.at) >= CACHE_MS)
        {
            let reply = runtime.enumerate(EnumerateParams::new(None)).await?;
            // Enumerations include every root; retain only the display names for a minute.
            *registry = Some(Registry {
                at: self.clock.now_ms(),
                names: reply
                    .projects
                    .into_iter()
                    .map(|project| (project.project_id, project.name))
                    .collect(),
            });
        }
        Ok(registry
            .as_ref()
            .and_then(|registry| registry.names.get(current_id))
            .filter(|name| !name.trim().is_empty())
            .cloned())
    }
}

pub(super) fn decode_project_reply<T: serde::de::DeserializeOwned>(
    bytes: &[u8],
) -> Result<T, Failure> {
    match serde_json::from_slice::<Outcome<T>>(bytes).map_err(|_| Failure::Decode)? {
        Outcome::Success(success) => Ok(success.result),
        Outcome::Refusal(_) => Err(Failure::Refused),
        _ => Err(Failure::Decode),
    }
}

#[async_trait]
impl ProjectRuntime for DaemonAccessRuntime {
    async fn resolve_project_id(
        &self,
        params: ResolveProjectIdParams,
    ) -> Result<ResolveProjectIdReply, Failure> {
        decode_project_reply(
            &self
                .identity_request(Request::new("resolve_project_id", params))
                .await?,
        )
    }

    async fn enumerate(&self, params: EnumerateParams) -> Result<EnumerateReply, Failure> {
        decode_project_reply(
            &self
                .identity_request(Request::new("enumerate", params))
                .await?,
        )
    }
}
