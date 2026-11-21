#[path = "support/broker_action.rs"]
mod broker_action;
#[path = "support/phone.rs"]
#[allow(dead_code)]
mod phone;
mod support;
#[path = "support/watch_pending.rs"]
#[allow(dead_code)]
mod watch_pending;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use broker_action::*;
use cingulate_decode::ConsentRequest;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

fn decode(statement: &Value) -> ConsentRequest {
    ConsentRequest::decode(filing(statement)).unwrap()
}

fn refuses(statement: &Value, field: &str) {
    let error = ConsentRequest::decode(filing(statement)).unwrap_err();
    assert_eq!(error.code, "elicitation_invalid_request");
    assert_eq!(error.message, format!("invalid broker_action.{field}"));
}

#[test]
fn broker_action_has_closed_typed_statement_and_filing_keys() {
    let single = statement(false);
    assert_eq!(decode(&single).default, "decline");
    for field in single
        .as_object()
        .unwrap()
        .keys()
        .filter(|key| *key != "context")
    {
        let mut missing = single.clone();
        missing.as_object_mut().unwrap().remove(field);
        refuses(&missing, field);
    }
    for (field, invalid) in [
        ("v", json!("v2")),
        ("request_id", json!("not-a-ulid")),
        ("agent_id", json!("agent/a")),
        ("service", json!("other")),
        ("identity", json!("github_app:Bad Slug")),
        ("host", json!("https://api.github.example")),
        ("resource", json!("owner/repo/path")),
        ("policy_version", json!("7")),
        ("policy_sha256", json!("AA".repeat(32))),
        ("method", json!("post")),
        ("kind", json!("not-the-action")),
        ("request_sha256", json!("bad")),
        ("context", Value::Null),
        ("expires_at_ms", json!(-1)),
    ] {
        let mut bad = single.clone();
        bad[field] = invalid;
        refuses(&bad, field);
    }
    for (container, expected) in [
        ("scope", "scope.extra"),
        ("fields", "fields.extra"),
        ("legs", "legs.extra"),
    ] {
        let mut bad = statement(container == "legs");
        if container == "scope" {
            bad[container]["extra"] = json!(false);
        } else {
            bad[container][0]["extra"] = json!(false);
        }
        refuses(&bad, expected);
    }
    for key in [
        "inputs",
        "title",
        "prompt",
        "options",
        "default",
        "expires_at_ms",
        "expires_in_ms",
    ] {
        let mut bad = filing(&single);
        bad[key] = json!(false);
        assert_eq!(
            ConsentRequest::decode(bad).unwrap_err().message,
            format!("invalid broker_action.{key}")
        );
    }
}

#[test]
fn broker_action_checks_limits_and_preserves_exact_bytes() {
    let mut single = statement(false);
    single["fields"] = json!(vec![
        json!({"name":"Sample", "value":[-9007199254740991_i64, 9007199254740991_i64, "verbatim\ntext"]});
        16
    ]);
    single["context"] = json!("é".repeat(200));
    assert_eq!(
        decode(&single).broker_action.unwrap().statement_b64url,
        encode(&single)
    );
    for value in [
        json!(-9007199254740992_i64),
        json!(9007199254740992_i64),
        json!({"nest":[-1.25]}),
    ] {
        let mut bad = single.clone();
        bad["fields"][0]["value"] = value;
        refuses(&bad, "fields.value");
    }
    for context in [
        "é".repeat(201),
        "one\nline".into(),
        "bidi\u{2069}mark".into(),
        "line\u{2028}break".into(),
        "para\u{2029}break".into(),
    ] {
        single["context"] = json!(context);
        refuses(&single, "context");
    }
    let mut single = statement(false);
    single["fields"][0]["value"] = json!("");
    let to_limit = 512 * 1024 - serde_jcs::to_vec(&single).unwrap().len();
    single["fields"][0]["value"] = json!("x".repeat(to_limit));
    decode(&single);
    single["fields"][0]["value"] = json!("x".repeat(to_limit + 1));
    refuses(&single, "statement_b64url");
}

#[test]
fn broker_action_legs_are_one_to_sixteen_and_exclude_top_level_binding() {
    let mut multiple = statement(true);
    let leg = multiple["legs"][0].clone();
    for count in [1, 16] {
        multiple["legs"] = json!(vec![leg.clone(); count]);
        decode(&multiple);
    }
    for count in [0, 17] {
        multiple["legs"] = json!(vec![leg.clone(); count]);
        refuses(&multiple, "legs");
    }
    for field in ["kind", "method", "request_sha256"] {
        let mut bad = statement(true);
        bad[field] = Value::Null;
        refuses(&bad, field);
    }
}

#[tokio::test]
async fn broker_action_handler_filings_answers_and_missed_cards() {
    let all = cases().await;
    let cases = &all["cases"];
    let result = |key: &str| &cases[key]["reply"]["result"];
    let code = |key: &str| &cases[key]["reply"]["error"]["code"];
    let first = &result("file-one-leg")["elicitation_id"];
    assert_eq!(result("dedup")["elicitation_id"], *first);
    assert_eq!(result("dedup-after-answer")["elicitation_id"], *first);
    assert_eq!(result("file-legs")["elicitation_id"], "el_2");
    assert_eq!(code("different-owner"), "elicitation_request_conflict");
    for name in ["unauthorized-filer", "missing-scope", "mismatched-agent"] {
        assert_eq!(code(name), "elicitation_requester_refused", "{name}");
    }
    for (name, field) in [
        ("unknown-statement-key", "unexpected"),
        ("invalid-encoding", "statement_b64url"),
        ("noncanonical", "statement_b64url"),
        ("oversize", "statement_b64url"),
        ("too-many-fields", "fields"),
        ("float", "fields.value"),
        ("context-character", "context"),
        ("expiry-override-refused", "expires_in_ms"),
    ] {
        assert_eq!(code(name), "elicitation_invalid_request", "{name}");
        assert_eq!(
            cases[name]["reply"]["error"]["message"],
            format!("invalid broker_action.{field}")
        );
    }
    for key in ["v", "request_id", "statement_sha256", "device_fingerprint"] {
        let name = format!("binding-{key}");
        assert_eq!(code(&name), "elicitation_invalid_request");
        assert_eq!(
            cases[&name]["reply"]["error"]["message"],
            format!("elicitation_invalid_request: invalid broker_action.{key}")
        );
    }
    let card = result("phone-get");
    assert_eq!(card["kind"], "broker_action");
    assert_eq!(card["default"], "decline");
    assert_eq!(card["onExpiry"], "deny");
    assert_eq!(card["expiresAtMs"], EXPIRY);
    assert_eq!(card["subject"]["agentId"], AGENT);
    assert_eq!(
        card["brokerAction"]["statement_b64url"],
        cases["file-one-leg"]["params"]["broker_action"]["statement_b64url"]
    );
    assert_eq!(code("desktop-get-refused"), "elicitation_not_found");
    assert_eq!(code("unsupported-get"), "elicitation_not_found");
    assert_eq!(
        code("unsupported-answer"),
        "elicitation_broker_action_approval_required"
    );
    assert_eq!(result("allow")["ok"], true);
    assert_eq!(result("decline")["ok"], true);
    let envelope = URL_SAFE_NO_PAD
        .decode(
            cases["allow"]["params"]["envelope_b64url"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
    let digest = &card["brokerAction"]["statement_sha256"];
    assert_eq!(
        result("requester-allow")["broker_action"],
        card["brokerAction"]
    );
    assert_eq!(result("requester-allow")["answered_device"], DEVICE);
    assert_eq!(
        result("requester-allow")["broker_action_answer"],
        json!({"decision":"allow","statement_sha256":digest,"envelope_sha256":format!("{:x}",Sha256::digest(envelope))})
    );
    assert_eq!(result("requester-decline")["state"], "answered");
    assert_eq!(
        result("requester-decline")["broker_action_answer"],
        json!({"decision":"decline","statement_sha256":digest})
    );
    for name in [
        "filing-at-expiry",
        "answer-at-expiry",
        "answer-after-expiry",
    ] {
        assert_eq!(code(name), "elicitation_expired", "{name}");
    }
    assert_eq!(result("requester-expiry")["state"], "expired");
    assert!(result("requester-expiry")
        .get("broker_action_answer")
        .is_none());
    assert_eq!(result("phone-get-missed")["state"], "expired");
    assert_eq!(
        result("phone-get-missed")["brokerAction"],
        card["brokerAction"]
    );
    assert!(result("phone-get-missed")
        .get("brokerActionAnswer")
        .is_none());
    assert_eq!(code("phone-get-after-retention"), "elicitation_not_found");
    assert_eq!(result("withdraw")["state"], "withdrawn");
}

#[tokio::test]
async fn broker_action_refuses_unbound_envelopes_and_conflicting_retries() {
    let f = fixture().await;
    let statement = statement(false);
    let id = f
        .request(&filer("aft", Some(AGENT), 7), filing(&statement))
        .await;
    let approval = envelope(&statement, DEVICE);
    let allow = answer(&id, Some(&approval));
    let mut missing = answer(&id, None);
    missing["choiceId"] = json!("allow");
    let mut decline_with_envelope = allow.clone();
    decline_with_envelope["choiceId"] = json!("decline");
    let mut decision = approval.clone();
    decision["decision"] = json!("decline");
    let mut extra = approval.clone();
    extra["extra"] = json!(false);
    let mut signed = allow.clone();
    signed["signature"] = json!("AQIDBA");
    for params in [
        missing,
        decline_with_envelope,
        answer(&id, Some(&decision)),
        answer(&id, Some(&extra)),
        signed,
    ] {
        assert_eq!(
            f.call(&phone(), "consent.answer", params)
                .await
                .unwrap_err()["code"],
            "elicitation_invalid_request"
        );
        assert_eq!(f.stored_record(&id).state, cingulate_store::State::Pending);
    }
    let ordinary_agent = filer("broca", Some(AGENT), 7);
    let computer = watch_pending::mac_stamp();
    for stamp in [ordinary_agent, computer, desktop()] {
        assert!(f.call(&stamp, "consent.get", get(&id)).await.is_err());
        assert!(f
            .call(&stamp, "consent.answer", answer(&id, None))
            .await
            .is_err());
    }
    let first = f.ok(&phone(), "consent.answer", allow.clone()).await;
    assert_eq!(first["alreadyAnswered"], false);
    let retry = f.ok(&phone(), "consent.answer", allow).await;
    assert_eq!(retry["alreadyAnswered"], true);
    let mut changed = approval;
    changed["issued_at_ms"] = json!(NOW + 1);
    let different = f
        .ok(&phone(), "consent.answer", answer(&id, Some(&changed)))
        .await;
    assert_eq!(different["code"], "conflict");
    assert_eq!(different["elicitation"], first["elicitation"]);
    let mut next = statement;
    next["request_id"] = json!(SECOND_ID);
    let next_id = f
        .request(&filer("aft", Some(AGENT), 7), filing(&next))
        .await;
    assert_ne!(next_id, id);
    f.clock.set(EXPIRY);
    assert_eq!(
        f.call(&phone(), "consent.answer", answer(&id, Some(&changed)))
            .await
            .unwrap_err()["code"],
        "elicitation_expired"
    );
}

#[tokio::test]
async fn broker_action_list_and_watch_never_show_hidden_cards_or_removals() {
    use watch_pending::{Watch, INSTANCE};
    let f = fixture().await;
    f.pin_store_instance(INSTANCE);
    let id = f
        .request(&filer("aft", Some(AGENT), 7), filing(&statement(false)))
        .await;
    let supported = f
        .ok(
            &phone(),
            "consent.list_pending",
            json!({"clientFeatures":[FEATURE]}),
        )
        .await;
    assert_eq!(supported["records"][0]["elicitationId"], id);
    assert_eq!(
        f.ok(
            &phone(),
            "consent.list_pending",
            json!({"if_revision":supported["revision"]})
        )
        .await["records"],
        json!([])
    );
    let mut visible = Watch::open(
        &f.handler,
        &phone(),
        json!({"cursor":{"instance":INSTANCE,"version":0},"clientFeatures":[FEATURE]}),
    );
    let mut hidden = Watch::open(
        &f.handler,
        &phone(),
        json!({"cursor":{"instance":INSTANCE,"version":0},"clientFeatures":["secret-use-approval"]}),
    );
    assert_eq!(visible.next_value().await["card"]["elicitationId"], id);
    hidden.quiet().await;
    f.ok(&phone(), "consent.answer", answer(&id, None)).await;
    assert_eq!(visible.next_value().await["card"]["state"], "answered");
    f.clock.set(NOW + cingulate_store::RETENTION_MS);
    f.ok(
        &phone(),
        "consent.list_pending",
        json!({"clientFeatures":[FEATURE]}),
    )
    .await;
    assert_eq!(visible.next_value().await["type"], "removed");
    hidden.quiet().await;
    for watch in [visible, hidden] {
        watch.unsubscribe();
        watch.outcome().await;
    }
}

#[tokio::test]
async fn broker_action_notices_contain_only_cingulate_metadata() {
    let f = fixture().await;
    let sender = filer("aft", Some(AGENT), 7);
    let single = statement(false);
    let answered = f.request(&sender, filing(&single)).await;
    f.ok(&phone(), "consent.answer", answer(&answered, None))
        .await;
    let withdrawn = f.request(&sender, filing(&statement(true))).await;
    f.ok(
        &sender,
        "consent.withdraw",
        json!({"elicitation_id":withdrawn}),
    )
    .await;
    let db = f.sql();
    let notices: Vec<Value> = db
        .prepare("SELECT body_json FROM access_notices ORDER BY rowid")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .map(|row| serde_json::from_str(&row.unwrap()).unwrap())
        .collect();
    assert_eq!(notices.len(), 4);
    for (notice, (id, purpose, fields)) in notices.iter().zip([
        (&answered, "escalated", Some(json!({"card_kind":"broker_action","subject":"ALF","expires_at_ms":EXPIRY,"phone_only":!ck_cingulate::DEVELOPMENT_LOCAL_OPERATOR}))),
        (&answered, "settled", Some(json!({"outcome":"answered"}))),
        (&withdrawn, "escalated", Some(json!({"card_kind":"broker_action","subject":"ALF","expires_at_ms":EXPIRY,"phone_only":!ck_cingulate::DEVELOPMENT_LOCAL_OPERATOR}))),
        (&withdrawn, "withdrawn", None),
    ]) {
        assert_eq!(notice["elicitation_id"], *id);
        assert_eq!(notice["purpose"], purpose);
        assert_eq!(notice.get("fields"), fields.as_ref());
        assert!(notice.as_object().unwrap().keys().all(|key| ["elicitation_id", "purpose", "fields", "asker"].contains(&key.as_str())));
    }
    assert_eq!(f.payloads().len(), 4);
    for pointer in f.payloads() {
        assert!(pointer.as_object().unwrap().keys().all(|key| [
            "elicitationId",
            "kind",
            "eventAtMs"
        ]
        .contains(&key.as_str())));
    }
    let outcomes = f
        .ok(
            &support::stamp("reserved:prefrontal"),
            "consent.outcomes",
            json!({"since":0}),
        )
        .await;
    assert_eq!(outcomes["records"], json!([]));
}

#[tokio::test]
async fn broker_action_advertises_the_capability_and_pins_real_handler_vectors() {
    let manifest = serde_json::to_value(ck_cingulate::manifest()).unwrap();
    assert!(manifest["capabilities"]["provides"]
        .as_array()
        .unwrap()
        .contains(&json!("broker-action-approval/v1")));
    let dev = development_cases().await;
    if ck_cingulate::DEVELOPMENT_LOCAL_OPERATOR {
        assert_eq!(
            dev["desktop-get"]["reply"]["result"]["kind"],
            "broker_action"
        );
        assert_eq!(
            dev["desktop-signed-refused"]["reply"]["error"]["code"],
            "elicitation_invalid_request"
        );
        assert_eq!(
            dev["desktop-allow"]["reply"]["result"]["elicitation"]["answeredDevice"],
            "0".repeat(32)
        );
    } else {
        for key in ["desktop-get", "desktop-signed-refused", "desktop-allow"] {
            assert!(dev[key]["reply"]["error"].is_object());
        }
    }
    let root = support::corpus(CORPUS);
    assert_eq!(
        format!(
            "{:x}",
            Sha256::digest(std::fs::read(root.join("SHA256SUMS")).unwrap())
        ),
        "17fdd86d6b29fe13995adcb360ae11f1289e34dfa97b9b88f1b2902f1440818c"
    );
    for (name, contents) in generate().await {
        assert_eq!(std::fs::read(root.join(name)).unwrap(), contents, "{name}");
    }
}
