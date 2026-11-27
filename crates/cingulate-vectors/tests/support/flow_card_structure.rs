use std::{collections::BTreeMap, fs, sync::Arc, time::Duration};

use async_trait::async_trait;
use cingulate_store::ScopeIdentity;
use ck_cingulate::{ResolvedAgent, Resolver, ScopeLife, Stamp};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use subc_client_rs::{HandlerOutcome, RouteBindRequest, RouteHandle};
use subc_protocol::{
    peer::{OperatorApp, RouteBindPeer},
    scope::{ScopeAttributes, ScopeKind, ScopeStamp},
    BindIdentity, Principal, RouteTarget,
};

use crate::{
    support::{corpus, worker, Fixture},
    watch_pending::{Watch, INSTANCE},
};

pub const CORPUS: &str = "flow-card-structure-v1";
pub const CARDS: [&str; 2] = ["update", "install"];
pub const NOW: i64 = 1_800_000_000_000;

struct SampleResolver;

#[async_trait]
impl Resolver for SampleResolver {
    async fn resolve(&self, agent: &str) -> Result<Option<ResolvedAgent>, String> {
        assert_eq!(agent, "agent-maple");
        Ok(Some(ResolvedAgent {
            name: Some("Maple".into()),
            retired: false,
        }))
    }
}

pub fn bind(session: &str) -> RouteBindRequest {
    RouteBindRequest::new(
        RouteHandle::detached(1, 1),
        RouteTarget::ManagementSurface {
            module_id: "cingulate".into(),
        },
        BindIdentity::new(session, "ignored-session"),
    )
}

pub fn requester() -> Stamp {
    Stamp::from_bind(
        &bind("sample-supervisor")
            .with_principal(worker::principal("prefrontal-core"))
            .with_scope(
                ScopeStamp::new(
                    worker::principal("prefrontal-core"),
                    "sc_head_maple",
                    7,
                    ScopeKind::Head,
                )
                .with_attributes(ScopeAttributes::new().with_agent_id(Some("agent-maple".into())))
                .with_owner_authorized(true),
            ),
    )
}

pub fn phone_stamp() -> Stamp {
    Stamp::from_bind(
        &bind("fed-phone:11111111111111111111111111111111")
            .with_principal(worker::principal("callosum")),
    )
}

pub fn desktop() -> Stamp {
    Stamp::from_bind(
        &bind("sample-desktop")
            .with_principal(Principal::Direct)
            .with_peer(
                RouteBindPeer::new().with_operator_app(Some(OperatorApp::new("sample-desktop", 5))),
            ),
    )
}

pub async fn fixture() -> Fixture {
    let fixture = Fixture::open_with_resolver(
        None,
        cingulate_store::CHANGE_LOG_ROWS,
        Arc::new(SampleResolver),
    )
    .await;
    fixture
        .handler
        .set_access_runtime(worker::Runtime::new(Ok(ScopeLife::Live(ScopeIdentity {
            scope_ref: "sc_head_maple".into(),
            scope_epoch: 7,
            agent_id: Some("agent-maple".into()),
        }))));
    fixture.clock.set(NOW);
    fixture.pin_store_instance(INSTANCE);
    fixture
}

pub fn input(card: &str) -> Value {
    serde_json::from_slice(&fs::read(corpus(CORPUS).join(format!("{card}-request.json"))).unwrap())
        .unwrap()
}

pub async fn served(fixture: &Fixture, stamp: &Stamp, method: &str, params: Value) -> Vec<u8> {
    let request = serde_json::to_vec(&json!({"method":method,"params":params})).unwrap();
    match tokio::time::timeout(
        Duration::from_secs(1),
        fixture.handler.handle_stamped(stamp, &request),
    )
    .await
    .expect("surface operation within 1 s")
    {
        HandlerOutcome::Response(bytes) => bytes,
        other => panic!("{method}: {other:?}"),
    }
}

pub async fn generate() -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    for card in CARDS {
        let fixture = fixture().await;
        let phone = phone_stamp();
        let mut watch = Watch::open(&fixture.handler, &phone, json!({}));
        assert_eq!(watch.next_value().await["type"], "reread");
        let name = format!("{card}-request.json");
        let bytes = fs::read(corpus(CORPUS).join(&name)).unwrap();
        let id = fixture
            .request(&requester(), serde_json::from_slice(&bytes).unwrap())
            .await;
        assert_eq!(id, "el_1");
        files.insert(name, bytes);
        files.insert(
            format!("{card}-requester-get.json"),
            served(
                &fixture,
                &requester(),
                "consent.get",
                json!({"elicitation_id":id}),
            )
            .await,
        );
        for (surface, stamp) in [("phone", phone), ("desktop", desktop())] {
            for (suffix, method, params) in [
                ("get", "consent.get", json!({"elicitationId":id})),
                ("list", "consent.list_pending", json!({})),
            ] {
                files.insert(
                    format!("{card}-{surface}-{suffix}.json"),
                    served(&fixture, &stamp, method, params).await,
                );
            }
        }
        files.insert(format!("{card}-phone-watch.json"), watch.next().await);
        watch.unsubscribe();
        assert!(matches!(watch.outcome().await, HandlerOutcome::Streamed));
    }
    files.insert(
        "README.md".into(),
        fs::read(corpus(CORPUS).join("README.md")).unwrap(),
    );
    let sums: String = files
        .iter()
        .map(|(name, bytes)| format!("{:x}  {name}\n", Sha256::digest(bytes)))
        .collect();
    files.insert("SHA256SUMS".into(), sums.into_bytes());
    files
}
