use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use cingulate_store::ScopeIdentity;
use ck_cingulate::{AccessRuntime, NoticeResult, ScopeLife, Stamp};
use serde_json::Value;
use subc_client_rs::{RouteBindRequest, RouteHandle};
use subc_protocol::{
    scope::{ScopeAttributes, ScopeKind, ScopeParent, ScopeStamp},
    BindIdentity, Principal, RouteTarget,
};

pub fn principal(module: &str) -> Principal {
    Principal::Reserved {
        module_id: module.into(),
    }
}

pub fn head() -> ScopeIdentity {
    ScopeIdentity {
        scope_ref: "sc_head_alf".into(),
        scope_epoch: 7,
        agent_id: Some("agent-alf".into()),
    }
}

pub fn stamp(module: &str, kind: ScopeKind) -> Stamp {
    let worker = kind == ScopeKind::Worker;
    Stamp::from_bind(
        &RouteBindRequest::new(
            RouteHandle::detached(1, 1),
            RouteTarget::ManagementSurface {
                module_id: "cingulate".into(),
            },
            BindIdentity::new("/scratch", "test-harness", "untrusted-session"),
        )
        .with_principal(if module == "direct" {
            Principal::Direct
        } else {
            principal(module)
        })
        .with_scope(
            ScopeStamp::new(
                principal("prefrontal-core"),
                if worker { "sc_worker" } else { "sc_head_alf" },
                if worker { 3 } else { 7 },
                kind,
            )
            .with_parent(
                worker.then(|| ScopeParent::new(principal("prefrontal-core"), "sc_head_alf", 7)),
            )
            .with_attributes(if worker {
                ScopeAttributes::new()
            } else {
                ScopeAttributes::new().with_agent_id(Some("agent-alf".into()))
            })
            .with_owner_authorized(true),
        ),
    )
}

pub struct Runtime {
    life: Result<ScopeLife, String>,
    pub described: Mutex<Vec<ScopeIdentity>>,
    pub notices: Mutex<Vec<Value>>,
}

impl Runtime {
    pub fn new(life: Result<ScopeLife, String>) -> Arc<Self> {
        Arc::new(Self {
            life,
            described: Mutex::new(Vec::new()),
            notices: Mutex::new(Vec::new()),
        })
    }
}

#[async_trait]
impl AccessRuntime for Runtime {
    async fn describe(&self, scope: &ScopeIdentity) -> Result<ScopeLife, String> {
        self.described.lock().unwrap().push(scope.clone());
        self.life.clone()
    }

    async fn notice(&self, body: Value) -> NoticeResult {
        self.notices.lock().unwrap().push(body);
        NoticeResult::Delivered
    }
}
