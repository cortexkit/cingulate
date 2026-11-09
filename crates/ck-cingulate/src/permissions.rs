//! The permissions inventory belongs to its providers, not the consent store.
//! Catalog and provider failures must never look like an empty grant inventory.
use super::*;
use std::{collections::BTreeMap, sync::atomic::AtomicU64};
use subc_client_rs::consumer::{
    CallError, CallOptions, CatalogList, CloseRouteOptions, SubcConsumer,
};
use subc_protocol::{BindIdentity, RouteTarget};

const PROVIDER_TIMEOUT: Duration = Duration::from_secs(5);
static NEXT_ROUTE: AtomicU64 = AtomicU64::new(1);

struct ProviderRoute {
    consumer: Arc<SubcConsumer>,
    target: RouteTarget,
    identity: BindIdentity,
    options: CloseRouteOptions,
    closed: bool,
}

impl ProviderRoute {
    async fn close(mut self) {
        self.consumer
            .close_route(
                self.target.clone(),
                self.identity.clone(),
                self.options.clone(),
            )
            .await;
        self.closed = true;
    }
}

impl Drop for ProviderRoute {
    fn drop(&mut self) {
        if !self.closed {
            // The handler's timeout can cancel a request while its route is
            // opening or awaiting a reply. Cleanup must survive that cancellation.
            let consumer = self.consumer.clone();
            let target = self.target.clone();
            let identity = self.identity.clone();
            let options = self.options.clone();
            tokio::spawn(async move {
                consumer.close_route(target, identity, options).await;
            });
        }
    }
}

#[derive(Debug, Clone)]
pub enum PermissionsCallError {
    Refused(String),
    Unreachable,
}

/// The daemon catalog and provider calls used by the real permissions handler.
#[async_trait]
pub trait PermissionsRuntime: Send + Sync {
    async fn catalog(&self) -> Result<CatalogList, String>;
    async fn call(
        &self,
        module: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, PermissionsCallError>;
}

pub(super) struct UnavailablePermissionsRuntime;
#[async_trait]
impl PermissionsRuntime for UnavailablePermissionsRuntime {
    async fn catalog(&self) -> Result<CatalogList, String> {
        Err("daemon catalog.list unavailable".into())
    }
    async fn call(&self, _: &str, _: &str, _: Value) -> Result<Value, PermissionsCallError> {
        Err(PermissionsCallError::Unreachable)
    }
}

#[async_trait]
impl PermissionsRuntime for DaemonAccessRuntime {
    async fn catalog(&self) -> Result<CatalogList, String> {
        self.consumer()
            .await?
            .catalog_list()
            .await
            .map_err(|error| error.to_string())
    }

    async fn call(
        &self,
        module: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, PermissionsCallError> {
        let consumer = self
            .consumer()
            .await
            .map_err(|_| PermissionsCallError::Unreachable)?;
        let options = CallOptions {
            timeout: PROVIDER_TIMEOUT,
            route_retry_deadline: PROVIDER_TIMEOUT,
            ..Default::default()
        };
        // Unique bind identities prevent the SDK's route cache from sharing a
        // route between concurrent calls whose cleanup must be independent.
        let route = ProviderRoute {
            consumer: consumer.clone(),
            target: RouteTarget::ManagementSurface {
                module_id: module.into(),
            },
            identity: BindIdentity::new(
                "cingulate",
                format!("permissions:{}", NEXT_ROUTE.fetch_add(1, Ordering::Relaxed)),
            ),
            options: CloseRouteOptions {
                consumer_identity: options.consumer_identity.clone(),
                reverse_requests: options.reverse_requests.clone(),
                role_versions: options.role_versions.clone(),
                ..Default::default()
            },
            closed: false,
        };
        let result = async {
            let handle = consumer
                .open_route(
                    route.target.clone(),
                    route.identity.clone(),
                    options.clone(),
                )
                .await
                .map_err(|_| PermissionsCallError::Unreachable)?;
            // A handle request never reopens and resends on stale-channel errors,
            // unlike managed `call`: a revoke may already have reached the module.
            // The SDK retries only daemon-origin route_busy refusals on opted-in
            // routes, which prove the request was never forwarded to the module.
            let bytes = consumer
                .request(
                    &handle,
                    serde_json::to_vec(&json!({"method":method,"params":params}))
                        .expect("permissions request serialization"),
                    options,
                )
                .await
                .map_err(|error| match error {
                    CallError::Module(error) => PermissionsCallError::Refused(error.code),
                    _ => PermissionsCallError::Unreachable,
                })?;
            let value: Value =
                serde_json::from_slice(&bytes).map_err(|_| PermissionsCallError::Unreachable)?;
            Ok(value.get("result").cloned().unwrap_or(value))
        }
        .await;
        route.close().await;
        result
    }
}

fn unavailable() -> Refusal {
    Refusal::new(
        "permissions_unavailable",
        "permissions catalog is unavailable",
    )
}

fn optional_text<'a>(params: &'a Value, key: &str) -> Result<Option<&'a str>, Refusal> {
    params.get(key).map(|_| text(params, key)).transpose()
}

impl Handler {
    pub fn set_permissions_runtime(&self, runtime: Arc<dyn PermissionsRuntime>) {
        *self
            .inner
            .permissions_runtime
            .lock()
            .expect("permissions runtime lock") = runtime;
    }

    pub(super) async fn permissions_dispatch(
        &self,
        stamp: &Stamp,
        method: &str,
        params: &Value,
    ) -> Option<Reply> {
        if !matches!(method, "permissions.list_all" | "permissions.revoke") {
            return None;
        }
        Some(self.permissions(stamp, method, params).await)
    }

    async fn permissions(&self, stamp: &Stamp, method: &str, params: &Value) -> Reply {
        let admitted = stamp.label() == "direct"
            || (stamp.label() == "reserved:callosum"
                && (stamp.operator_surface
                    || (method == "permissions.list_all" && stamp.sim_surface)));
        if !admitted {
            return Err(Refusal::new(
                "permissions_operator_required",
                "permissions operations need the operator: reserved:callosum on an operator surface, or direct",
            ));
        }
        if !params.is_object() {
            return Err(invalid("permissions params must be an object"));
        }
        let agent_id = if method == "permissions.list_all" {
            optional_text(params, "agent_id")?
        } else {
            None
        };
        let since_version = if method == "permissions.list_all" {
            optional_text(params, "since_version")?
        } else {
            None
        };
        let target = if method == "permissions.revoke" {
            Some((text(params, "module")?, text(params, "id")?))
        } else {
            None
        };
        let runtime = self
            .inner
            .permissions_runtime
            .lock()
            .expect("permissions runtime lock")
            .clone();
        let catalog = tokio::time::timeout(PROVIDER_TIMEOUT, runtime.catalog())
            .await
            .map_err(|_| unavailable())?
            .map_err(|_| unavailable())?;
        let mut providers = BTreeMap::new();
        let mut display_names = BTreeMap::new();
        for entry in catalog.modules {
            if let Some(display_name) = entry.display_name {
                display_names.insert(entry.module_id.clone(), display_name);
            }
            if entry.capabilities.as_ref().is_some_and(|capabilities| {
                capabilities
                    .provides
                    .iter()
                    .any(|cap| cap == "permissions/v1")
            }) {
                let supports_list = entry.roles.iter().any(|role| {
                    matches!(role, ProviderRole::ManagementSurface { operations, .. }
                        if operations.iter().any(|op| op.name == "permissions.list"))
                });
                providers.insert(entry.module_id, (entry.ready, supports_list));
            }
        }
        if let Some((module, id)) = target {
            let Some((ready, _)) = providers.get(module) else {
                return Ok(json!({"outcome":"refused","code":"not_a_provider"}));
            };
            if !ready {
                return Ok(json!({"outcome":"module_unreachable"}));
            }
            // A lost reply does not prove that the grant survived. Report the
            // ambiguity and leave reconciliation to the next inventory read.
            return Ok(
                match tokio::time::timeout(
                    PROVIDER_TIMEOUT,
                    runtime.call(module, "permissions.revoke", json!({"id":id})),
                )
                .await
                {
                    Ok(Ok(reply)) => match reply["outcome"].as_str() {
                        Some("revoked") if reply["stopping"] == true => {
                            json!({"outcome":"stopping"})
                        }
                        Some("revoked") => json!({"outcome":"revoked"}),
                        Some("not_found") => json!({"outcome":"already_gone"}),
                        _ => json!({"outcome":"module_unreachable"}),
                    },
                    Ok(Err(PermissionsCallError::Refused(code))) => {
                        json!({"outcome":"refused","code":code})
                    }
                    _ => json!({"outcome":"module_unreachable"}),
                },
            );
        }

        let list_params = agent_id.map_or_else(|| json!({}), |id| json!({"agent_id":id}));
        let mut calls = Vec::new();
        for (module, (ready, supports_list)) in providers {
            let runtime = runtime.clone();
            let params = list_params.clone();
            let name = module.clone();
            let call = tokio::spawn(async move {
                if !ready {
                    return json!({"module":name,"status":"unreachable"});
                }
                if !supports_list {
                    return json!({"module":name,"status":"unsupported"});
                }
                match tokio::time::timeout(
                    PROVIDER_TIMEOUT,
                    runtime.call(&name, "permissions.list", params),
                )
                .await
                {
                    Ok(Ok(reply)) if reply["grants"].is_array() => {
                        json!({"module":name,"status":"ok","grants":reply["grants"]})
                    }
                    Ok(Err(PermissionsCallError::Refused(code))) if code == "unknown_method" => {
                        json!({"module":name,"status":"unsupported"})
                    }
                    _ => json!({"module":name,"status":"unreachable"}),
                }
            });
            calls.push((module, call));
        }
        let mut modules = Vec::new();
        for (module, call) in calls {
            modules.push(
                call.await
                    .unwrap_or_else(|_| json!({"module":module,"status":"unreachable"})),
            );
        }
        for module in &mut modules {
            if module["status"] != "ok" {
                continue;
            }
            let Some(grants) = module["grants"].as_array_mut() else {
                continue;
            };
            for grant in grants {
                let Some(holder) = grant.get_mut("holder").and_then(Value::as_object_mut) else {
                    continue;
                };
                if holder.get("kind").and_then(Value::as_str) != Some("module")
                    || holder.contains_key("label")
                {
                    continue;
                }
                if let Some(display_name) = holder
                    .get("module_id")
                    .and_then(Value::as_str)
                    .and_then(|id| display_names.get(id))
                {
                    holder.insert("label".into(), json!(display_name));
                }
            }
        }
        let canonical = serde_jcs::to_vec(&modules).map_err(|_| unavailable())?;
        let version = format!("{:x}", Sha256::digest(canonical));
        if since_version == Some(&version) {
            Ok(json!({"version":version,"unchanged":true}))
        } else {
            Ok(json!({"version":version,"modules":modules}))
        }
    }
}
