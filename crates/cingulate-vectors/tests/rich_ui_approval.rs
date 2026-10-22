#[allow(dead_code)]
#[path = "support/phone.rs"]
mod phone;
#[path = "support/rich_ui.rs"]
mod rich_ui;
mod support;
#[allow(dead_code)]
#[path = "support/watch_pending.rs"]
mod watch_pending;

use cingulate_decode::ConsentRequest;
use rich_ui::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use support::{stamp, Fixture};

#[test]
fn rich_ui_body_is_typed_and_fail_closed() {
    ConsentRequest::decode(body()).unwrap();
    for (field, value) in [
        ("approvalDigest", json!("sha256:bad")),
        (
            "manifestDigest",
            json!(format!("sha256:{}", "A".repeat(64))),
        ),
        ("capabilityDigest", json!("a".repeat(64))),
        ("provider", json!("Weather_tools")),
        ("provider", json!("")),
        ("device", json!("A".repeat(32))),
        ("device", json!("a".repeat(64))),
        ("extra", json!(true)),
    ] {
        let mut bad = body();
        bad["rich_ui_approval"][field] = value;
        assert!(ConsentRequest::decode(bad).is_err(), "{field}");
    }
    for field in [
        "approvalDigest",
        "manifestDigest",
        "capabilityDigest",
        "provider",
        "device",
    ] {
        let mut bad = body();
        bad["rich_ui_approval"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(ConsentRequest::decode(bad).is_err(), "missing {field}");
    }
    for (field, value) in [
        ("default", json!("approve")),
        ("on_expiry", json!("allow")),
        ("rich_ui_approval", Value::Null),
        ("flow_install", json!({})),
        ("kind", json!("consent")),
        (
            "options",
            json!([{"id":"deny","label":"No","effect":"decline"}]),
        ),
    ] {
        let mut bad = body();
        bad[field] = value;
        assert!(ConsentRequest::decode(bad).is_err(), "{field}");
    }
    let mut bad = body();
    bad["options"][1]["effect"] = json!("grant");
    assert!(ConsentRequest::decode(bad).is_err());
}

#[tokio::test]
async fn only_occipital_requests_rich_ui_without_storing_refusals() {
    let f = Fixture::open(None).await;
    for principal in [
        "reserved:prefrontal-core",
        "reserved:basal",
        "direct",
        "reserved:callosum",
    ] {
        let before = f.rows();
        let error = f
            .call(&stamp(principal), "consent.request", body())
            .await
            .unwrap_err();
        assert_eq!(error["code"], "elicitation_requester_refused");
        assert_eq!(f.rows(), before);
    }
    f.request(&stamp("reserved:occipital"), body()).await;
}

#[tokio::test]
async fn rich_ui_answers_require_exact_digest_without_writes() {
    let f = Fixture::open(None).await;
    let id = f.request(&stamp("reserved:occipital"), body()).await;
    for choice in ["approve", "deny"] {
        for binding in [Some(json!("sha256:wrong")), Some(Value::Null), None] {
            if choice == "deny" && binding.is_none() {
                continue;
            }
            let mut params = answer(&id, choice);
            params.as_object_mut().unwrap().remove("bindDigest");
            if let Some(binding) = binding {
                params["bindDigest"] = binding;
            }
            let before = f.rows();
            assert_eq!(
                f.call(&phone(DEVICE), "consent.answer", params)
                    .await
                    .unwrap_err()["code"],
                "approval_digest_mismatch"
            );
            assert_eq!(f.rows(), before);
        }
    }
    let result = f
        .ok(&phone(DEVICE), "consent.answer", answer(&id, "approve"))
        .await;
    assert_eq!(result["ok"], true);
    assert_eq!(
        f.stored_record(&id).answered_device.as_deref(),
        Some(DEVICE)
    );
    let id = f.request(&stamp("reserved:occipital"), body()).await;
    let mut decline = answer(&id, "deny");
    decline.as_object_mut().unwrap().remove("bindDigest");
    assert_eq!(
        f.ok(&phone(DEVICE), "consent.answer", decline).await["ok"],
        true
    );
}

#[tokio::test]
async fn other_kinds_reject_bind_digest_and_audit_fed_answers() {
    let f = Fixture::open(None).await;
    let id = f.request(&support::owner(), support::request_body()).await;
    let choice = f.stored_record(&id).request.default;
    let before = f.rows();
    let err = f
        .call(&phone(DEVICE), "consent.answer", answer(&id, &choice))
        .await
        .unwrap_err();
    assert_eq!(err["code"], "invalid_params");
    assert!(err["message"].as_str().unwrap().contains("bindDigest"));
    assert_eq!(f.rows(), before);
    f.ok(
        &phone(DEVICE),
        "consent.answer",
        json!({"elicitationId":id,"choiceId":choice}),
    )
    .await;
    assert_eq!(
        f.stored_record(&id).answered_device.as_deref(),
        Some(DEVICE)
    );
}

#[tokio::test]
async fn rich_ui_device_is_private_on_list_get_and_answer() {
    let f = Fixture::open(None).await;
    let id = f.request(&stamp("reserved:occipital"), body()).await;
    for route in [phone(OTHER), phone("bad"), phone(&DEVICE.to_uppercase())] {
        let features = json!({"clientFeatures":["rich-ui-approval"]});
        assert_eq!(
            f.ok(&route, "consent.list_pending", features).await["records"],
            json!([])
        );
        let get = json!({"elicitationId":id,"clientFeatures":["rich-ui-approval"]});
        assert_eq!(
            f.call(&route, "consent.get", get.clone()).await,
            f.call(
                &route,
                "consent.get",
                json!({"elicitationId":"unknown","clientFeatures":["rich-ui-approval"]})
            )
            .await
        );
        let before = f.rows();
        assert_eq!(
            f.ok(&route, "consent.answer", answer(&id, "approve")).await,
            f.ok(&route, "consent.answer", answer("unknown", "approve"))
                .await
        );
        assert_eq!(f.rows(), before);
        let continuation = json!({"clientFeatures":["rich-ui-approval"], "continuation":{"elicitation_id":id,"expires_at_ms":1}});
        assert!(f
            .call(&route, "consent.list_pending", continuation)
            .await
            .is_err());
    }
    assert_eq!(get(&f, &id).await["elicitationId"], id);
    let list = f
        .ok(
            &phone(DEVICE),
            "consent.list_pending",
            json!({"clientFeatures":["rich-ui-approval"]}),
        )
        .await;
    assert_eq!(list["records"].as_array().unwrap().len(), 1);
    let other = f
        .ok(
            &phone(OTHER),
            "consent.list_pending",
            json!({"clientFeatures":["rich-ui-approval"],"if_revision":list["revision"]}),
        )
        .await;
    assert_eq!(other["records"], json!([]));
}

#[tokio::test]
async fn rich_ui_feature_gate_hides_cards_and_refuses_answers() {
    let f = Fixture::open(None).await;
    let id = f.request(&stamp("reserved:occipital"), body()).await;
    assert_eq!(
        f.ok(&phone(DEVICE), "consent.list_pending", json!({}))
            .await["records"],
        json!([])
    );
    assert_eq!(
        f.call(&phone(DEVICE), "consent.get", json!({"elicitationId":id}))
            .await
            .unwrap_err()["code"],
        "elicitation_not_found"
    );
    let mut params = answer(&id, "approve");
    params.as_object_mut().unwrap().remove("clientFeatures");
    assert_eq!(
        f.call(&phone(DEVICE), "consent.answer", params)
            .await
            .unwrap_err()["code"],
        "elicitation_rich_ui_approval_required"
    );
    assert_eq!(f.stored_record(&id).state, cingulate_store::State::Pending);
}

#[tokio::test]
async fn rich_ui_revoke_requires_requester_and_is_idempotent() {
    let f = Fixture::open(None).await;
    let owner = stamp("reserved:occipital");
    let id = f.request(&owner, body()).await;
    let params = json!({"elicitationId":id});
    assert_eq!(
        f.call(&owner, "consent.revoke", params.clone())
            .await
            .unwrap_err()["code"],
        "elicitation_not_granted"
    );
    f.ok(&phone(DEVICE), "consent.answer", answer(&id, "approve"))
        .await;
    for stranger in [
        stamp("reserved:basal"),
        ck_cingulate::Stamp {
            bind_session: "other".into(),
            ..owner.clone()
        },
    ] {
        let before = f.rows();
        assert_eq!(
            f.call(&stranger, "consent.revoke", params.clone())
                .await
                .unwrap_err()["code"],
            "elicitation_not_owner"
        );
        assert_eq!(f.rows(), before);
    }
    f.clock.set(12_000);
    let revoked = f.ok(&owner, "consent.revoke", params.clone()).await;
    assert_eq!(revoked["state"], "revoked");
    assert_eq!(revoked["revoked_at_ms"], 12_000);
    f.clock.set(13_000);
    assert_eq!(f.ok(&owner, "consent.revoke", params).await, revoked);
    assert_eq!(get(&f, &id).await["revokedAtMs"], 12_000);
    assert_eq!(
        f.ok(&owner, "consent.withdraw", json!({"elicitation_id":id}))
            .await,
        revoked
    );
    let declined = f.request(&owner, body()).await;
    f.ok(&phone(DEVICE), "consent.answer", answer(&declined, "deny"))
        .await;
    assert_eq!(
        f.call(&owner, "consent.revoke", json!({"elicitationId":declined}))
            .await
            .unwrap_err()["code"],
        "elicitation_not_granted"
    );
    let other = f.request(&support::owner(), support::request_body()).await;
    assert_eq!(
        f.call(
            &support::owner(),
            "consent.revoke",
            json!({"elicitationId":other})
        )
        .await
        .unwrap_err()["code"],
        "elicitation_unknown_kind"
    );
}

#[tokio::test]
async fn rich_ui_revoke_not_granted_is_terminal_and_names_each_state() {
    for state in ["pending", "declined", "expired", "withdrawn"] {
        let f = Fixture::open(None).await;
        let owner = stamp("reserved:occipital");
        let mut request = body();
        if state == "expired" {
            request["expires_in_ms"] = json!(1);
        }
        let id = f.request(&owner, request).await;
        match state {
            "declined" => {
                f.ok(&phone(DEVICE), "consent.answer", answer(&id, "deny"))
                    .await;
            }
            "expired" => {
                f.clock.set(support::NOW + 2);
                assert_eq!(get(&f, &id).await["state"], "expired");
            }
            "withdrawn" => {
                f.ok(&owner, "consent.withdraw", json!({"elicitation_id":id}))
                    .await;
            }
            _ => {}
        }
        let before = f.rows();
        for _ in 0..2 {
            assert_eq!(
                f.call(&owner, "consent.revoke", json!({"elicitationId":id}))
                    .await
                    .unwrap_err(),
                json!({
                    "code":"elicitation_not_granted",
                    "message":format!("elicitation_not_granted: revoke requires a granted card; this card is {state}")
                }),
                "{state}"
            );
            assert_eq!(f.rows(), before, "{state}");
        }
    }
}

#[tokio::test]
async fn rich_ui_never_notifies_on_create_answer_withdraw_or_revoke() {
    let f = Fixture::open(None).await;
    let owner = stamp("reserved:occipital");
    let id = f.request(&owner, body()).await;
    f.ok(&phone(DEVICE), "consent.answer", answer(&id, "approve"))
        .await;
    f.ok(&owner, "consent.revoke", json!({"elicitationId":id}))
        .await;
    let id = f.request(&owner, body()).await;
    f.ok(&owner, "consent.withdraw", json!({"elicitation_id":id}))
        .await;
    let mut expiring = body();
    expiring["expires_in_ms"] = json!(1);
    let id = f.request(&owner, expiring).await;
    f.clock.set(support::NOW + 2);
    let expired = get(&f, &id).await;
    assert_eq!(expired["state"], "expired");
    assert!(expired.get("answeredChoiceId").is_none());
    assert!(f.payloads().is_empty());
}

#[tokio::test]
async fn rich_ui_samples_are_served_bytes_and_hashes_match() {
    let root = support::corpus("rich-ui-approval-v1");
    for (file, expected) in samples().await {
        let actual: Value =
            serde_json::from_slice(&std::fs::read(root.join(file)).unwrap()).unwrap();
        assert_eq!(actual, expected);
    }
    let sums = std::fs::read_to_string(root.join("SHA256SUMS")).unwrap();
    let mut count = 0;
    for line in sums.lines() {
        let (digest, file) = line.split_once("  ").unwrap();
        let bytes = std::fs::read(root.join(file)).unwrap();
        assert_eq!(format!("{:x}", Sha256::digest(bytes)), digest, "{file}");
        count += 1;
    }
    assert_eq!(count, std::fs::read_dir(root).unwrap().count() - 1);
}

#[tokio::test]
async fn rich_ui_watch_filters_replay_live_revoke_and_tombstones() {
    use watch_pending::{Watch, INSTANCE};
    let f = Fixture::open(None).await;
    f.pin_store_instance(INSTANCE);
    let owner = stamp("reserved:occipital");
    let id = f.request(&owner, body()).await;
    let mut visible = Watch::open(
        &f.handler,
        &phone(DEVICE),
        json!({"cursor":{"instance":INSTANCE,"version":0},"clientFeatures":["rich-ui-approval"]}),
    );
    assert_eq!(visible.next_value().await["card"], get(&f, &id).await);
    let mut wrong = Watch::open(
        &f.handler,
        &phone(OTHER),
        json!({"cursor":{"instance":INSTANCE,"version":0},"clientFeatures":["rich-ui-approval"]}),
    );
    let mut legacy = Watch::open(
        &f.handler,
        &phone(DEVICE),
        json!({"cursor":{"instance":INSTANCE,"version":0}}),
    );
    wrong.quiet().await;
    legacy.quiet().await;
    f.ok(&phone(DEVICE), "consent.answer", answer(&id, "approve"))
        .await;
    assert_eq!(visible.next_value().await["card"]["state"], "answered");
    f.ok(&owner, "consent.revoke", json!({"elicitationId":id}))
        .await;
    assert_eq!(visible.next_value().await["card"]["state"], "revoked");
    wrong.quiet().await;
    legacy.quiet().await;
    f.clock
        .set(support::NOW + cingulate_store::RETENTION_MS + 1);
    f.ok(
        &phone(DEVICE),
        "consent.list_pending",
        json!({"clientFeatures":["rich-ui-approval"]}),
    )
    .await;
    assert_eq!(visible.next_value().await["type"], "removed");
    wrong.quiet().await;
    legacy.quiet().await;
    for watch in [visible, wrong, legacy] {
        watch.unsubscribe();
        watch.outcome().await;
    }
}

#[tokio::test]
async fn permission_answers_reject_bind_digest_and_record_fed_device_audit() {
    use subc_protocol::{
        scope::{ScopeAttributes, ScopeKind, ScopeStamp},
        Principal,
    };
    let f = Fixture::open(None).await;
    let requester = ck_cingulate::Stamp {
        scope: Some(
            ScopeStamp::new(
                Principal::Reserved {
                    module_id: "prefrontal-core".into(),
                },
                "head",
                7,
                ScopeKind::Head,
            )
            .with_attributes(ScopeAttributes::new().with_agent_id(Some("agent-alf".into())))
            .with_owner_authorized(true),
        ),
        ..stamp("reserved:aft")
    };
    let reply = f.ok(&requester, "consent.request", json!({
        "kind":"tool_permission", "call_key":"call-device-audit", "tool":"read", "operation":"read",
        "paths":["/tmp/file"], "reason":"Inspect file", "args_hash":"hash-device-audit", "pattern":"/tmp/file"
    })).await;
    let id = reply["elicitation_id"].as_str().unwrap();
    let params = json!({"elicitationId":id,"choiceId":"deny","bindDigest":DIGEST});
    assert_eq!(
        f.call(&phone(DEVICE), "consent.answer", params)
            .await
            .unwrap_err()["code"],
        "invalid_params"
    );
    f.ok(
        &phone(DEVICE),
        "consent.answer",
        json!({"elicitationId":id,"choiceId":"deny"}),
    )
    .await;
    let raw: String = f
        .sql()
        .query_row(
            "SELECT record_json FROM access_asks WHERE elicitation_id=?1",
            [id],
            |row| row.get(0),
        )
        .unwrap();
    let record: cingulate_store::AccessRecord = serde_json::from_str(&raw).unwrap();
    assert_eq!(record.attempts.len(), 1);
    assert_eq!(record.attempts[0].device.as_deref(), Some(DEVICE));
}
