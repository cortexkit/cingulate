use std::{collections::BTreeMap, fs, time::Duration};

use ck_cingulate::{ScopeLife, Stamp};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use subc_client_rs::{HandlerOutcome, RouteBindRequest, RouteHandle};
use subc_protocol::{
    peer::{OperatorApp, RouteBindPeer},
    scope::{ScopeKind, ScopeParent, ScopeStamp},
    BindIdentity, Principal, RouteTarget,
};

use crate::{
    support::{corpus, worker, Fixture},
    watch_pending::{Watch, INSTANCE},
};

#[path = "secret_use.rs"]
mod secret_use;

pub const CORPUS: &str = "local-secret-cards-served-v1";
pub const CORPUS_V1_1: &str = "local-secret-cards-served-v1.1";
pub const CORPUS_V1_2: &str = "local-secret-cards-served-v1.2";
pub const CARDS: [&str; 7] = [
    "sudo-typed",
    "sudo-vault",
    "ssh-agent",
    "ssh-vault",
    "ssh-passphrase",
    "git-push",
    "git-fetch",
];
const FILED_AT_MS: i64 = 1_800_000_000_000;

fn requester_stamp() -> Stamp {
    Stamp::from_bind(
        &RouteBindRequest::new(
            RouteHandle::detached(1, 1),
            RouteTarget::ManagementSurface {
                module_id: "cingulate".into(),
            },
            BindIdentity::new("test-harness", "ignored-session"),
        )
        .with_principal(worker::principal("aft"))
        .with_scope(
            ScopeStamp::new(
                worker::principal("prefrontal-core"),
                "fake-agent",
                7,
                ScopeKind::Worker,
            )
            .with_parent(Some(ScopeParent::new(
                worker::principal("prefrontal-core"),
                "sc_head_alf",
                7,
            )))
            .with_owner_authorized(true),
        ),
    )
}

fn phone_stamp() -> Stamp {
    Stamp::from_bind(
        &RouteBindRequest::new(
            RouteHandle::detached(1, 1),
            RouteTarget::ManagementSurface {
                module_id: "cingulate".into(),
            },
            BindIdentity::new(
                "fed-phone:11111111111111111111111111111111",
                "ignored-session",
            ),
        )
        .with_principal(worker::principal("callosum")),
    )
}

fn desktop_stamp() -> Stamp {
    Stamp::from_bind(
        &RouteBindRequest::new(
            RouteHandle::detached(1, 1),
            RouteTarget::ManagementSurface {
                module_id: "cingulate".into(),
            },
            BindIdentity::new("local-desktop", "ignored-session"),
        )
        .with_principal(Principal::Direct)
        .with_peer(
            RouteBindPeer::new().with_operator_app(Some(OperatorApp::new("sample-desktop", 5))),
        ),
    )
}

async fn served(fixture: &Fixture, surface: &Stamp, method: &str, params: Value) -> Vec<u8> {
    let request = serde_json::to_vec(&json!({"method":method,"params":params})).unwrap();
    match tokio::time::timeout(
        Duration::from_secs(1),
        fixture.handler.handle_stamped(surface, &request),
    )
    .await
    .expect("surface operation within 1 s")
    {
        HandlerOutcome::Response(bytes) => bytes,
        other => panic!("{method}: {other:?}"),
    }
}

#[allow(dead_code)]
pub async fn generate(corpus_name: &str) -> BTreeMap<String, Vec<u8>> {
    generate_cards(corpus_name, &CARDS).await
}

pub async fn generate_cards(corpus_name: &str, cards: &[&str]) -> BTreeMap<String, Vec<u8>> {
    assert!(matches!(corpus_name, CORPUS | CORPUS_V1_1 | CORPUS_V1_2));
    let root = corpus(corpus_name);
    let mut files = BTreeMap::new();
    let desktop = desktop_stamp();
    let phone = phone_stamp();
    let features = json!({"clientFeatures":["secret-use-approval"]});
    for &card in cards {
        let input_name = format!("{card}-request.json");
        let input = fs::read(root.join(&input_name)).unwrap();
        let params: Value = serde_json::from_slice(&input).unwrap();
        let vault = params["kind"] == "secret_use";
        // The v1.1 and v1.2 vault statements bind a head scope, not the ordinary worker.
        // Keep route identity fixed independently of the request body.
        let requester = match (corpus_name, card) {
            (CORPUS_V1_1 | CORPUS_V1_2, "sudo-vault") => secret_use::filer("aft", Some("agent-A")),
            (CORPUS_V1_1 | CORPUS_V1_2, "ssh-vault") => secret_use::filer("aft", None),
            _ => requester_stamp(),
        };
        let fixture = if matches!(corpus_name, CORPUS_V1_1 | CORPUS_V1_2) && vault {
            secret_use::fixture().await
        } else {
            Fixture::open(None).await
        };
        fixture
            .handler
            .set_access_runtime(worker::Runtime::new(Ok(ScopeLife::Live(worker::head()))));
        fixture
            .clock
            .set(if matches!(corpus_name, CORPUS_V1_1 | CORPUS_V1_2) {
                secret_use::NOW
            } else {
                FILED_AT_MS
            });
        fixture.pin_store_instance(INSTANCE);

        let mut desktop_watch =
            (!vault).then(|| Watch::open(&fixture.handler, &desktop, features.clone()));
        let mut phone_watch = Watch::open(&fixture.handler, &phone, features.clone());
        let initial = phone_watch.next().await;
        if let Some(watch) = &mut desktop_watch {
            assert_eq!(initial, watch.next().await);
        }
        assert_eq!(
            serde_json::from_slice::<Value>(&initial).unwrap()["type"],
            "reread"
        );

        let id = fixture.request(&requester, params).await;
        assert_eq!(id, "el_1", "{card}");
        files.insert(input_name, input);

        // Capture the handler's bytes, including envelopes, without reserializing.
        for (suffix, method, params) in [
            ("list-pending", "consent.list_pending", features.clone()),
            (
                "get-pending",
                "consent.get",
                json!({"elicitationId":id,"clientFeatures":["secret-use-approval"]}),
            ),
        ] {
            let bytes = served(&fixture, &phone, method, params.clone()).await;
            let response: Value = serde_json::from_slice(&bytes).unwrap();
            if suffix == "list-pending" {
                let records = response["result"]["records"].as_array().unwrap();
                assert_eq!(records.len(), 1, "pending phone card: {card}");
                assert_eq!(records[0]["elicitationId"], id);
            } else {
                assert_eq!(response["result"]["elicitationId"], id);
            }
            if !vault {
                let desktop_bytes = served(&fixture, &desktop, method, params).await;
                if suffix == "list-pending" {
                    let desktop_response: Value = serde_json::from_slice(&desktop_bytes).unwrap();
                    assert_eq!(
                        response["result"]["records"],
                        desktop_response["result"]["records"]
                    );
                    assert_eq!(
                        response["result"]["revision"],
                        format!(
                            "{}:secret-use",
                            desktop_response["result"]["revision"].as_str().unwrap()
                        )
                    );
                } else {
                    assert_eq!(bytes, desktop_bytes, "desktop and phone {method}: {card}");
                }
                files.insert(format!("{card}-desktop-{suffix}.json"), desktop_bytes);
            }
            files.insert(format!("{card}-{suffix}.json"), bytes);
        }
        let push = phone_watch.next().await;
        if let Some(watch) = &mut desktop_watch {
            assert_eq!(push, watch.next().await, "watch push: {card}");
        }
        files.insert(format!("{card}-watch-created.json"), push);
        phone_watch.unsubscribe();
        if let Some(watch) = desktop_watch {
            watch.unsubscribe();
            assert!(matches!(watch.outcome().await, HandlerOutcome::Streamed));
        }
        assert!(matches!(
            phone_watch.outcome().await,
            HandlerOutcome::Streamed
        ));
    }
    files.insert(
        "README.md".into(),
        fs::read(root.join("README.md")).unwrap(),
    );
    let sums: String = files
        .iter()
        .map(|(name, bytes)| format!("{:x}  {name}\n", Sha256::digest(bytes)))
        .collect();
    files.insert("SHA256SUMS".into(), sums.into_bytes());
    files
}
