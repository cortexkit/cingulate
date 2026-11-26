#[path = "support/flow_secrets.rs"]
mod flow_secrets;
#[path = "support/phone.rs"]
#[allow(dead_code)]
mod phone;
#[path = "support/secret_use.rs"]
mod secret_use;
mod support;
#[path = "support/watch_pending.rs"]
#[allow(dead_code)]
mod watch_pending;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use cingulate_store::State;
use flow_secrets::*;
use secret_use::{decoded, encoded, filer, fixture, phone, DEVICE, NOW};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

#[tokio::test]
async fn flow_install_secrets_filing_refusals_store_nothing() {
    for (name, request, code) in filing_refusals() {
        let f = fixture().await;
        let before = f.rows();
        let error = f
            .call(&filer("prefrontal-core", None), "consent.request", request)
            .await
            .unwrap_err();
        assert_eq!(error["code"], code, "{name}: {error}");
        assert_eq!(f.rows(), before, "{name}");
    }
}

#[tokio::test]
async fn flow_install_secrets_identity_is_closed_and_bounded() {
    let mut bad = Vec::new();
    for (key, value) in [
        ("flow_id", json!("")),
        ("flow_id", json!("x".repeat(257))),
        ("flow_id", json!("café")),
        ("flow_id", json!("line\n")),
        ("install_id", json!("")),
        ("install_id", json!("x".repeat(257))),
        ("install_id", json!("\u{7f}")),
        ("version", json!(0)),
        ("version", json!(-1)),
        ("version", json!(1.5)),
        ("version", json!(9_007_199_254_740_992_u64)),
        ("code_hash", json!("A".repeat(64))),
        ("code_hash", json!("a".repeat(63))),
        ("extra", json!(true)),
    ] {
        let mut r = body();
        r["flow_install"]["flow_identity"][key] = value;
        bad.push(r);
    }
    for field in ["flow_id", "version", "code_hash", "install_id"] {
        let mut r = body();
        r["flow_install"]["flow_identity"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        bad.push(r);
    }
    for value in [Value::Null, json!([]), json!({})] {
        let mut r = body();
        r["flow_install"]["flow_identity"] = value;
        bad.push(r);
    }
    let mut r = body();
    r["flow_install"].as_object_mut().unwrap().remove("secrets");
    bad.push(r);
    for request in bad {
        assert!(cingulate_decode::ConsentRequest::decode(request).is_err());
    }
    for version in [1, 9_007_199_254_740_991_u64] {
        let mut r = body();
        let mut identity = r["flow_install"]["flow_identity"].clone();
        identity["version"] = json!(version);
        identity["flow_id"] = json!(" ".repeat(256));
        identity["install_id"] = json!("~".repeat(256));
        r["flow_install"]["flow_identity"] = identity.clone();
        for i in 0..2 {
            alter_statement(&mut r, i, |s| s["flow"] = identity.clone());
        }
        assert!(cingulate_decode::ConsentRequest::decode(r).is_ok());
    }
}

#[tokio::test]
async fn flow_install_secrets_statements_are_closed_strict_and_canonical() {
    for count in [1, 16] {
        let mut r = body();
        let template = r["flow_install"]["secrets"][0].clone();
        let items: Vec<_> = (1..=count)
            .map(|n| {
                let mut item = template.clone();
                let id = format!("01HF7YAT{n:018}");
                let mut statement = decoded(&item["statement_b64url"]);
                statement["request_id"] = json!(id);
                item["request_id"] = json!(id);
                item["statement_b64url"] = encoded(&statement);
                item
            })
            .collect();
        r["flow_install"]["secrets"] = json!(items);
        assert!(cingulate_decode::ConsentRequest::decode(r).is_ok());
    }
    let mut bad = Vec::new();
    for value in [Value::Null, json!({}), json!([])] {
        let mut r = body();
        r["flow_install"]["secrets"] = value;
        bad.push(r);
    }
    for field in ["extra", "flow", "secret", "use"] {
        let mut r = body();
        alter_statement(&mut r, 0, |s| {
            if field == "extra" {
                s[field] = json!(true);
            } else {
                s[field]["extra"] = json!(true);
            }
        });
        bad.push(r);
    }
    for (key, value) in [
        ("v", json!("ck-secret-use-statement/v1")),
        ("request_id", json!("01HF7YAT000000000000000003")),
        ("durations", json!(["version", "version"])),
        ("request_expires_at_ms", json!(1.5)),
        ("request_expires_at_ms", json!(9_007_199_254_740_992_u64)),
    ] {
        let mut r = body();
        alter_statement(&mut r, 0, |s| s[key] = value);
        bad.push(r);
    }
    for field in [
        "v",
        "request_id",
        "member_id",
        "flow",
        "secret",
        "use",
        "durations",
        "request_expires_at_ms",
    ] {
        let mut r = body();
        alter_statement(&mut r, 0, |s| {
            s.as_object_mut().unwrap().remove(field);
        });
        bad.push(r);
    }
    let statement = decoded(&body()["flow_install"]["secrets"][0]["statement_b64url"]);
    let mut trailing_statement = statement.clone();
    // Find a canonical byte length that leaves base64 padding bits to corrupt.
    while serde_jcs::to_vec(&trailing_statement)
        .unwrap()
        .len()
        .is_multiple_of(3)
    {
        trailing_statement["secret"]["name"] = json!(format!(
            "{}x",
            trailing_statement["secret"]["name"].as_str().unwrap()
        ));
    }
    let text = encoded(&trailing_statement).as_str().unwrap().to_owned();
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let last = alphabet
        .iter()
        .position(|b| *b == *text.as_bytes().last().unwrap())
        .unwrap();
    let trailing = format!("{}{}", &text[..text.len() - 1], alphabet[last + 1] as char);
    for text in [
        format!("{}=", encoded(&statement).as_str().unwrap()),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec_pretty(&statement).unwrap()),
        URL_SAFE_NO_PAD.encode(GOLDEN.replacen("{", "{\"v\":\"duplicate\",", 1)),
        trailing,
    ] {
        let mut r = body();
        r["flow_install"]["secrets"][0]["statement_b64url"] = json!(text);
        bad.push(r);
    }
    let mut r = body();
    r["flow_install"]["secrets"][0]["extra"] = json!(true);
    bad.push(r);
    for request in bad {
        let f = fixture().await;
        let before = f.rows();
        assert!(
            f.call(
                &filer("prefrontal-core", None),
                "consent.request",
                request.clone()
            )
            .await
            .is_err(),
            "{request}"
        );
        assert_eq!(f.rows(), before);
    }
    let mut r = body();
    alter_statement(&mut r, 1, |s| {
        s["secret"]["kind"] = json!("site_login");
        s["use"] = json!({"origin":"https://x.test"});
    });
    assert!(cingulate_decode::ConsentRequest::decode(r).is_ok());
}

#[tokio::test]
async fn flow_install_secrets_deadline_without_expiry_and_earlier_statement() {
    for ttl in [None, Some(1_200_000), Some(1_000)] {
        let f = fixture().await;
        let mut request = body();
        if let Some(ttl) = ttl {
            request["expires_in_ms"] = json!(ttl);
        }
        request["on_expiry"] = json!("allow");
        alter_statement(&mut request, 1, |s| {
            s["request_expires_at_ms"] = json!(NOW + 300_000)
        });
        let sender = filer("prefrontal-core", None);
        let id = f.request(&sender, request).await;
        let expiry = NOW + ttl.unwrap_or(300_000).min(300_000);
        assert_eq!(f.stored_record(&id).expires_at_ms, Some(expiry));
        assert_eq!(f.stored_record(&id).request.on_expiry, "deny");
        f.clock.set(expiry);
        f.handler.timer_tick().await.unwrap();
        assert_eq!(f.stored_record(&id).state, State::Expired);
        assert_eq!(f.stored_record(&id).answered_choice_id, None);
        assert_eq!(
            f.call(&phone(DEVICE), "consent.answer", decline(&id))
                .await
                .unwrap_err()["code"],
            "elicitation_expired"
        );
    }
    let f = fixture().await;
    let mut r = body();
    for i in 0..2 {
        alter_statement(&mut r, i, |s| {
            s["request_expires_at_ms"] = json!(NOW + 86_400_000)
        });
    }
    let id = f.request(&filer("prefrontal-core", None), r).await;
    assert_eq!(f.stored_record(&id).expires_at_ms, Some(NOW + 86_400_000));
}

#[tokio::test]
async fn flow_install_secrets_allow_is_complete_and_phone_bound() {
    for (name, params, stamp) in answer_refusals() {
        let f = fixture().await;
        f.request(&filer("prefrontal-core", None), body()).await;
        let before = f.rows();
        assert!(
            f.call(&stamp, "consent.answer", params).await.is_err(),
            "{name}"
        );
        assert_eq!(f.rows(), before, "{name}");
    }
    let mut bad = Vec::new();
    for (key, value) in [
        ("duration", json!("hour")),
        ("grant_expires_at_ms", json!(1)),
        ("extra", json!(true)),
    ] {
        let mut e = envelope(&body());
        e[key] = value;
        bad.push(e);
    }
    let mut e = envelope(&body());
    e["items"][1] = e["items"][0].clone();
    bad.push(e);
    let mut e = envelope(&body());
    e["items"][1]["statement_sha256"] = json!("0".repeat(64));
    bad.push(e);
    let mut e = envelope(&body());
    e["items"][0]["duration"] = json!("version");
    bad.push(e);
    for e in bad {
        let f = fixture().await;
        let id = f.request(&filer("prefrontal-core", None), body()).await;
        assert!(f
            .call(&phone(DEVICE), "consent.answer", answer(&id, &e))
            .await
            .is_err());
        assert_eq!(f.stored_record(&id).state, State::Pending);
    }
    let f = fixture().await;
    let sender = filer("prefrontal-core", None);
    let id = f.request(&sender, body()).await;
    let mut e = envelope(&body());
    e["items"].as_array_mut().unwrap().reverse();
    let params = answer(&id, &e);
    assert_eq!(
        f.ok(&phone(DEVICE), "consent.answer", params.clone()).await["ok"],
        true
    );
    let record = f.stored_record(&id);
    assert_eq!(record.answered_device.as_deref(), Some(DEVICE));
    let binding = record.secret_use_answer.unwrap();
    assert_eq!(
        binding.envelope_sha256,
        Some(format!(
            "{:x}",
            Sha256::digest(
                URL_SAFE_NO_PAD
                    .decode(encoded(&e).as_str().unwrap())
                    .unwrap()
            )
        ))
    );
    assert!(binding.items.iter().all(|i| i.decision == "allow"));
    assert_eq!(
        f.ok(&phone(DEVICE), "consent.answer", params).await["alreadyAnswered"],
        true
    );
    e["issued_at_ms"] = json!(NOW + 1);
    assert_eq!(
        f.ok(&phone(DEVICE), "consent.answer", answer(&id, &e))
            .await["code"],
        "conflict"
    );
    assert_eq!(
        f.ok(&phone(DEVICE), "consent.answer", decline(&id)).await["code"],
        "conflict"
    );
    f.clock.set(NOW + 600_000);
    assert_eq!(
        f.call(
            &phone(DEVICE),
            "consent.answer",
            answer(&id, &envelope(&body()))
        )
        .await
        .unwrap_err()["code"],
        "elicitation_expired"
    );
    let f = fixture().await;
    let id = f.request(&sender, body()).await;
    f.ok(&phone(DEVICE), "consent.answer", decline(&id)).await;
    assert!(f
        .stored_record(&id)
        .secret_use_answer
        .unwrap()
        .items
        .iter()
        .all(|i| i.decision == "decline"));
    assert_eq!(
        f.ok(&phone(DEVICE), "consent.answer", decline(&id)).await["alreadyAnswered"],
        true
    );
}

#[tokio::test]
async fn flow_install_secrets_only_supported_phone_routes_see_or_answer() {
    let f = fixture().await;
    let sender = filer("prefrontal-core", None);
    let id = f.request(&sender, body()).await;
    for stamp in [
        support::operator(),
        support::stamp("reserved:aft"),
        phone("bad-fingerprint"),
        {
            let mut s = phone(DEVICE);
            s.harness = format!("fed-computer:{DEVICE}");
            s
        },
        {
            let mut s = phone(DEVICE);
            s.harness = "simulator".into();
            s
        },
    ] {
        let features = json!(["secret-use-approval"]);
        let list = f
            .call(
                &stamp,
                "consent.list_pending",
                json!({"clientFeatures":features}),
            )
            .await;
        if let Ok(list) = list {
            assert!(!list.to_string().contains("daily-fetch"));
        }
        assert!(f
            .call(
                &stamp,
                "consent.get",
                json!({"elicitationId":id,"clientFeatures":features})
            )
            .await
            .is_err());
        assert!(f
            .call(&stamp, "consent.answer", decline(&id))
            .await
            .is_err());
    }
    assert!(f
        .call(&phone(DEVICE), "consent.get", json!({"elicitationId":id}))
        .await
        .is_err());
    assert!(f
        .call(
            &phone(DEVICE),
            "consent.answer",
            json!({"elicitationId":id,"choiceId":"decline"})
        )
        .await
        .is_err());
    let list = f
        .ok(&phone(DEVICE), "consent.list_pending", json!({}))
        .await;
    assert!(!list.to_string().contains("daily-fetch"));
    let card = f
        .ok(
            &phone(DEVICE),
            "consent.get",
            json!({"elicitationId":id,"clientFeatures":["secret-use-approval"]}),
        )
        .await;
    assert_eq!(card["flowInstall"], body()["flow_install"]);
    assert_eq!(card["flowInstall"]["flow"], "daily-fetch");
    assert_eq!(
        f.ok(
            &phone(DEVICE),
            "consent.list_pending",
            json!({"clientFeatures":["secret-use-approval"]})
        )
        .await["records"][0]["elicitationId"],
        id
    );
}

#[tokio::test]
async fn basal_flow_install_secrets_require_the_approving_device() {
    let f = fixture().await;
    let id = f.request(&filer("basal", None), body()).await;
    let params = answer(&id, &envelope(&body()));
    let before = f.rows();
    let mut no_feature = params.clone();
    no_feature.as_object_mut().unwrap().remove("clientFeatures");
    assert_eq!(
        f.call(&phone(DEVICE), "consent.answer", no_feature)
            .await
            .unwrap_err()["code"],
        "elicitation_secret_use_approval_required"
    );
    for stamp in [
        support::operator(),
        phone("22222222222222222222222222222222"),
    ] {
        assert!(f
            .call(&stamp, "consent.answer", params.clone())
            .await
            .is_err());
    }
    assert_eq!(f.rows(), before);
    assert_eq!(f.stored_record(&id).state, State::Pending);

    assert_eq!(
        f.ok(&phone(DEVICE), "consent.answer", params).await["ok"],
        true
    );
    assert_eq!(
        f.stored_record(&id).answered_device.as_deref(),
        Some(DEVICE)
    );
}

#[tokio::test]
async fn flow_install_secrets_capability_and_filing_allowlist() {
    let manifest = serde_json::to_value(ck_cingulate::manifest()).unwrap();
    assert!(manifest["capabilities"]["provides"]
        .as_array()
        .unwrap()
        .contains(&json!("flow-install-secrets/v1")));
    let f = fixture().await;
    for module in ["aft", "cerebellum", "callosum"] {
        assert!(f
            .call(&filer(module, None), "consent.request", body())
            .await
            .is_err());
    }
    assert!(f.rows().iter().all(|table| table.is_empty()));
    let mut legacy = body();
    legacy["flow_install"]
        .as_object_mut()
        .unwrap()
        .remove("secrets");
    legacy["flow_install"]
        .as_object_mut()
        .unwrap()
        .remove("flow_identity");
    legacy["flow_install"]["future-opaque-key"] = json!({"anything":[1,true,null]});
    let id = f
        .request(&filer("prefrontal-core", None), legacy.clone())
        .await;
    assert_eq!(f.stored_record(&id).expires_at_ms, None);
    let card = f
        .ok(
            &support::operator(),
            "consent.get",
            json!({"elicitationId":id}),
        )
        .await;
    assert_eq!(card["flowInstall"], legacy["flow_install"]);
    assert_eq!(
        f.ok(
            &support::operator(),
            "consent.answer",
            json!({"elicitationId":id,"choiceId":"install"})
        )
        .await["ok"],
        true
    );
}

#[tokio::test]
async fn flow_install_secrets_vector_contract_bytes_and_checksums() {
    let root = support::corpus(CORPUS);
    let generated = generate().await;
    for (name, bytes) in &generated {
        assert_eq!(std::fs::read(root.join(name)).unwrap(), *bytes, "{name}");
    }
    let sums = std::fs::read_to_string(root.join("SHA256SUMS")).unwrap();
    let mut covered = std::collections::BTreeSet::new();
    for line in sums.lines() {
        let (hash, name) = line.split_once("  ").unwrap();
        assert_eq!(
            hash,
            format!(
                "{:x}",
                Sha256::digest(std::fs::read(root.join(name)).unwrap())
            ),
            "{name}"
        );
        assert!(covered.insert(name.to_owned()));
    }
    let mut expected: std::collections::BTreeSet<_> = generated.keys().cloned().collect();
    expected.insert("README.md".into());
    assert_eq!(covered, expected);
    let first = URL_SAFE_NO_PAD
        .decode(
            body()["flow_install"]["secrets"][0]["statement_b64url"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
    assert_eq!(first, GOLDEN.as_bytes());
    assert_eq!(
        format!("{:x}", Sha256::digest(first)),
        "bcf3f59a99c0beaa52366c6fa37af6a799460257215252d804ce7474240a7c57"
    );
}

#[tokio::test]
async fn flow_install_secrets_watch_filters_replay_settlements_and_pruned_ids() {
    use watch_pending::{Watch, INSTANCE};
    let f = fixture().await;
    f.pin_store_instance(INSTANCE);
    let id = f.request(&filer("prefrontal-core", None), body()).await;
    let mut visible = Watch::open(
        &f.handler,
        &phone(DEVICE),
        json!({"cursor":{"instance":INSTANCE,"version":0},"clientFeatures":["secret-use-approval"]}),
    );
    assert_eq!(visible.next_value().await["card"]["elicitationId"], id);
    let mut legacy = Watch::open(
        &f.handler,
        &phone(DEVICE),
        json!({"cursor":{"instance":INSTANCE,"version":0}}),
    );
    let mut unsupported = Watch::open(
        &f.handler,
        &phone("bad"),
        json!({"cursor":{"instance":INSTANCE,"version":0},"clientFeatures":["secret-use-approval"]}),
    );
    legacy.quiet().await;
    unsupported.quiet().await;
    f.ok(&phone(DEVICE), "consent.answer", decline(&id)).await;
    assert_eq!(visible.next_value().await["card"]["state"], "answered");
    f.clock.set(NOW + cingulate_store::RETENTION_MS + 1);
    f.ok(
        &phone(DEVICE),
        "consent.list_pending",
        json!({"clientFeatures":["secret-use-approval"]}),
    )
    .await;
    assert_eq!(visible.next_value().await["type"], "removed");
    legacy.quiet().await;
    unsupported.quiet().await;
    for watch in [visible, legacy, unsupported] {
        watch.unsubscribe();
        watch.outcome().await;
    }
}
