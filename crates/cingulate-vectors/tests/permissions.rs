#[path = "support/permissions.rs"]
mod permissions;
mod support;

use ck_cingulate::{manifest, Stamp};
use permissions::{
    bound, call, catalog_module, handler, holder_row, phone, row, Answer, Provider, Runtime,
};
use serde_json::{json, Value};
use std::{fs, sync::Arc};
use subc_protocol::{
    manifest::{ManagementOperationKind, ProviderRole},
    Principal,
};

async fn holder_snapshot(grants: Value, catalog: Vec<Provider>, params: Value) -> Value {
    let mut providers = vec![Provider::new("claustrum", grants)];
    providers.extend(catalog);
    call(
        &handler(Arc::new(Runtime::new(providers))),
        &phone(),
        "permissions.list_all",
        params,
    )
    .await
    .unwrap()
}

#[tokio::test(start_paused = true)]
async fn federated_rows_relay_labels_filtering_and_liveness_failure() {
    let cases: std::collections::BTreeMap<_, _> =
        permissions::served_cases().await.into_iter().collect();
    let rows = cases["federated-holders"]["reply"]["modules"][0]["grants"]
        .as_array()
        .unwrap();
    let expected = [
        "Juniper · Sage",
        "Juniper · worker for Sage",
        "Juniper · worker",
        "Juniper · flow Morning-digest v3",
        "Juniper · any program",
        "Juniper · everything on it",
    ];
    assert_eq!(rows.len(), expected.len());
    for (row, label) in rows.iter().zip(expected) {
        assert_eq!(row["holder"]["kind"], "federated");
        assert_eq!(row["holder"]["label"], label);
        assert!(row.get("agent_id").is_none());
        assert!(row["holder"].get("agent_id").is_none());
        assert_ne!(row["holder"]["claim"]["kind"], "run");
    }
    assert_eq!(
        cases["federated-holders-agent-filtered"]["reply"]["modules"][0]["grants"],
        json!([])
    );
    let unavailable = &cases["federated-provider-liveness-unavailable"]["reply"]["modules"][0];
    assert_eq!(unavailable["status"], "unreachable");
    assert!(unavailable.get("grants").is_none());
    assert_eq!(cases["federated-revoke"]["reply"]["outcome"], "revoked");
    let named = cases["federated-holders-named"]["reply"]["modules"][0]["grants"]
        .as_array()
        .unwrap();
    let expected_named = [
        "Juniper · “Remote Sage” (Sage)",
        "Juniper · worker for “Remote Sage” (Sage)",
        "Juniper · worker",
        "Juniper · flow “Daily digest” (Morning-digest) v3",
        "Juniper · any program",
        "Juniper · everything on it",
    ];
    for (row, label) in named.iter().zip(expected_named) {
        assert_eq!(row["holder"]["label"], label);
    }
    assert_eq!(named[0]["holder"]["claim"]["agent_name"], "Remote Sage");
    assert_eq!(named[1]["holder"]["claim"]["agent_name"], "Remote Sage");
    assert_eq!(named[3]["holder"]["claim"]["flow_name"], "Daily digest");
    assert_ne!(
        cases["federated-holders"]["reply"]["version"],
        cases["federated-holders-named"]["reply"]["version"]
    );
}

#[tokio::test]
async fn permissions_module_holder_gets_catalog_display_name() {
    let grant = holder_row(
        "named",
        json!({"kind":"module","module_id":"indexer","extra":7}),
    );
    let mut expected = grant.clone();
    expected["holder"]["label"] = json!("Web Indexer");
    let reply = holder_snapshot(
        json!([grant]),
        vec![catalog_module("indexer", Some("Web Indexer"))],
        json!({}),
    )
    .await;
    assert_eq!(
        reply["modules"],
        json!([{"module":"claustrum","status":"ok","grants":[expected]}])
    );
}

#[tokio::test]
async fn permissions_module_holder_provider_label_wins() {
    let grants = json!([
        holder_row(
            "label",
            json!({"kind":"module","module_id":"indexer","label":"Provider name"})
        ),
        holder_row(
            "empty-label",
            json!({"kind":"module","module_id":"indexer","label":""})
        ),
        holder_row(
            "null-label",
            json!({"kind":"module","module_id":"indexer","label":null})
        ),
        holder_row(
            "future-label",
            json!({"kind":"module","module_id":"indexer","label":{"future":true}})
        ),
    ]);
    let reply = holder_snapshot(
        grants.clone(),
        vec![catalog_module("indexer", Some("Web Indexer"))],
        json!({}),
    )
    .await;
    assert_eq!(reply["modules"][0]["grants"], grants);
}

#[tokio::test]
async fn permissions_module_holder_without_catalog_name_stays_unlabeled() {
    let grants = json!([
        holder_row("unknown", json!({"kind":"module","module_id":"unknown"})),
        holder_row("unnamed", json!({"kind":"module","module_id":"unnamed"})),
        holder_row("missing-id", json!({"kind":"module"})),
    ]);
    let reply = holder_snapshot(
        grants.clone(),
        vec![catalog_module("unnamed", None)],
        json!({}),
    )
    .await;
    assert_eq!(reply["modules"][0]["grants"], grants);
}

#[tokio::test]
async fn permissions_non_module_holders_stay_untouched() {
    let grants = json!([
        holder_row("enrolled", json!({"kind":"enrolled","name":"kitchen-tablet","module_id":"indexer"})),
        holder_row("enrolled-label", json!({"kind":"enrolled","name":"desk-tablet","label":"Kitchen tablet","module_id":"indexer"})),
        holder_row("agent", json!({"kind":"agent","agent_id":"agent-alf","module_id":"indexer"})),
        holder_row("future", json!({"kind":"future","module_id":"indexer"})),
        {"id":"missing-holder","extra":true},
        {"id":"null-holder","holder":null},
        {"id":"scalar-holder","holder":"indexer"},
        "future-row",
    ]);
    let reply = holder_snapshot(
        grants.clone(),
        vec![catalog_module("indexer", Some("Web Indexer"))],
        json!({}),
    )
    .await;
    assert_eq!(reply["modules"][0]["grants"], grants);
}

#[tokio::test]
async fn permissions_module_display_name_changes_version() {
    let grants = json!([holder_row(
        "named",
        json!({"kind":"module","module_id":"indexer"})
    )]);
    let before = holder_snapshot(
        grants.clone(),
        vec![catalog_module("indexer", Some("Web Indexer"))],
        json!({}),
    )
    .await;
    let unchanged = holder_snapshot(
        grants.clone(),
        vec![catalog_module("indexer", Some("Web Indexer"))],
        json!({"since_version":before["version"]}),
    )
    .await;
    assert_eq!(
        unchanged,
        json!({"version":before["version"],"unchanged":true})
    );
    let after = holder_snapshot(
        grants,
        vec![catalog_module("indexer", Some("Renamed Indexer"))],
        json!({"since_version":before["version"]}),
    )
    .await;
    assert_ne!(before["version"], after["version"]);
    assert_eq!(
        after["modules"][0]["grants"][0]["holder"]["label"],
        "Renamed Indexer"
    );
}

#[tokio::test]
async fn permissions_guard_discovery_uses_capability_not_module_name() {
    let mut decoy = Provider::new("cerebellum", json!([]));
    decoy.capabilities = vec!["other/v1".into()];
    let runtime = Arc::new(Runtime::new(vec![
        Provider::new(
            "unrelated-name",
            json!([row("unrelated-name", "b", "agent-alf")]),
        ),
        decoy,
        Provider::new("entorhinal", json!([row("entorhinal", "a", "agent-alf")])),
    ]));
    let handler = handler(runtime.clone());
    let reply = call(&handler, &phone(), "permissions.list_all", json!({}))
        .await
        .unwrap();
    assert_eq!(
        reply["modules"],
        json!([
            {"module":"entorhinal","status":"ok","grants":[row("entorhinal", "a", "agent-alf")]},
            {"module":"unrelated-name","status":"ok","grants":[row("unrelated-name", "b", "agent-alf")]},
        ])
    );
    assert_eq!(runtime.calls.lock().unwrap().len(), 2);
    assert!(runtime
        .calls
        .lock()
        .unwrap()
        .iter()
        .all(|(module, _, _)| module != "cerebellum"));
}

#[tokio::test(start_paused = true)]
async fn permissions_guard_timeout_is_a_named_gap() {
    let mut slow = Provider::new("entorhinal", json!([]));
    slow.list = Answer::Hang;
    let runtime = Arc::new(Runtime::new(vec![
        slow,
        Provider::new("cerebellum", json!([row("cerebellum", "one", "agent-alf")])),
    ]));
    let handler = handler(runtime.clone());
    let reply = call(&handler, &phone(), "permissions.list_all", json!({}))
        .await
        .unwrap();
    assert_eq!(
        reply["modules"],
        json!([
            {"module":"cerebellum","status":"ok","grants":[row("cerebellum", "one", "agent-alf")]},
            {"module":"entorhinal","status":"unreachable"},
        ])
    );
    assert_eq!(runtime.calls.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn permissions_not_ready_refused_unsupported_and_unreadable_are_named_gaps() {
    let mut providers = vec![];
    for name in [
        "not-ready",
        "refuses",
        "unsupported",
        "unknown-method",
        "bad-reply",
    ] {
        let mut p = Provider::new(name, json!([]));
        match name {
            "not-ready" => p.ready = false,
            "refuses" => p.list = Answer::Refused("permissions_operator_required"),
            "unsupported" => p.supports_list = false,
            "unknown-method" => p.list = Answer::Refused("unknown_method"),
            "bad-reply" => p.list = Answer::Reply(json!({"not_grants":[]})),
            _ => unreachable!(),
        }
        providers.push(p);
    }
    let runtime = Arc::new(Runtime::new(providers));
    let reply = call(
        &handler(runtime.clone()),
        &phone(),
        "permissions.list_all",
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(
        reply["modules"],
        json!([
            {"module":"bad-reply","status":"unreachable"},
            {"module":"not-ready","status":"unreachable"},
            {"module":"refuses","status":"unreachable"},
            {"module":"unknown-method","status":"unsupported"},
            {"module":"unsupported","status":"unsupported"},
        ])
    );
    assert_eq!(runtime.calls.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn permissions_revoke_routes_once_to_the_holding_provider_and_preserves_codes() {
    for (answer, expected) in [
        (
            Answer::Reply(json!({"outcome":"revoked"})),
            json!({"outcome":"revoked"}),
        ),
        (
            Answer::Reply(json!({"outcome":"revoked","stopping":true})),
            json!({"outcome":"stopping"}),
        ),
        (
            Answer::Reply(json!({"outcome":"not_found"})),
            json!({"outcome":"already_gone"}),
        ),
        (
            Answer::Refused("grant_locked"),
            json!({"outcome":"refused","code":"grant_locked"}),
        ),
        (Answer::Lost, json!({"outcome":"module_unreachable"})),
        (
            Answer::Reply(json!({"garbled":true})),
            json!({"outcome":"module_unreachable"}),
        ),
    ] {
        let mut target = Provider::new("unrelated-name", json!([]));
        target.revoke = answer;
        let runtime = Arc::new(Runtime::new(vec![
            Provider::new("cerebellum", json!([])),
            target,
        ]));
        let reply = call(
            &handler(runtime.clone()),
            &phone(),
            "permissions.revoke",
            json!({"module":"unrelated-name","id":"held-grant"}),
        )
        .await
        .unwrap();
        assert_eq!(reply, expected);
        assert_eq!(
            *runtime.calls.lock().unwrap(),
            [(
                "unrelated-name".into(),
                "permissions.revoke".into(),
                json!({"id":"held-grant"})
            )]
        );
    }
}

#[tokio::test(start_paused = true)]
async fn permissions_revoke_timeout_is_unknown_and_never_retried() {
    let mut p = Provider::new("entorhinal", json!([]));
    p.revoke = Answer::Hang;
    let runtime = Arc::new(Runtime::new(vec![p]));
    let reply = call(
        &handler(runtime.clone()),
        &phone(),
        "permissions.revoke",
        json!({"module":"entorhinal","id":"one"}),
    )
    .await
    .unwrap();
    assert_eq!(reply, json!({"outcome":"module_unreachable"}));
    assert_eq!(runtime.calls.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn permissions_catalog_failure_refuses_before_any_provider_call() {
    let mut runtime = Runtime::new(vec![Provider::new("entorhinal", json!([]))]);
    runtime.catalog_available = false;
    let runtime = Arc::new(runtime);
    let handler = handler(runtime.clone());
    for (method, params) in [
        ("permissions.list_all", json!({})),
        (
            "permissions.revoke",
            json!({"module":"entorhinal","id":"one"}),
        ),
    ] {
        assert_eq!(
            call(&handler, &phone(), method, params).await.unwrap_err(),
            json!({"code":"permissions_unavailable","message":"permissions catalog is unavailable"})
        );
    }
    assert!(runtime.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn permissions_non_provider_and_not_ready_revoke_never_send() {
    let mut p = Provider::new("configured", json!([]));
    p.ready = false;
    let mut decoy = Provider::new("cerebellum", json!([]));
    decoy.capabilities.clear();
    let runtime = Arc::new(Runtime::new(vec![p, decoy]));
    let handler = handler(runtime.clone());
    for (module, expected) in [
        ("configured", json!({"outcome":"module_unreachable"})),
        (
            "cerebellum",
            json!({"outcome":"refused","code":"not_a_provider"}),
        ),
        (
            "missing",
            json!({"outcome":"refused","code":"not_a_provider"}),
        ),
    ] {
        assert_eq!(
            call(
                &handler,
                &phone(),
                "permissions.revoke",
                json!({"module":module,"id":"one"})
            )
            .await
            .unwrap(),
            expected
        );
    }
    assert!(runtime.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn permissions_harness_vectors_admit_sim_lists_but_only_phone_mutations() {
    let fixture = support::Fixture::open(None).await;
    let runtime = Arc::new(Runtime::new(vec![Provider::new(
        "entorhinal",
        json!([row("entorhinal", "one", "agent-alf")]),
    )]));
    fixture.handler.set_permissions_runtime(runtime.clone());
    for line in include_str!("../../../test-vectors/device-class-harness/harness.jsonl").lines() {
        let vector: Value = serde_json::from_str(line).unwrap();
        for principal in ["callosum", "plexus"] {
            let stamp = bound(principal, vector["harness"].as_str().unwrap());
            let phone = principal == "callosum" && vector["class"] == "phone";
            let sim = principal == "callosum" && vector["class"] == "sim";
            assert_eq!(stamp.operator_surface, phone);
            assert_eq!(stamp.sim_surface, sim);
            for (method, params, allowed) in [
                ("permissions.list_all", json!({}), phone || sim),
                (
                    "permissions.revoke",
                    json!({"module":"entorhinal","id":"one"}),
                    phone,
                ),
            ] {
                let result = call(&fixture.handler, &stamp, method, params).await;
                if allowed {
                    assert!(result.is_ok());
                } else {
                    assert_eq!(result.unwrap_err()["code"], "permissions_operator_required");
                }
            }
            for (method, params) in [
                ("consent.list_pending", json!({})),
                (
                    "consent.answer",
                    json!({"elicitationId":"absent","choiceId":"decline"}),
                ),
            ] {
                let result = call(&fixture.handler, &stamp, method, params).await;
                if phone {
                    assert!(result.is_ok());
                } else {
                    assert_eq!(result.unwrap_err()["code"], "elicitation_operator_required");
                }
            }
        }
    }
    assert_eq!(runtime.calls.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn permissions_refuses_body_authority_and_admits_direct_with_its_documented_caveat() {
    let runtime = Arc::new(Runtime::new(vec![Provider::new("entorhinal", json!([]))]));
    let handler = handler(runtime.clone());
    for principal in [
        None,
        Some(Principal::Unverified),
        Some(Principal::Reserved {
            module_id: "entorhinal".into(),
        }),
        Some(Principal::Reserved {
            module_id: "callosum".into(),
        }),
    ] {
        let stamp = Stamp::from_principal(principal.as_ref());
        for method in ["permissions.list_all", "permissions.revoke"] {
            assert_eq!(call(&handler, &stamp, method, json!({"module":"entorhinal","id":"one","operator_surface":true,"sim_surface":true,"principal":"direct"})).await.unwrap_err()["code"], "permissions_operator_required");
        }
    }
    assert!(runtime.calls.lock().unwrap().is_empty());
    let direct = Stamp::from_principal(Some(&Principal::Direct));
    assert_eq!(
        call(
            &handler,
            &direct,
            "permissions.revoke",
            json!({"module":"entorhinal","id":"one"})
        )
        .await
        .unwrap(),
        json!({"outcome":"revoked"})
    );
    assert!(call(&handler, &direct, "permissions.list_all", json!({}))
        .await
        .is_ok());
}

#[tokio::test]
async fn permissions_filters_agents_and_hashes_the_current_snapshot_for_conditional_reads() {
    let runtime = Arc::new(Runtime::new(vec![Provider::new(
        "entorhinal",
        json!([
            row("entorhinal", "a", "agent-alf"),
            row("entorhinal", "b", "other-agent")
        ]),
    )]));
    let handler = handler(runtime.clone());
    let full = call(&handler, &phone(), "permissions.list_all", json!({}))
        .await
        .unwrap();
    let filtered = call(
        &handler,
        &phone(),
        "permissions.list_all",
        json!({"agent_id":"agent-alf"}),
    )
    .await
    .unwrap();
    assert_eq!(
        filtered["modules"][0]["grants"],
        json!([row("entorhinal", "a", "agent-alf")])
    );
    assert_ne!(full["version"], filtered["version"]);
    assert_eq!(
        runtime.calls.lock().unwrap()[1].2,
        json!({"agent_id":"agent-alf"})
    );
    let unchanged = call(
        &handler,
        &phone(),
        "permissions.list_all",
        json!({"agent_id":"agent-alf","since_version":filtered["version"]}),
    )
    .await
    .unwrap();
    assert_eq!(
        unchanged,
        json!({"version":filtered["version"],"unchanged":true})
    );
    let changed = call(
        &handler,
        &phone(),
        "permissions.list_all",
        json!({"since_version":filtered["version"]}),
    )
    .await
    .unwrap();
    assert_eq!(changed, full);
}

#[test]
fn permissions_manifest_has_query_and_mutate_without_claiming_provider_capability() {
    let manifest = manifest();
    assert!(!manifest
        .capabilities
        .unwrap()
        .provides
        .contains(&"permissions/v1".into()));
    let ProviderRole::ManagementSurface { operations, .. } = &manifest.provides[0] else {
        panic!("management role missing")
    };
    for (name, kind) in [
        ("permissions.list_all", ManagementOperationKind::Query),
        ("permissions.revoke", ManagementOperationKind::Mutate),
    ] {
        assert_eq!(
            operations.iter().find(|op| op.name == name).unwrap().kind,
            kind
        );
    }
}

#[tokio::test]
async fn permissions_original_aggregator_vectors_match_real_handler_replies() {
    let grants = support::vector("permissions-v1", "provider-list.json")["reply"]["grants"].clone();
    // The handler fixture filters by `agent_id` on the `agent_id` field, so it
    // can replay only the rows a real provider would filter the same way.
    let grants: Value = Value::Array(
        grants
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row.get("agent_id").is_some())
            .cloned()
            .collect(),
    );
    assert!(
        grants.as_array().unwrap().len() < 8,
        "some rows are filtered"
    );
    let mut tested = 0;
    for entry in fs::read_dir(permissions::root()).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_stem().unwrap().to_str().unwrap();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json")
            || name.starts_with("served-")
            || name.starts_with("federated-")
            || name == "list-all-holder-labels"
        {
            continue;
        }
        let vector: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        if vector["method"] != "permissions.list_all" && vector["request"].get("module").is_none() {
            continue;
        }
        let mut cerebellum = Provider::new("cerebellum", grants.clone());
        cerebellum.revoke = match name {
            "revoke-stopping" => Answer::Reply(json!({"outcome":"revoked","stopping":true})),
            "revoke-already-gone" => Answer::Reply(json!({"outcome":"not_found"})),
            _ => Answer::Reply(json!({"outcome":"revoked"})),
        };
        let mut providers = vec![cerebellum];
        if name == "list-all-unreachable" || name == "revoke-module-unreachable" {
            let mut unreachable = Provider::new("plexus", json!([]));
            unreachable.list = Answer::Lost;
            unreachable.revoke = Answer::Lost;
            providers.push(unreachable);
        }
        if name == "list-all-unsupported" {
            let mut unsupported = Provider::new("aft", json!([]));
            unsupported.supports_list = false;
            providers.push(unsupported);
        }
        let stamp = Stamp {
            principal: vector["route"]["principal"].as_str().map(str::to_owned),
            operator_surface: vector["route"]["operator_surface"]
                .as_bool()
                .unwrap_or(false),
            ..Default::default()
        };
        let result = call(
            &handler(Arc::new(Runtime::new(providers))),
            &stamp,
            vector["method"].as_str().unwrap(),
            vector["request"].clone(),
        )
        .await;
        if let Some(reply) = vector.get("reply") {
            assert_eq!(&result.unwrap(), reply, "{name}");
        } else {
            assert_eq!(result.unwrap_err(), vector["error"], "{name}");
        }
        tested += 1;
    }
    assert_eq!(tested, 14);
}

#[tokio::test(start_paused = true)]
async fn permissions_served_vectors_regenerate_byte_for_byte() {
    let generated = permissions::served_files().await;
    assert_eq!(generated.len(), 48);
    for (name, bytes) in generated {
        assert_eq!(
            fs::read(permissions::root().join(&name)).unwrap(),
            bytes,
            "served bytes: {name}"
        );
    }
}
