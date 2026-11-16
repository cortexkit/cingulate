#[path = "support/manifest_binding.rs"]
mod manifest_binding;
#[path = "support/phone.rs"]
#[allow(dead_code)]
mod phone;
#[path = "support/secret_use.rs"]
mod secret_use;
mod support;
#[path = "support/watch_pending.rs"]
#[allow(dead_code)]
mod watch_pending;

use cingulate_decode::ConsentRequest;
use manifest_binding::*;
use serde_json::{json, Value};

#[test]
fn manifest_binding_statement_fields_are_strict_and_named() {
    assert!(ConsentRequest::decode(body()).is_ok());
    let original = sample("statement")["statement"].clone();
    let cases = [
        ("extra", json!(true)),
        ("v", json!("wrong")),
        ("installation_permissions", json!({"contents":"owner"})),
        ("manifest_version", json!(30)),
        ("manifest_sha256", json!("A".repeat(64))),
        ("prior_sha256", json!("F".repeat(64))),
    ];
    for (field, value) in cases {
        let mut statement = original.clone();
        statement[field] = value;
        let mut request = body();
        request["manifest_binding"]["statement_b64url"] = secret_use::encoded(&statement);
        let error = ConsentRequest::decode(request).unwrap_err();
        assert_eq!(error.code, "elicitation_invalid_request");
        assert!(error.message.contains(field), "{field}: {error}");
    }
    for field in original.as_object().unwrap().keys() {
        let mut statement = original.clone();
        statement.as_object_mut().unwrap().remove(field);
        let mut request = body();
        request["manifest_binding"]["statement_b64url"] = secret_use::encoded(&statement);
        assert!(ConsentRequest::decode(request)
            .unwrap_err()
            .message
            .contains(field));
    }
}

#[test]
fn manifest_binding_filing_is_one_statement_and_no_authored_text_or_expiry() {
    use base64::Engine;
    let mut request = body();
    request["manifest_binding"]["statement_b64url"] =
        json!(base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec_pretty(&sample("statement")["statement"]).unwrap()));
    assert!(ConsentRequest::decode(request)
        .unwrap_err()
        .message
        .contains("statement_b64url"));
    for field in [
        "expires_at_ms",
        "expires_in_ms",
        "title",
        "prompt",
        "agent_id",
    ] {
        let mut request = body();
        request[field] = json!(1);
        assert!(ConsentRequest::decode(request)
            .unwrap_err()
            .message
            .contains("manifest_binding filing has invalid fields"));
    }
    let mut request = body();
    request["manifest_binding"] = json!([
        request["manifest_binding"].clone(),
        request["manifest_binding"].clone()
    ]);
    assert!(ConsentRequest::decode(request).is_err());
}

#[tokio::test]
async fn manifest_binding_only_plexus_files_with_matching_or_absent_stamped_agent() {
    for caller in [
        "direct",
        "plexus",
        "enrolled:plexus",
        "reserved:aft",
        "reserved:cerebellum",
        "reserved:broca",
        "reserved:prefrontal-core",
        "reserved:callosum",
    ] {
        let f = fixture().await;
        let error = f
            .call(&support::stamp(caller), "consent.request", body())
            .await
            .unwrap_err();
        assert_eq!(error["code"], "elicitation_requester_refused", "{caller}");
    }
    for stamp in [filer(), secret_use::filer("plexus", Some(AGENT))] {
        let f = fixture().await;
        let id = f.request(&stamp, body()).await;
        assert_eq!(f.stored_record(&id).expires_at_ms, Some(EXPIRY));
    }
    for stamp in [
        secret_use::filer("plexus", Some("other-agent")),
        ck_cingulate::Stamp {
            carried_agent_id: "other-agent".into(),
            ..filer()
        },
    ] {
        let f = fixture().await;
        let error = f.call(&stamp, "consent.request", body()).await.unwrap_err();
        assert_eq!(error["code"], "elicitation_invalid_request");
        assert!(error["message"].as_str().unwrap().contains("agent_id"));
    }
}

#[tokio::test]
async fn manifest_binding_same_statement_reuses_card_without_widening_ownership() {
    let f = fixture().await;
    let id = f.request(&filer(), body()).await;
    let before = f.rows();
    assert_eq!(f.request(&filer(), body()).await, id);
    assert_eq!(f.rows(), before);
    let other_session = ck_cingulate::Stamp {
        bind_session: "another-session".into(),
        ..filer()
    };
    assert_eq!(
        f.call(&other_session, "consent.request", body())
            .await
            .unwrap_err()["code"],
        "elicitation_request_conflict"
    );
    assert_eq!(
        f.call(&other_session, "consent.get", json!({"elicitation_id":id}))
            .await
            .unwrap_err()["code"],
        "elicitation_not_owner"
    );
    let mut statement = sample("statement")["statement"].clone();
    statement["proposal_id"] = json!("01HF7YAT000000000000000002");
    let mut new = body();
    new["manifest_binding"]["statement_b64url"] = secret_use::encoded(&statement);
    assert_ne!(f.request(&filer(), new).await, id);
}

#[tokio::test]
async fn manifest_binding_phone_only_feature_digest_device_and_expiry_are_enforced() {
    let f = fixture().await;
    let id = f.request(&filer(), body()).await;
    for stamp in [
        support::operator(),
        support::stamp("direct"),
        ck_cingulate::Stamp {
            operator_surface: true,
            local_operator: true,
            ..support::stamp("direct")
        },
        ck_cingulate::Stamp {
            harness: format!("fed-computer:{}", secret_use::DEVICE),
            ..support::operator()
        },
    ] {
        assert!(f
            .call(&stamp, "consent.answer", answer(&id, "allow"))
            .await
            .is_err());
    }
    let old_phone = f
        .ok(
            &phone(),
            "consent.list_pending",
            json!({"clientFeatures":["secret-use-approval"]}),
        )
        .await;
    assert!(!old_phone.to_string().contains("manifest_binding"));
    assert!(f
        .call(&phone(), "consent.get", json!({"elicitationId":id}))
        .await
        .is_err());
    for (field, value) in [
        ("statement_sha256", json!("0".repeat(64))),
        ("device_fingerprint", json!("2".repeat(32))),
        ("proposal_id", json!("01HF7YAT000000000000000002")),
        ("v", json!("ck-secret-use-envelope/v1")),
        ("decision", json!("decline")),
    ] {
        let mut envelope = sample("envelope")["envelope"].clone();
        envelope[field] = value;
        let mut params = answer(&id, "allow");
        params["envelope_b64url"] = secret_use::encoded(&envelope);
        let error = f
            .call(&phone(), "consent.answer", params)
            .await
            .unwrap_err();
        assert!(
            error["message"].as_str().unwrap().contains(field),
            "{error}"
        );
    }
    f.ok(&phone(), "consent.answer", answer(&id, "allow")).await;
    let record = f
        .ok(&filer(), "consent.get", json!({"elicitation_id":id}))
        .await;
    assert_eq!(record["manifest_binding_answer"]["decision"], "allow");
    assert_eq!(
        record["manifest_binding_answer"]["statement_sha256"],
        sample("statement")["statement_sha256"]
    );
    assert_eq!(record["answered_device"], secret_use::DEVICE);
    let f = fixture().await;
    let id = f.request(&filer(), body()).await;
    f.clock.set(EXPIRY);
    assert_eq!(
        f.call(&phone(), "consent.answer", answer(&id, "allow"))
            .await
            .unwrap_err()["code"],
        "elicitation_expired"
    );
    assert_eq!(
        f.call(&filer(), "consent.request", body())
            .await
            .unwrap_err()["code"],
        "elicitation_expired"
    );
}

#[tokio::test]
async fn manifest_binding_card_projection_decline_and_push_privacy() {
    for choice in ["allow", "decline", "withdraw"] {
        let f = fixture().await;
        let id = f.request(&filer(), body()).await;
        let card = f
            .ok(
                &phone(),
                "consent.get",
                json!({"elicitationId":id,"clientFeatures":["manifest-binding-approval"]}),
            )
            .await;
        let source = sample("statement");
        for field in [
            "repo",
            "agent_id",
            "app_slug",
            "installation_permissions",
            "statement_sha256",
            "statement_b64url",
        ] {
            assert_eq!(
                &card["manifestBinding"][field],
                source.get(field).unwrap_or(&source["statement"][field])
            );
        }
        assert_eq!(card["subject"]["label"], "Plexus");
        assert!(card["subject"].get("agentId").is_none());
        if choice == "withdraw" {
            f.ok(&filer(), "consent.withdraw", json!({"elicitation_id":id}))
                .await;
        } else {
            f.ok(&phone(), "consent.answer", answer(&id, choice)).await;
        }
        let notices: Vec<String> = f
            .sql()
            .prepare("SELECT body_json FROM access_notices")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(notices.len() >= 2);
        for notice in notices {
            assert!(
                !notice.contains("Octo-Owner")
                    && !notice.contains("permissions")
                    && !notice.contains("contents")
                    && !notice.contains("statement"),
                "{notice}"
            );
        }
    }
}

#[tokio::test]
async fn manifest_binding_vectors_are_handler_generated_and_hash_pinned() {
    use sha2::{Digest, Sha256};
    let generated = generate().await;
    let case = |name: &str| -> Value { serde_json::from_slice(&generated[name]).unwrap() };
    let withdraw = case("filer-withdraw.json");
    let repeat = case("filer-withdraw-repeat.json");
    let refused = case("other-withdraw-refused.json");
    let phone = case("phone-get-withdrawn.json");
    assert_eq!(refused["reply"]["error"]["code"], "elicitation_not_owner");
    assert_eq!(withdraw["reply"]["result"]["state"], "withdrawn");
    assert_eq!(repeat["reply"], withdraw["reply"]);
    assert_eq!(phone["reply"]["result"]["state"], "withdrawn");
    assert!(phone["reply"]["result"].get("choices").is_none());
    let sums = std::fs::read_to_string(support::corpus(CORPUS).join("SHA256SUMS")).unwrap();
    let pinned: std::collections::BTreeSet<_> = sums
        .lines()
        .map(|line| line.split_once("  ").unwrap().1)
        .collect();
    for (name, bytes) in generated {
        assert!(pinned.contains(name.as_str()), "{name} is not hash-pinned");
        assert_eq!(
            std::fs::read(support::corpus(CORPUS).join(&name)).unwrap(),
            bytes,
            "{name}"
        );
    }
    for line in sums.lines() {
        let (hash, name) = line.split_once("  ").unwrap();
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(std::fs::read(support::corpus(CORPUS).join(name)).unwrap())
            ),
            hash
        );
    }
    let mismatch: Value = serde_json::from_slice(
        &std::fs::read(support::corpus(CORPUS).join("phone-refuse-mismatch.json")).unwrap(),
    )
    .unwrap();
    assert_ne!(
        mismatch["reply"]["result"]["manifestBinding"]["repo"],
        sample("statement")["statement"]["repo"]
    );
}

#[tokio::test]
async fn manifest_binding_watch_hides_unsupported_replay_live_and_pruned_removal_ids() {
    use watch_pending::{Watch, INSTANCE};
    let f = fixture().await;
    f.pin_store_instance(INSTANCE);
    let id = f.request(&filer(), body()).await;
    let mut visible = Watch::open(
        &f.handler,
        &phone(),
        json!({"cursor":{"instance":INSTANCE,"version":0},"clientFeatures":["manifest-binding-approval"]}),
    );
    assert_eq!(visible.next_value().await["card"]["elicitationId"], id);
    let mut old = Watch::open(
        &f.handler,
        &phone(),
        json!({"cursor":{"instance":INSTANCE,"version":0},"clientFeatures":["secret-use-approval"]}),
    );
    old.quiet().await;
    f.ok(&phone(), "consent.answer", answer(&id, "allow")).await;
    assert_eq!(visible.next_value().await["card"]["state"], "answered");
    f.clock.set(NOW + cingulate_store::RETENTION_MS + 1);
    f.ok(
        &phone(),
        "consent.list_pending",
        json!({"clientFeatures":["manifest-binding-approval"]}),
    )
    .await;
    assert_eq!(visible.next_value().await["type"], "removed");
    old.quiet().await;
    for watch in [visible, old] {
        watch.unsubscribe();
        watch.outcome().await;
    }
}
