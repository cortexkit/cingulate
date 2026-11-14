use async_trait::async_trait;
use ck_cingulate::{Handler, Options, PermissionsCallError, PermissionsRuntime, Stamp};
use serde_json::{json, Value};
use std::sync::Arc;
use subc_client_rs::{consumer::CatalogList, HandlerOutcome};
use subc_protocol::{
    manifest::{Concurrency, ManagementOperation, ManagementOperationKind, ProviderRole},
    Principal,
};

struct Provider(Value);

#[async_trait]
impl PermissionsRuntime for Provider {
    async fn catalog(&self) -> Result<CatalogList, String> {
        let role = ProviderRole::ManagementSurface {
            operations: vec![ManagementOperation {
                name: "permissions.list".into(),
                kind: ManagementOperationKind::Query,
                description: None,
            }],
            config_schema: json!({}),
            observability: vec![],
            identity_scope: vec![],
            concurrency: Concurrency::ModuleManaged,
        };
        Ok(serde_json::from_value(json!({"generation":1,"modules":[{
            "module_id":"plexus","ready":true,"roles":[role],"control_ops":[],
            "capabilities":{"provides":["permissions/v1"],"requires":[],"must_never_reach":[]}
        }]}))
        .unwrap())
    }

    async fn call(
        &self,
        module: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, PermissionsCallError> {
        assert_eq!(module, "plexus");
        assert_eq!(method, "permissions.list");
        assert_eq!(params, json!({}));
        Ok(self.0.clone())
    }
}

async fn listing(row: Value, params: Value) -> Value {
    listing_reply(json!({"grants":[row]}), params).await
}

async fn listing_reply(reply: Value, params: Value) -> Value {
    let scratch = tempfile::tempdir().unwrap();
    let handler = Handler::new(Options {
        data_home: scratch.path().to_str().unwrap().into(),
        sweep_timer: false,
        ..Options::default()
    });
    handler.set_permissions_runtime(Arc::new(Provider(reply)));
    let stamp = Stamp::from_principal(Some(&Principal::Direct));
    let request =
        serde_json::to_vec(&json!({"method":"permissions.list_all","params":params})).unwrap();
    match handler.handle_stamped(&stamp, &request).await {
        HandlerOutcome::Response(bytes) => {
            serde_json::from_slice::<Value>(&bytes).unwrap()["result"].clone()
        }
        other => panic!("unexpected permissions reply: {other:?}"),
    }
}

#[tokio::test]
async fn permissions_list_all_relays_federated_deny_provider_vector() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test-vectors/permissions-v1/provider-list-federated-deny.json");
    let vector: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(vector["method"], "permissions.list");
    let reply = vector["reply"].clone();
    let grants = reply["grants"].as_array().unwrap();
    assert!(grants
        .iter()
        .any(|row| row["holder"]["kind"] == "federated" && row["effect"] == "deny"));

    let result = listing_reply(reply.clone(), json!({})).await;
    let modules = result["modules"].as_array().unwrap();
    assert_eq!(modules.len(), 1);
    assert_eq!(modules[0]["module"], "plexus");
    assert_eq!(modules[0]["status"], "ok");
    assert_eq!(modules[0]["grants"], reply["grants"]);
}

#[tokio::test]
async fn permissions_list_all_relays_effect_unchanged_and_includes_it_in_version() {
    let grant = json!({"id":"grt_09",
        "holder":{"kind":"federated","peer_fingerprint":"a".repeat(32),
            "pairing_id":"pairing-invented-1","peer_class":"computer",
            "selector":"exact_claim","claim":{"kind":"agent","agent_id":"Sage"},
            "label":"Juniper · Sage"},
        "module":"plexus","subject_label":"Mail","scope":"mail_read",
        "scope_label":"read your mail","granted_at_ms":1000000,"card_ref":"el_9"});
    let before = listing(grant.clone(), json!({})).await;
    assert_eq!(before["modules"][0]["grants"], json!([grant]));
    assert!(!before["version"].as_str().unwrap().is_empty());
    let unchanged = listing(grant.clone(), json!({"since_version":before["version"]})).await;
    assert_eq!(
        unchanged,
        json!({"version":before["version"],"unchanged":true})
    );

    // Keep every other byte identical to isolate effect's contribution to version.
    for effect in ["deny", "future-effect"] {
        let mut changed = grant.clone();
        changed["effect"] = json!(effect);
        let after = listing(changed.clone(), json!({"since_version":before["version"]})).await;
        assert_eq!(after["modules"][0]["grants"], json!([changed]));
        assert_eq!(after["modules"][0]["grants"][0]["effect"], effect);
        assert!(!after["version"].as_str().unwrap().is_empty());
        assert_ne!(before["version"], after["version"]);
        assert!(after.get("unchanged").is_none());
    }
}
