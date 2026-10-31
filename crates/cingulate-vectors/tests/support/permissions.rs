#![allow(dead_code)]

use async_trait::async_trait;
use ck_cingulate::{Handler, Options, PermissionsCallError, PermissionsRuntime, Stamp};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use subc_client_rs::{consumer::CatalogList, HandlerOutcome, RouteBindRequest, RouteHandle};
use subc_protocol::{
    manifest::{Concurrency, ManagementOperation, ManagementOperationKind, ProviderRole},
    BindIdentity, Principal, RouteTarget,
};

#[derive(Clone)]
pub enum Answer {
    Reply(Value),
    Refused(&'static str),
    Lost,
    Hang,
}

#[derive(Clone)]
pub struct Provider {
    pub module: String,
    pub capabilities: Vec<String>,
    pub ready: bool,
    pub supports_list: bool,
    pub list: Answer,
    pub revoke: Answer,
}
impl Provider {
    pub fn new(module: &str, grants: Value) -> Self {
        Self {
            module: module.into(),
            capabilities: vec!["permissions/v1".into()],
            ready: true,
            supports_list: true,
            list: Answer::Reply(json!({"grants":grants})),
            revoke: Answer::Reply(json!({"outcome":"revoked"})),
        }
    }
    fn catalog_entry(&self) -> Value {
        let mut operations = vec![ManagementOperation {
            name: "permissions.revoke".into(),
            kind: ManagementOperationKind::Mutate,
            description: None,
        }];
        if self.supports_list {
            operations.push(ManagementOperation {
                name: "permissions.list".into(),
                kind: ManagementOperationKind::Query,
                description: None,
            });
        }
        let role = ProviderRole::ManagementSurface {
            operations,
            config_schema: json!({}),
            observability: vec![],
            identity_scope: vec![],
            concurrency: Concurrency::ModuleManaged,
        };
        json!({"module_id":self.module,"ready":self.ready,"roles":[role],"control_ops":[],
            "capabilities":{"provides":self.capabilities,"requires":[],"must_never_reach":[]}})
    }
}

pub struct Runtime {
    pub providers: Vec<Provider>,
    pub catalog_available: bool,
    pub calls: Mutex<Vec<(String, String, Value)>>,
}
impl Runtime {
    pub fn new(providers: Vec<Provider>) -> Self {
        Self {
            providers,
            catalog_available: true,
            calls: Mutex::new(vec![]),
        }
    }
}
#[async_trait]
impl PermissionsRuntime for Runtime {
    async fn catalog(&self) -> Result<CatalogList, String> {
        if !self.catalog_available {
            return Err("catalog lost".into());
        }
        Ok(serde_json::from_value(json!({"generation":1,"modules":self.providers.iter().map(Provider::catalog_entry).collect::<Vec<_>>()})).unwrap())
    }
    async fn call(
        &self,
        module: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, PermissionsCallError> {
        self.calls
            .lock()
            .unwrap()
            .push((module.into(), method.into(), params.clone()));
        let provider = self.providers.iter().find(|p| p.module == module).unwrap();
        let answer = match method {
            "permissions.list" => &provider.list,
            "permissions.revoke" => &provider.revoke,
            _ => panic!("unexpected method {method}"),
        };
        match answer {
            Answer::Reply(value) => {
                let mut value = value.clone();
                if method == "permissions.list" {
                    if let Some(agent) = params.get("agent_id") {
                        if let Some(grants) = value["grants"].as_array_mut() {
                            grants.retain(|row| row.get("agent_id") == Some(agent));
                        }
                    }
                }
                Ok(value)
            }
            Answer::Refused(code) => Err(PermissionsCallError::Refused((*code).into())),
            Answer::Lost => Err(PermissionsCallError::Unreachable),
            Answer::Hang => std::future::pending().await,
        }
    }
}

pub fn row(module: &str, id: &str, agent: &str) -> Value {
    let (subject, scope, label) = if module == "cerebellum" {
        ("TextEdit", "app", "the app TextEdit")
    } else {
        (
            "Hire helpers",
            "agent_hires",
            "hire helpers without asking again",
        )
    };
    json!({"id":id,"agent_id":agent,"holder":{"kind":"agent","agent_id":agent},
        "module":module,"subject_label":subject,"scope":scope,
        "scope_label":label,"granted_at_ms":1000000})
}
pub fn phone() -> Stamp {
    bound("callosum", "fed-phone:device")
}
pub fn bound(principal: &str, harness: &str) -> Stamp {
    Stamp::from_bind(
        &RouteBindRequest::new(
            RouteHandle::detached(1, 1),
            RouteTarget::ManagementSurface {
                module_id: "cingulate".into(),
            },
            BindIdentity::new(harness, "surface"),
        )
        .with_principal(Principal::Reserved {
            module_id: principal.into(),
        }),
    )
}
pub fn handler(runtime: Arc<Runtime>) -> Handler {
    let handler = Handler::new(Options {
        sweep_timer: false,
        ..Default::default()
    });
    handler.set_permissions_runtime(runtime);
    handler
}
pub async fn call(
    handler: &Handler,
    stamp: &Stamp,
    method: &str,
    params: Value,
) -> Result<Value, Value> {
    let body = serde_json::to_vec(&json!({"method":method,"params":params})).unwrap();
    match handler.handle_stamped(stamp, &body).await {
        HandlerOutcome::Response(bytes) => {
            Ok(serde_json::from_slice::<Value>(&bytes).unwrap()["result"].clone())
        }
        HandlerOutcome::Error { code, message } => Err(json!({"code":code,"message":message})),
        other => panic!("unexpected outcome {other:?}"),
    }
}

pub fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-vectors/permissions-v1")
}

async fn capture(
    cases: &mut Vec<(&'static str, Value)>,
    name: &'static str,
    runtime: Runtime,
    stamp: Stamp,
    method: &str,
    params: Value,
) {
    let handler = handler(Arc::new(runtime));
    let mut case = json!({"tag":"permissions-v1","method":method,"now_ms":1060000,"request":params,
        "route":{"principal":stamp.principal,"operator_surface":stamp.operator_surface,"sim_surface":stamp.sim_surface}});
    match call(&handler, &stamp, method, params).await {
        Ok(reply) => case["reply"] = reply,
        Err(error) => case["error"] = error,
    }
    cases.push((name, case));
}

pub async fn served_cases() -> Vec<(&'static str, Value)> {
    let providers = || {
        vec![
            Provider::new("z-hiring", json!([row("z-hiring", "hire-1", "agent-alf")])),
            Provider::new(
                "cerebellum",
                json!([row("cerebellum", "computer-1", "agent-alf")]),
            ),
        ]
    };
    let mut cases = Vec::new();
    capture(
        &mut cases,
        "served-list-two-providers",
        Runtime::new(providers()),
        phone(),
        "permissions.list_all",
        json!({}),
    )
    .await;
    let mut timeout = providers();
    timeout[0].list = Answer::Hang;
    capture(
        &mut cases,
        "served-list-timeout",
        Runtime::new(timeout),
        phone(),
        "permissions.list_all",
        json!({}),
    )
    .await;
    let mut not_ready = providers();
    not_ready[0].ready = false;
    capture(
        &mut cases,
        "served-list-not-ready",
        Runtime::new(not_ready),
        phone(),
        "permissions.list_all",
        json!({}),
    )
    .await;
    capture(
        &mut cases,
        "served-list-sim",
        Runtime::new(providers()),
        bound("callosum", "fed-sim:device"),
        "permissions.list_all",
        json!({}),
    )
    .await;
    capture(
        &mut cases,
        "served-revoke-sim-refused",
        Runtime::new(providers()),
        bound("callosum", "fed-sim:device"),
        "permissions.revoke",
        json!({"module":"z-hiring","id":"hire-1"}),
    )
    .await;
    for (name, method, params) in [
        (
            "served-list-catalog-unavailable",
            "permissions.list_all",
            json!({}),
        ),
        (
            "served-revoke-catalog-unavailable",
            "permissions.revoke",
            json!({"module":"z-hiring","id":"hire-1"}),
        ),
    ] {
        let mut runtime = Runtime::new(providers());
        runtime.catalog_available = false;
        capture(&mut cases, name, runtime, phone(), method, params).await;
    }
    let mut refused = providers();
    refused[0].revoke = Answer::Refused("grant_locked");
    capture(
        &mut cases,
        "served-revoke-provider-refused",
        Runtime::new(refused),
        phone(),
        "permissions.revoke",
        json!({"module":"z-hiring","id":"hire-1"}),
    )
    .await;
    let mut lost = providers();
    lost[0].revoke = Answer::Lost;
    capture(
        &mut cases,
        "served-revoke-reply-lost",
        Runtime::new(lost),
        phone(),
        "permissions.revoke",
        json!({"module":"z-hiring","id":"hire-1"}),
    )
    .await;
    capture(
        &mut cases,
        "served-revoke-routed",
        Runtime::new(providers()),
        phone(),
        "permissions.revoke",
        json!({"module":"z-hiring","id":"hire-1"}),
    )
    .await;
    cases
}

pub async fn served_files() -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    for (name, case) in served_cases().await {
        let canonical = serde_json::to_vec(&case).unwrap();
        use sha2::{Digest, Sha256};
        files.insert(
            format!("{name}.sha256"),
            format!("{:x}", Sha256::digest(&canonical)).into_bytes(),
        );
        files.insert(format!("{name}.jcs"), canonical);
        files.insert(
            format!("{name}.json"),
            (serde_json::to_string_pretty(&case).unwrap() + "\n").into_bytes(),
        );
    }
    files
}
