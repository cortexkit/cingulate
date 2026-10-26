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
use secret_use::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs};

#[tokio::test]
async fn secret_use_vault_vectors_file_and_bind_answers() {
    for case in ["approval", "batch-omitted", "batch-unknown"] {
        let f = fixture().await;
        let sender = filer("aft", Some("agent-A"));
        let id = f.request(&sender, body(case)).await;
        let before = f.rows();
        if case == "batch-unknown" {
            assert!(f
                .call(&phone(DEVICE), "consent.answer", answer(&id, case))
                .await
                .is_err());
            assert_eq!(f.rows(), before);
            continue;
        }
        assert_eq!(
            f.ok(&phone(DEVICE), "consent.answer", answer(&id, case))
                .await["ok"],
            true
        );
        let record = f
            .ok(&sender, "consent.get", json!({"elicitation_id":id}))
            .await;
        let binding = &record["secret_use_answer"];
        assert_eq!(binding["decision"], "allow");
        assert_eq!(binding["duration"], "hour");
        assert_eq!(binding["envelope_sha256"], vector(case)["envelope_sha256"]);
        assert_eq!(binding["items"][0]["decision"], "allow");
        if case == "batch-omitted" {
            assert_eq!(
                binding["items"][1],
                json!({"request_id":"01HF7YAT000000000000000002","decision":"decline"})
            );
        }
        let feed = f.ok(&sender, "consent.answers", json!({"since":0})).await;
        assert_eq!(feed["records"][0]["secret_use_answer"], *binding);
        assert_eq!(
            f.ok(&phone(DEVICE), "consent.answer", answer(&id, case))
                .await["alreadyAnswered"],
            true
        );
    }
}

#[tokio::test]
async fn secret_use_agent_id_must_match_filing_stamp() {
    let f = fixture().await;
    let before = f.rows();
    assert!(f
        .call(
            &filer("aft", Some("another-agent")),
            "consent.request",
            body("approval")
        )
        .await
        .is_err());
    assert_eq!(f.rows(), before);
}

#[tokio::test]
async fn secret_use_null_agent_binding_is_exact() {
    for (agent, statement_agent, accepted) in [
        (None, Value::Null, true),
        (None, json!("agent-A"), false),
        (Some("agent-A"), Value::Null, false),
    ] {
        let f = fixture().await;
        let mut request = body("approval");
        let mut statement = decoded(&request["secret_use"]["items"][0]["statement_b64url"]);
        statement["agent_id"] = statement_agent;
        request["secret_use"]["items"][0]["statement_b64url"] = encoded(&statement);
        assert_eq!(
            f.call(&filer("cerebellum", agent), "consent.request", request)
                .await
                .is_ok(),
            accepted
        );
    }
}

#[tokio::test]
async fn secret_use_device_fingerprint_must_match_answering_route() {
    let f = fixture().await;
    let id = f
        .request(&filer("aft", Some("agent-A")), body("approval"))
        .await;
    let before = f.rows();
    assert!(f
        .call(
            &phone("22222222222222222222222222222222"),
            "consent.answer",
            answer(&id, "approval")
        )
        .await
        .is_err());
    assert_eq!(f.rows(), before);
}

#[tokio::test]
async fn secret_use_statement_digest_must_match() {
    let f = fixture().await;
    let id = f
        .request(&filer("aft", Some("agent-A")), body("approval"))
        .await;
    let mut params = answer(&id, "approval");
    let mut envelope = decoded(&params["envelope_b64url"]);
    envelope["items"][0]["statement_sha256"] = json!("0".repeat(64));
    params["envelope_b64url"] = encoded(&envelope);
    assert!(f
        .call(&phone(DEVICE), "consent.answer", params)
        .await
        .is_err());
    assert_eq!(f.stored_record(&id).state, cingulate_store::State::Pending);
}

#[tokio::test]
async fn secret_use_duration_must_be_offered_by_every_allowed_item() {
    let f = fixture().await;
    let mut request = body("batch-omitted");
    let mut statement = decoded(&request["secret_use"]["items"][1]["statement_b64url"]);
    statement["durations"] = json!(["permanent"]);
    request["secret_use"]["items"][1]["statement_b64url"] = encoded(&statement);
    let id = f
        .request(&filer("aft", Some("agent-A")), request.clone())
        .await;
    let mut params = answer(&id, "batch-omitted");
    let mut envelope = decoded(&params["envelope_b64url"]);
    envelope["items"].as_array_mut().unwrap().push(json!({
        "request_id":statement["request_id"],
        "statement_sha256":format!("{:x}", Sha256::digest(URL_SAFE_NO_PAD.decode(request["secret_use"]["items"][1]["statement_b64url"].as_str().unwrap()).unwrap()))
    }));
    params["envelope_b64url"] = encoded(&envelope);
    assert!(f
        .call(&phone(DEVICE), "consent.answer", params)
        .await
        .is_err());
    assert_eq!(f.stored_record(&id).state, cingulate_store::State::Pending);
}

#[tokio::test]
async fn secret_use_filing_is_closed_and_canonical() {
    let original = body("approval");
    let statement = decoded(&original["secret_use"]["items"][0]["statement_b64url"]);
    let mut bad = Vec::new();
    for (field, value) in [
        ("title", json!("filer text")),
        ("prompt", json!("text")),
        ("labels", json!([])),
        ("on_expiry", json!("allow")),
    ] {
        let mut request = original.clone();
        request[field] = value;
        bad.push(request);
    }
    for (field, value) in [
        ("extra", json!(true)),
        ("v", json!("ck-secret-use-statement/v2")),
        ("request_id", json!("01HF7YAT000000000000000002")),
        ("request_expires_at_ms", json!(9_007_199_254_740_992_u64)),
        ("request_expires_at_ms", json!(1700000600000.5)),
    ] {
        let mut s = statement.clone();
        s[field] = value;
        let mut request = original.clone();
        request["secret_use"]["items"][0]["statement_b64url"] = encoded(&s);
        bad.push(request);
    }
    for pointer in [
        "/agent_id",
        "/scope/epoch",
        "/secret/incarnation",
        "/use/location",
    ] {
        let mut s = statement.clone();
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        s.pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(key);
        let mut request = original.clone();
        request["secret_use"]["items"][0]["statement_b64url"] = encoded(&s);
        bad.push(request);
    }
    for encoded in [
        URL_SAFE_NO_PAD.encode(serde_json::to_string_pretty(&statement).unwrap()),
        format!(
            "{}=",
            original["secret_use"]["items"][0]["statement_b64url"]
                .as_str()
                .unwrap()
        ),
        URL_SAFE_NO_PAD.encode(serde_jcs::to_string(&statement).unwrap().replacen(
            "{",
            "{\"v\":\"duplicate\",",
            1,
        )),
    ] {
        let mut request = original.clone();
        request["secret_use"]["items"][0]["statement_b64url"] = json!(encoded);
        bad.push(request);
    }
    let mut extra = original.clone();
    extra["secret_use"]["extra"] = json!(true);
    bad.push(extra);
    for field in ["scope", "secret", "use"] {
        let mut s = statement.clone();
        s[field]["extra"] = json!(true);
        let mut request = original.clone();
        request["secret_use"]["items"][0]["statement_b64url"] = encoded(&s);
        bad.push(request);
    }
    let mut trailing = original.clone();
    let mut text = trailing["secret_use"]["items"][0]["statement_b64url"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(text.pop(), Some('0'));
    text.push('1');
    trailing["secret_use"]["items"][0]["statement_b64url"] = json!(text);
    bad.push(trailing);
    let mut extra = original.clone();
    extra["secret_use"]["items"][0]["extra"] = json!(true);
    bad.push(extra);
    for n in [0, 2, 17] {
        let mut request = original.clone();
        request["secret_use"]["items"] = json!(vec![original["secret_use"]["items"][0].clone(); n]);
        bad.push(request);
    }
    let mut lower = original.clone();
    lower["secret_use"]["items"][0]["request_id"] = json!("01hf7yat000000000000000001");
    bad.push(lower);
    for id in [
        "01hf7yat000000000000000001",
        "81HF7YAT000000000000000001",
        "01HF7YAT00000000000000000I",
        "short",
    ] {
        let mut request = original.clone();
        let mut s = statement.clone();
        s["request_id"] = json!(id);
        request["secret_use"]["items"][0]["request_id"] = json!(id);
        request["secret_use"]["items"][0]["statement_b64url"] = encoded(&s);
        bad.push(request);
    }
    for request in bad {
        let f = fixture().await;
        assert!(
            f.call(
                &filer("aft", Some("agent-A")),
                "consent.request",
                request.clone()
            )
            .await
            .is_err(),
            "{request}"
        );
        assert!(f.rows()[0].is_empty());
    }
}

#[tokio::test]
async fn secret_use_envelope_is_closed_canonical_and_allow_only() {
    let f = fixture().await;
    let id = f
        .request(&filer("aft", Some("agent-A")), body("approval"))
        .await;
    let original = answer(&id, "approval");
    let envelope = decoded(&original["envelope_b64url"]);
    let mut bad = Vec::new();
    for (field, value) in [
        ("extra", json!(true)),
        ("v", json!("ck-secret-use-envelope/v2")),
        ("decision", json!("delete")),
        ("issued_at_ms", json!(9_007_199_254_740_992_u64)),
        ("grant_expires_at_ms", json!(0.5)),
        ("items", json!([])),
        ("items", json!([envelope["items"][0], envelope["items"][0]])),
    ] {
        let mut e = envelope.clone();
        e[field] = value;
        bad.push(encoded(&e));
    }
    for field in envelope.as_object().unwrap().keys() {
        let mut e = envelope.clone();
        e.as_object_mut().unwrap().remove(field);
        bad.push(encoded(&e));
    }
    let mut e = envelope.clone();
    e["items"][0]["extra"] = json!(true);
    bad.push(encoded(&e));
    bad.push(json!(
        URL_SAFE_NO_PAD.encode(serde_json::to_string_pretty(&envelope).unwrap())
    ));
    bad.push(json!(format!(
        "{}=",
        original["envelope_b64url"].as_str().unwrap()
    )));
    bad.push(json!(URL_SAFE_NO_PAD.encode(
        serde_jcs::to_string(&envelope)
            .unwrap()
            .replacen("{", "{\"v\":\"duplicate\",", 1)
    )));
    bad.push(Value::Null);
    let mut text = original["envelope_b64url"].as_str().unwrap().to_owned();
    assert_eq!(text.pop(), Some('Q'));
    text.push('R');
    bad.push(json!(text));
    for encoded in bad {
        let mut params = original.clone();
        params["envelope_b64url"] = encoded;
        let before = f.rows();
        assert!(
            f.call(&phone(DEVICE), "consent.answer", params.clone())
                .await
                .is_err(),
            "{params}"
        );
        assert_eq!(f.rows(), before);
    }
    let mut params = original.clone();
    params.as_object_mut().unwrap().remove("envelope_b64url");
    assert!(f
        .call(&phone(DEVICE), "consent.answer", params)
        .await
        .is_err());
    let mut decline = original;
    decline["choiceId"] = json!("decline");
    assert!(f
        .call(&phone(DEVICE), "consent.answer", decline.clone())
        .await
        .is_err());
    decline.as_object_mut().unwrap().remove("envelope_b64url");
    assert_eq!(
        f.ok(&phone(DEVICE), "consent.answer", decline).await["ok"],
        true
    );
    let binding = f.stored_record(&id).secret_use_answer.unwrap();
    assert_eq!(binding.decision, "decline");
    assert_eq!(binding.duration, None);
    assert_eq!(binding.envelope_sha256, None);
    assert_eq!(binding.items[0].decision, "decline");
}

#[tokio::test]
async fn secret_use_filer_requires_sender_principal_and_scope() {
    let f = fixture().await;
    for sender in [
        support::stamp("reserved:aft"),
        support::stamp("reserved:cerebellum"),
        filer("plexus", Some("agent-A")),
        filer("prefrontal-core", Some("agent-A")),
        support::stamp("direct"),
        phone(DEVICE),
    ] {
        let before = f.rows();
        assert_eq!(
            f.call(&sender, "consent.request", body("approval"))
                .await
                .unwrap_err()["code"],
            "elicitation_requester_refused"
        );
        assert_eq!(f.rows(), before);
    }
    let id = f
        .request(&filer("cerebellum", Some("agent-A")), body("approval"))
        .await;
    assert_eq!(
        f.stored_record(&id).subject.agent_id.as_deref(),
        Some("agent-A")
    );
    assert_eq!(
        f.payloads(),
        [json!({"elicitationId":id,"kind":"elicitation","eventAtMs":NOW})]
    );
}

#[tokio::test]
async fn secret_use_batch_requires_one_member_and_agent() {
    for field in ["agent_id", "member_id"] {
        let f = fixture().await;
        let mut request = body("batch-omitted");
        let mut statement = decoded(&request["secret_use"]["items"][1]["statement_b64url"]);
        statement[field] = json!("different");
        request["secret_use"]["items"][1]["statement_b64url"] = encoded(&statement);
        assert!(f
            .call(&filer("aft", Some("agent-A")), "consent.request", request)
            .await
            .is_err());
    }
}

#[tokio::test]
async fn secret_use_expiry_is_earliest_and_late_answers_are_refused() {
    let f = fixture().await;
    let mut request = body("batch-omitted");
    let mut statement = decoded(&request["secret_use"]["items"][1]["statement_b64url"]);
    statement["request_expires_at_ms"] = json!(NOW + 1000);
    request["secret_use"]["items"][1]["statement_b64url"] = encoded(&statement);
    let id = f
        .request(&filer("aft", Some("agent-A")), request.clone())
        .await;
    assert_eq!(f.stored_record(&id).expires_at_ms, Some(NOW + 1000));
    assert_eq!(f.stored_record(&id).request.default, "decline");
    assert_eq!(f.stored_record(&id).request.on_expiry, "deny");
    f.clock.set(NOW + 1000);
    for choice in ["allow", "decline"] {
        let mut params = answer(&id, "approval");
        params["choiceId"] = json!(choice);
        if choice == "decline" {
            params.as_object_mut().unwrap().remove("envelope_b64url");
        }
        assert_eq!(
            f.call(&phone(DEVICE), "consent.answer", params)
                .await
                .unwrap_err()["code"],
            "elicitation_expired"
        );
    }
    assert_eq!(
        f.call(&filer("aft", Some("agent-A")), "consent.request", request)
            .await
            .unwrap_err()["code"],
        "elicitation_expired"
    );
}

#[tokio::test]
async fn secret_use_idempotency_does_not_refresh_or_reuse_request_ids() {
    let f = fixture().await;
    let sender = filer("aft", Some("agent-A"));
    let id = f.request(&sender, body("batch-omitted")).await;
    let before = f.rows();
    f.clock.set(NOW + 1);
    assert_eq!(f.request(&sender, body("batch-omitted")).await, id);
    assert_eq!(f.rows(), before);
    for request in [body("approval"), {
        let mut r = body("batch-omitted");
        r["secret_use"]["items"].as_array_mut().unwrap().reverse();
        r
    }] {
        assert_eq!(
            f.call(&sender, "consent.request", request)
                .await
                .unwrap_err()["code"],
            "elicitation_request_conflict"
        );
    }
    assert_eq!(
        f.call(
            &filer("cerebellum", Some("agent-A")),
            "consent.request",
            body("batch-omitted")
        )
        .await
        .unwrap_err()["code"],
        "elicitation_request_conflict"
    );
    f.ok(
        &phone(DEVICE),
        "consent.answer",
        answer(&id, "batch-omitted"),
    )
    .await;
    assert_eq!(f.request(&sender, body("batch-omitted")).await, id);
    let mut params = answer(&id, "batch-omitted");
    let mut envelope = decoded(&params["envelope_b64url"]);
    envelope["duration"] = json!("permanent");
    envelope["grant_expires_at_ms"] = json!(0);
    params["envelope_b64url"] = encoded(&envelope);
    assert_eq!(
        f.ok(&phone(DEVICE), "consent.answer", params).await["code"],
        "conflict"
    );
    assert_eq!(
        f.stored_record(&id)
            .secret_use_answer
            .unwrap()
            .envelope_sha256
            .as_deref(),
        vector("batch-omitted")["envelope_sha256"].as_str()
    );
    f.clock.set(NOW + cingulate_store::RETENTION_MS + 1);
    f.handler.timer_tick().await.unwrap();
    let mut request = body("approval");
    let mut statement = decoded(&request["secret_use"]["items"][0]["statement_b64url"]);
    statement["request_expires_at_ms"] = json!(NOW + cingulate_store::RETENTION_MS + 1000);
    request["secret_use"]["items"][0]["statement_b64url"] = encoded(&statement);
    assert_eq!(
        f.call(&sender, "consent.request", request)
            .await
            .unwrap_err()["code"],
        "elicitation_request_conflict"
    );
}

#[tokio::test]
async fn secret_use_phone_feature_is_required_for_reads_allow_and_decline() {
    let f = fixture().await;
    let id = f
        .request(&filer("aft", Some("agent-A")), body("approval"))
        .await;
    let visible = f
        .ok(
            &phone(DEVICE),
            "consent.list_pending",
            json!({"clientFeatures":["secret-use-approval"]}),
        )
        .await;
    assert_eq!(visible["records"].as_array().unwrap().len(), 1);
    for route in [
        phone(DEVICE),
        phone("bad"),
        {
            let mut p = phone(DEVICE);
            p.harness = format!("fed-computer:{DEVICE}");
            p
        },
        support::owner(),
    ] {
        for features in [json!([]), json!(["secret-use-approval"])] {
            if route.harness == format!("fed-phone:{DEVICE}")
                && features == json!(["secret-use-approval"])
            {
                continue;
            }
            let list = f
                .call(
                    &route,
                    "consent.list_pending",
                    json!({"clientFeatures":features,"if_revision":visible["revision"]}),
                )
                .await;
            assert!(list.is_err() || list.unwrap()["records"] == json!([]));
            let get = f
                .call(
                    &route,
                    "consent.get",
                    json!({"elicitationId":id,"clientFeatures":features}),
                )
                .await;
            assert!(get.is_err());
            for choice in ["allow", "decline"] {
                let mut params = answer(&id, "approval");
                params["clientFeatures"] = features.clone();
                params["choiceId"] = json!(choice);
                if choice == "decline" {
                    params.as_object_mut().unwrap().remove("envelope_b64url");
                }
                let before = f.rows();
                assert!(f.call(&route, "consent.answer", params).await.is_err());
                assert_eq!(f.rows(), before);
            }
        }
    }
}

#[tokio::test]
async fn secret_use_served_vectors_regenerate_byte_for_byte() {
    for (name, bytes) in generate().await {
        assert_eq!(fs::read(support::corpus(CORPUS).join(name)).unwrap(), bytes);
    }
}

#[tokio::test]
async fn secret_use_filer_vectors_regenerate_byte_for_byte() {
    // Every filer reply comes from the real handler; the files hold the exact
    // framed method, request params and reply envelope for decoder builders.
    let files = generate().await;
    for name in [
        "filer-request.json",
        "filer-request-repeat.json",
        "filer-await-pending.json",
        "filer-await-allowed.json",
        "filer-await-partial.json",
        "filer-await-declined.json",
        "filer-await-expired.json",
        "filer-refusals.json",
    ] {
        assert_eq!(
            fs::read(support::corpus(CORPUS).join(name)).unwrap(),
            files[name],
            "{name}"
        );
    }
}

#[tokio::test]
async fn secret_use_watch_hides_replay_live_and_pruned_removal_ids() {
    use watch_pending::{Watch, INSTANCE};
    let f = fixture().await;
    f.pin_store_instance(INSTANCE);
    let id = f
        .request(&filer("aft", Some("agent-A")), body("approval"))
        .await;
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
    let mut malformed = Watch::open(
        &f.handler,
        &phone("bad"),
        json!({"cursor":{"instance":INSTANCE,"version":0},"clientFeatures":["secret-use-approval"]}),
    );
    legacy.quiet().await;
    malformed.quiet().await;
    f.ok(&phone(DEVICE), "consent.answer", answer(&id, "approval"))
        .await;
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
    malformed.quiet().await;
    for watch in [visible, legacy, malformed] {
        watch.unsubscribe();
        watch.outcome().await;
    }
}

#[test]
fn secret_use_vector_checksums_cover_every_file() {
    let root = support::corpus(CORPUS);
    assert_eq!(
        format!(
            "{:x}",
            Sha256::digest(fs::read(root.join("secret-approval-v1.json")).unwrap())
        ),
        "efd4ba60641586e143bce81087e4f9f5aaf8a99ffede74190303da7d4495cc27"
    );
    let mut seen = BTreeSet::new();
    for line in fs::read_to_string(root.join("SHA256SUMS")).unwrap().lines() {
        let (hash, name) = line.split_once("  ").unwrap();
        assert!(seen.insert(name.to_owned()));
        assert_eq!(
            format!("{:x}", Sha256::digest(fs::read(root.join(name)).unwrap())),
            hash
        );
    }
    seen.insert("SHA256SUMS".into());
    assert_eq!(
        seen,
        fs::read_dir(root)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect()
    );
}
