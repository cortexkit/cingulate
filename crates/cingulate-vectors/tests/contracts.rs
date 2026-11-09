//! Checks the contract corpora `danger-option-group-v1` and `permissions-v1`.
//!
//! These tests check the vectors against the contract's rules: canonical bytes, digests, and the
//! cross-case invariants an implementation will be held to. The canonicalizer
//! here is written independently of the generator in `examples/`, which relies
//! on serde_json's sorted maps.

mod support;

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};

use serde_json::Value;
use sha2::{Digest, Sha256};
use support::corpus;

const DANGER: &str = "danger-option-group-v1";
const PERMISSIONS: &str = "permissions-v1";

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// RFC 8785 for JSON whose numbers are integers. Keys sort by UTF-16 code
/// units; strings escape only what JSON requires, with the short escapes.
fn canonical(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        Value::Number(number) => {
            assert!(
                number.is_i64() || number.is_u64(),
                "corpus numbers must be integers: {number}"
            );
            out.push_str(&number.to_string());
        }
        Value::String(text) => {
            out.push('"');
            for ch in text.chars() {
                match ch {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\u{8}' => out.push_str("\\b"),
                    '\u{c}' => out.push_str("\\f"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
                    ch => out.push(ch),
                }
            }
            out.push('"');
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                canonical(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
            out.push('{');
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                canonical(&Value::String(key.clone()), out);
                out.push(':');
                canonical(&map[key], out);
            }
            out.push('}');
        }
    }
}

fn jcs(value: &Value) -> String {
    let mut out = String::new();
    canonical(value, &mut out);
    out
}

/// Loads every case of a corpus after checking its triple: the `.jcs` is the
/// canonical form of the `.json`, and the `.sha256` is the digest of the `.jcs`.
fn cases(name: &str) -> BTreeMap<String, Value> {
    let dir = corpus(name);
    let mut stems = BTreeMap::<String, BTreeSet<String>>::new();
    for entry in fs::read_dir(&dir).unwrap() {
        let file = entry.unwrap().file_name().into_string().unwrap();
        if file == "README.md" || file == "SHA256SUMS" {
            continue;
        }
        let (stem, extension) = file.rsplit_once('.').unwrap();
        stems
            .entry(stem.to_owned())
            .or_default()
            .insert(extension.to_owned());
    }
    let mut cases = BTreeMap::new();
    for (stem, extensions) in stems {
        assert_eq!(
            extensions,
            BTreeSet::from(["jcs".into(), "json".into(), "sha256".into()]),
            "{name}/{stem} must be a complete triple"
        );
        let pretty = fs::read_to_string(dir.join(format!("{stem}.json"))).unwrap();
        assert!(pretty.ends_with("}\n") && !pretty.ends_with("\n\n"));
        let value: Value = serde_json::from_str(&pretty).unwrap();
        let jcs_bytes = fs::read(dir.join(format!("{stem}.jcs"))).unwrap();
        assert_eq!(
            String::from_utf8(jcs_bytes.clone()).unwrap(),
            jcs(&value),
            "{name}/{stem}.jcs"
        );
        let digest = fs::read_to_string(dir.join(format!("{stem}.sha256"))).unwrap();
        assert_eq!(digest, sha256(&jcs_bytes), "{name}/{stem}.sha256");
        assert_eq!(value["tag"], name, "{name}/{stem}");
        if name == PERMISSIONS && stem == "federated-precedence" {
            assert!(value["cases"].is_array(), "decision table cases");
        } else {
            assert!(
                value.get("reply").is_some() != value.get("error").is_some(),
                "{name}/{stem} has exactly one of reply and error"
            );
        }
        cases.insert(stem, value);
    }
    cases
}

fn options(record: &Value) -> &Vec<Value> {
    record["options"].as_array().unwrap()
}

fn is_danger(option: &Value) -> bool {
    option.get("group").and_then(Value::as_str) == Some("danger")
}

#[test]
fn canonicalizer_sorts_by_utf16_and_escapes_controls() {
    let value = serde_json::json!({"b": [1, -2], "a": "x\"\n\u{1}", "A": null});
    assert_eq!(jcs(&value), r#"{"A":null,"a":"x\"\n\u0001","b":[1,-2]}"#);
}

#[test]
fn danger_corpus_triples_are_canonical() {
    assert_eq!(cases(DANGER).len(), 10);
}

#[test]
fn permissions_corpus_triples_are_canonical() {
    assert_eq!(cases(PERMISSIONS).len(), 40);
}

#[test]
fn filed_danger_card_keeps_its_default_and_a_safe_grant() {
    let cases = cases(DANGER);
    let card = &cases["file-card"]["request"];
    let default = card["default"].as_str().unwrap();
    for option in options(card) {
        if let Some(group) = option.get("group") {
            assert_eq!(group, "danger", "the only defined group");
        }
        if option["id"] == default {
            assert_eq!(option["effect"], "decline");
            assert!(!is_danger(option), "the default is never a danger option");
        }
    }
    assert!(options(card)
        .iter()
        .any(|option| option["effect"] == "grant" && !is_danger(option)));
    assert!(options(card).iter().any(is_danger));
}

#[test]
fn undeclared_reads_strip_exactly_the_danger_options() {
    let cases = cases(DANGER);
    let filed = options(&cases["file-card"]["request"]).clone();
    let declared = serde_json::json!(["option-group"]);
    for (with, without, record) in [
        (
            "list-pending-declared",
            "list-pending-undeclared",
            "/reply/records/0",
        ),
        ("get-declared", "get-undeclared", "/reply"),
    ] {
        assert_eq!(cases[with]["request"]["clientFeatures"], declared);
        assert!(cases[without]["request"].get("clientFeatures").is_none());
        let full = cases[with].pointer(record).unwrap();
        let stripped = cases[without].pointer(record).unwrap();
        assert_eq!(options(full), &filed, "{with} carries every filed option");
        let expected: Vec<Value> = filed.iter().filter(|o| !is_danger(o)).cloned().collect();
        assert_eq!(options(stripped), &expected, "{without}");
        assert!(options(stripped).iter().any(|o| o["id"] == full["default"]));
        let mut rest = stripped.clone();
        rest["options"] = full["options"].clone();
        assert_eq!(&rest, full, "{without} differs from {with} only in options");
    }
}

#[test]
fn filing_refusals_are_minimal_edits_with_their_codes() {
    let cases = cases(DANGER);
    let filed = &cases["file-card"]["request"];
    for (name, code) in [
        ("refuse-danger-default", "elicitation_fail_closed_required"),
        ("refuse-only-danger-grants", "elicitation_invalid_request"),
        ("refuse-unknown-group", "elicitation_invalid_request"),
    ] {
        let case = &cases[name];
        assert_eq!(case["method"], "consent.request");
        assert_eq!(case["error"]["code"], code, "{name}");
        let mut body = case["request"].clone();
        body["options"] = filed["options"].clone();
        assert_eq!(&body, filed, "{name} edits only the options");
    }
    let default_case = &cases["refuse-danger-default"]["request"];
    let default = default_case["default"].as_str().unwrap();
    assert!(options(default_case)
        .iter()
        .any(|o| o["id"] == default && is_danger(o)));
    let only = &cases["refuse-only-danger-grants"]["request"];
    let grants: Vec<&Value> = options(only)
        .iter()
        .filter(|o| o["effect"] == "grant")
        .collect();
    assert!(!grants.is_empty() && grants.iter().all(|o| is_danger(o)));
    let unknown = &cases["refuse-unknown-group"]["request"];
    assert!(options(unknown)
        .iter()
        .any(|o| o.get("group").is_some_and(|g| g != "danger")));
}

#[test]
fn danger_answer_needs_the_declared_feature() {
    let cases = cases(DANGER);
    let filed = options(&cases["file-card"]["request"]).clone();
    let refused = &cases["answer-danger-undeclared"];
    let choice = refused["request"]["choiceId"].as_str().unwrap();
    assert!(filed.iter().any(|o| o["id"] == choice && is_danger(o)));
    assert!(refused["request"].get("clientFeatures").is_none());
    assert_eq!(
        refused["error"]["code"],
        "elicitation_option_group_required"
    );
    assert!(refused["error"]["message"]
        .as_str()
        .unwrap()
        .contains(choice));
    let answered = &cases["answer-danger-declared"];
    assert_eq!(answered["request"]["choiceId"], choice);
    assert_eq!(answered["reply"]["elicitation"]["answeredChoiceId"], choice);
    assert_eq!(options(&answered["reply"]["elicitation"]), &filed);
}

/// Undeclared surfaces see exactly the projection of a card filed without its
/// danger options, including every field outside the option list.
#[tokio::test]
async fn undeclared_replies_match_plain_card_projection_byte_for_byte() {
    let cases = cases(DANGER);
    let filed = cases["file-card"]["request"].clone();
    let mut stripped = filed;
    let kept: Vec<Value> = options(&stripped)
        .iter()
        .filter(|o| !is_danger(o))
        .cloned()
        .collect();
    stripped["options"] = Value::Array(kept);
    let (full, full_id) = danger_fixture(&cases).await;
    let plain = support::Fixture::open(None).await;
    plain.clock.set(1_000_000);
    let plain_id = plain
        .request(&danger_stamp(&cases["file-card"]), stripped)
        .await;
    for name in ["list-pending-undeclared", "get-undeclared"] {
        let full_reply = danger_call(&full, &full_id, &cases[name]).await.unwrap();
        let plain_reply = danger_call(&plain, &plain_id, &cases[name]).await.unwrap();
        let full_reply = danger_projection(full_reply, &full_id);
        let plain_reply = danger_projection(plain_reply, &plain_id);
        assert_eq!(jcs(&full_reply), jcs(&plain_reply), "{name}");
        assert_eq!(jcs(&full_reply), jcs(&cases[name]["reply"]), "{name}");
    }
}

fn danger_stamp(case: &Value) -> ck_cingulate::Stamp {
    let route = &case["route"];
    ck_cingulate::Stamp {
        principal: route["principal"].as_str().map(str::to_owned),
        carried_agent_id: route["carried_agent_id"]
            .as_str()
            .unwrap_or_default()
            .into(),
        operator_surface: route["operator_surface"].as_bool().unwrap_or(false),
        bind_session: "danger-contract".into(),
        ..Default::default()
    }
}

fn rename_danger_id(value: &mut Value, from: &str, to: &str) {
    match value {
        Value::String(text) if text == from => *text = to.into(),
        Value::Array(values) => values
            .iter_mut()
            .for_each(|v| rename_danger_id(v, from, to)),
        Value::Object(values) => values
            .values_mut()
            .for_each(|v| rename_danger_id(v, from, to)),
        _ => {}
    }
}

fn danger_projection(mut reply: Value, id: &str) -> Value {
    rename_danger_id(&mut reply, id, "el_1");
    if let Some(records) = reply.get("records") {
        assert!(records.is_array());
        // The danger corpus predates the revision field used for conditional
        // polling. Require it here, but exclude it from the older byte comparison;
        // test-vectors/consent-phone-v1 separately fixes its complete wire bytes.
        assert!(reply["revision"].as_str().is_some_and(|v| !v.is_empty()));
        reply.as_object_mut().unwrap().remove("revision");
    }
    reply
}

async fn danger_call(fixture: &support::Fixture, id: &str, case: &Value) -> Result<Value, Value> {
    let mut params = case["request"].clone();
    rename_danger_id(&mut params, "el_1", id);
    fixture
        .call(
            &danger_stamp(case),
            case["method"].as_str().unwrap(),
            params,
        )
        .await
}

fn assert_danger_case(name: &str, case: &Value, outcome: Result<Value, Value>, id: &str) {
    let mut actual = case.clone();
    match outcome {
        Ok(reply) => actual["reply"] = danger_projection(reply, id),
        Err(error) => actual["error"] = error,
    }
    assert_eq!(
        jcs(&actual),
        fs::read_to_string(corpus(DANGER).join(format!("{name}.jcs"))).unwrap(),
        "{name} handler bytes"
    );
}

async fn danger_fixture(cases: &BTreeMap<String, Value>) -> (support::Fixture, String) {
    let fixture = support::Fixture::open(None).await;
    fixture.clock.set(1_000_000);
    let filed = &cases["file-card"];
    let reply = danger_call(&fixture, "el_1", filed).await.unwrap();
    let id = reply["elicitation_id"].as_str().unwrap().to_owned();
    assert_danger_case("file-card", filed, Ok(reply), &id);
    assert_eq!(
        serde_json::to_value(fixture.stored_record(&id).request).unwrap()["options"],
        filed["request"]["options"]
    );
    (fixture, id)
}

#[tokio::test]
async fn danger_filing_refusals_match_handler_byte_for_byte() {
    let cases = cases(DANGER);
    let fixture = support::Fixture::open(None).await;
    fixture.clock.set(1_000_000);
    for name in [
        "refuse-danger-default",
        "refuse-only-danger-grants",
        "refuse-unknown-group",
    ] {
        let rows = fixture.rows();
        let notices = fixture.payloads();
        let outcome = danger_call(&fixture, "el_1", &cases[name]).await;
        assert_danger_case(name, &cases[name], outcome, "el_1");
        assert_eq!(fixture.rows(), rows, "{name} wrote records");
        assert_eq!(fixture.payloads(), notices, "{name} notified");
    }
}

#[tokio::test]
async fn danger_surface_reads_match_handler_byte_for_byte() {
    let cases = cases(DANGER);
    let (fixture, id) = danger_fixture(&cases).await;
    for name in [
        "get-declared",
        "get-undeclared",
        "list-pending-declared",
        "list-pending-undeclared",
    ] {
        let rows = fixture.rows();
        let outcome = danger_call(&fixture, &id, &cases[name]).await;
        assert_danger_case(name, &cases[name], outcome, &id);
        assert_eq!(fixture.rows(), rows, "{name} wrote records");
    }
}

#[tokio::test]
async fn danger_answers_match_handler_byte_for_byte() {
    let cases = cases(DANGER);
    let (fixture, id) = danger_fixture(&cases).await;
    fixture.clock.set(1_005_000);
    let rows = fixture.rows();
    let notices = fixture.payloads();
    let refused = &cases["answer-danger-undeclared"];
    assert_danger_case(
        "answer-danger-undeclared",
        refused,
        danger_call(&fixture, &id, refused).await,
        &id,
    );
    assert_eq!(fixture.rows(), rows);
    assert_eq!(fixture.payloads(), notices);
    let declared = &cases["answer-danger-declared"];
    assert_danger_case(
        "answer-danger-declared",
        declared,
        danger_call(&fixture, &id, declared).await,
        &id,
    );
    let rows = fixture.rows();
    let notices = fixture.payloads();
    assert_danger_case(
        "answer-danger-undeclared",
        refused,
        danger_call(&fixture, &id, refused).await,
        &id,
    );
    assert_eq!(fixture.rows(), rows, "settled refusal wrote records");
    assert_eq!(fixture.payloads(), notices, "settled refusal notified");
}

#[tokio::test]
async fn danger_feature_declarations_are_typed_and_poll_revisions_are_projection_specific() {
    use serde_json::json;

    let cases = cases(DANGER);
    let (fixture, id) = danger_fixture(&cases).await;
    let phone = support::operator();
    for features in [
        json!(null),
        json!("option-group"),
        json!(["option-group", 1]),
    ] {
        for method in ["consent.get", "consent.list_pending", "consent.answer"] {
            let rows = fixture.rows();
            let notices = fixture.payloads();
            let error = fixture
                .call(
                    &phone,
                    method,
                    json!({
                        "elicitationId": id, "choiceId": "app_30m", "clientFeatures": features
                    }),
                )
                .await
                .unwrap_err();
            assert_eq!(error["code"], "elicitation_invalid_request", "{method}");
            assert_eq!(fixture.rows(), rows);
            assert_eq!(fixture.payloads(), notices);
        }
    }
    let unknown = fixture
        .ok(
            &phone,
            "consent.get",
            json!({
                "elicitationId": id, "clientFeatures": ["future-feature"]
            }),
        )
        .await;
    assert_eq!(options(&unknown).len(), 3);
    let plain = fixture.ok(&phone, "consent.list_pending", json!({})).await;
    let declared = fixture.ok(&phone, "consent.list_pending", json!({
        "clientFeatures": ["future-feature", "option-group"], "if_revision": plain["revision"]
    })).await;
    assert_eq!(options(&declared["records"][0]).len(), 5);
    assert_ne!(plain["revision"], declared["revision"]);
    let unchanged = fixture
        .ok(
            &phone,
            "consent.list_pending",
            json!({
                "clientFeatures": ["option-group"], "if_revision": declared["revision"]
            }),
        )
        .await;
    assert_eq!(
        unchanged,
        json!({"unchanged": true, "revision": declared["revision"]})
    );
    let plain_again = fixture
        .ok(
            &phone,
            "consent.list_pending",
            json!({
                "if_revision": declared["revision"]
            }),
        )
        .await;
    assert_eq!(plain_again, plain);
}

#[tokio::test]
async fn danger_answer_refusal_precedes_expiry_and_console_checks_without_writes() {
    let cases = cases(DANGER);
    let fixture = support::Fixture::open(Some("local")).await;
    fixture.clock.set(1_000_000);
    let mut body = cases["file-card"]["request"].clone();
    body["at_console"] = serde_json::json!("local");
    let id = fixture
        .request(&danger_stamp(&cases["file-card"]), body)
        .await;
    fixture.clock.set(1_060_001);
    let rows = fixture.rows();
    let notices = fixture.payloads();
    let refused = &cases["answer-danger-undeclared"];
    assert_danger_case(
        "answer-danger-undeclared",
        refused,
        danger_call(&fixture, &id, refused).await,
        &id,
    );
    assert_eq!(fixture.rows(), rows);
    assert_eq!(fixture.payloads(), notices);
}

#[tokio::test]
async fn danger_answer_readbacks_strip_options_but_keep_the_settled_choice() {
    use serde_json::json;

    let cases = cases(DANGER);
    let (fixture, id) = danger_fixture(&cases).await;
    fixture.clock.set(1_005_000);
    danger_call(&fixture, &id, &cases["answer-danger-declared"])
        .await
        .unwrap();
    let phone = support::operator();
    let settled = fixture
        .ok(&phone, "consent.get", json!({"elicitationId": id}))
        .await;
    assert_eq!(options(&settled).len(), 3);
    assert_eq!(settled["answeredChoiceId"], "computer_30m");
    let conflict = fixture
        .ok(
            &phone,
            "consent.answer",
            json!({
                "elicitationId": id, "choiceId": "app_30m"
            }),
        )
        .await;
    assert_eq!(conflict["code"], "conflict");
    assert_eq!(jcs(&conflict["elicitation"]), jcs(&settled));

    let (fixture, id) = danger_fixture(&cases).await;
    let answer = json!({"elicitationId": id, "choiceId": "app_30m"});
    let first = fixture.ok(&phone, "consent.answer", answer.clone()).await;
    assert_eq!(first["ok"], true);
    assert_eq!(options(&first["elicitation"]).len(), 3);
    let again = fixture.ok(&phone, "consent.answer", answer).await;
    assert_eq!(again["alreadyAnswered"], true);
    assert_eq!(jcs(&first["elicitation"]), jcs(&again["elicitation"]));
}

#[tokio::test]
async fn danger_requester_reads_and_feeds_keep_all_filed_options() {
    use serde_json::json;

    let cases = cases(DANGER);
    let (fixture, id) = danger_fixture(&cases).await;
    let requester = danger_stamp(&cases["file-card"]);
    let expected = &cases["file-card"]["request"]["options"];
    for method in ["consent.get", "consent.await"] {
        let reply = fixture
            .ok(
                &requester,
                method,
                json!({
                    "elicitation_id": id, "timeout_ms": 0
                }),
            )
            .await;
        assert_eq!(&reply["options"], expected, "{method}");
    }
    fixture.clock.set(1_005_000);
    danger_call(&fixture, &id, &cases["answer-danger-declared"])
        .await
        .unwrap();
    for (stamp, method) in [
        (requester, "consent.answers"),
        (
            support::stamp("reserved:prefrontal-core"),
            "consent.outcomes",
        ),
    ] {
        let reply = fixture.ok(&stamp, method, json!({"since": 0})).await;
        assert_eq!(reply["records"].as_array().unwrap().len(), 1, "{method}");
        assert_eq!(&reply["records"][0]["options"], expected, "{method}");
    }
}

const CEREBELLUM_SCOPES: [&str; 4] = ["app", "whole_computer", "site", "all_sites"];

/// 2^53 - 1: the largest integer every JSON reader represents exactly.
const MAX_FLOW_VERSION: u64 = 9_007_199_254_740_991;

/// Holder kinds this contract defines. Anything else is a kind from a later
/// version of the contract, which a reader must keep rather than drop.
const KNOWN_HOLDER_KINDS: [&str; 4] = ["agent", "worker", "operator", "flow"];

/// `methods`, when a row carries it, limits the grant to uppercase HTTP method
/// names and nothing else.
fn check_methods(name: &str, grant: &Value) {
    let Some(methods) = grant.get("methods") else {
        return;
    };
    let methods = methods.as_array().expect("methods is a list");
    assert!(!methods.is_empty(), "{name}: methods is never empty");
    for method in methods {
        let method = method.as_str().expect("methods holds strings");
        assert!(!method.is_empty(), "{name}: empty method");
        assert!(
            method.chars().all(|ch| ch.is_ascii_uppercase()),
            "{name}: method {method} is not uppercase"
        );
    }
}

/// A flow grant lasts while its installed flow version exists, so it names no
/// agent and carries no expiry.
fn check_flow_holder(name: &str, grant: &Value, holder: &Value) {
    for path in ["agent_id", "expires_at_ms"] {
        assert!(grant.get(path).is_none(), "{name}: flow row carries {path}");
        assert!(
            holder.get(path).is_none(),
            "{name}: flow holder carries {path}"
        );
    }
    for field in ["flow_id", "flow_install_id"] {
        assert!(
            holder[field].as_str().is_some_and(|text| !text.is_empty()),
            "{name}: flow holder needs {field}"
        );
    }
    let version = holder["flow_version"]
        .as_u64()
        .expect("flow_version is a non-negative integer");
    assert!(
        (1..=MAX_FLOW_VERSION).contains(&version),
        "{name}: flow_version {version} out of range"
    );
}

fn grant_rows(cases: &BTreeMap<String, Value>) -> Vec<(String, i64, Vec<Value>)> {
    let mut rows = Vec::new();
    for (name, case) in cases {
        if case.get("reply").is_none() {
            continue;
        }
        let now = case["now_ms"].as_i64().unwrap();
        if let Some(grants) = case.pointer("/reply/grants") {
            rows.push((name.clone(), now, grants.as_array().unwrap().clone()));
        }
        if let Some(modules) = case.pointer("/reply/modules") {
            for module in modules.as_array().unwrap() {
                if let Some(grants) = module.get("grants") {
                    rows.push((name.clone(), now, grants.as_array().unwrap().clone()));
                }
            }
        }
    }
    rows
}

#[test]
fn grant_rows_follow_the_row_rules() {
    let cases = cases(PERMISSIONS);
    let rows = grant_rows(&cases);
    assert!(rows.len() >= 6);
    for (name, now, grants) in rows {
        let mut holder_subjects = BTreeSet::new();
        for grant in &grants {
            let scope = grant["scope"].as_str().unwrap();
            assert!(!scope.is_empty(), "{name}: scope token");
            check_methods(&name, grant);
            // Scope vocabularies and broad-grant group rules belong to each
            // provider; only Cerebellum declares these four scopes here.
            if grant["module"] == "cerebellum" {
                assert!(CEREBELLUM_SCOPES.contains(&scope), "{name}: scope {scope}");
                let danger = matches!(scope, "whole_computer" | "all_sites");
                assert_eq!(grant.get("group").is_some(), danger, "{name}: group");
            }
            if grant.get("group").is_some() {
                assert_eq!(grant["group"], "danger");
            }
            assert!(!grant["scope_label"].as_str().unwrap().is_empty());
            assert!(grant["granted_at_ms"].as_i64().unwrap() <= now);
            if let Some(expiry) = grant.get("expires_at_ms") {
                assert!(expiry.as_i64().unwrap() > now, "{name}: expired row listed");
            }
            let holder = &grant["holder"];
            let kind = holder["kind"].as_str().expect("holder kind is a string");
            if !KNOWN_HOLDER_KINDS.contains(&kind) {
                // An unknown holder kind is shown as held by something, but
                // never as operator-held, and never dropped from the list.
                assert!(
                    kind != "operator",
                    "{name}: operator is a known kind, not an unknown one"
                );
                continue;
            }
            match kind {
                "agent" => assert!(holder.get("scope_ref").is_none()),
                "worker" => {
                    assert!(holder["scope_ref"].is_string());
                    assert_eq!(holder["agent_id"], grant["agent_id"], "owning head");
                }
                "operator" => {}
                "flow" => check_flow_holder(&name, grant, holder),
                _ => unreachable!(),
            }
            assert!(
                holder_subjects.insert((
                    holder.to_string(),
                    grant["subject_label"].clone().to_string()
                )),
                "{name}: one row per subject per holder"
            );
        }
    }
    let base = cases["provider-list"]["reply"]["grants"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|g| g["module"] == "cerebellum");
    let scopes: BTreeSet<&str> = base.map(|g| g["scope"].as_str().unwrap()).collect();
    assert_eq!(scopes, CEREBELLUM_SCOPES.into_iter().collect());
}

#[test]
fn flow_and_unknown_holder_rows_keep_their_holder_and_stay_unfiltered() {
    let cases = cases(PERMISSIONS);
    let base = cases["provider-list"]["reply"]["grants"]
        .as_array()
        .unwrap();

    let flows: Vec<&Value> = base
        .iter()
        .filter(|g| g["holder"]["kind"] == "flow")
        .collect();
    assert!(flows.len() >= 2, "flow grants with and without methods");
    let with_methods: Vec<&Value> = flows
        .iter()
        .filter(|grant| grant.get("methods").is_some())
        .copied()
        .collect();
    assert!(!with_methods.is_empty(), "a flow grant limited by method");
    assert!(
        flows.iter().any(|g| g.get("methods").is_none()),
        "a flow grant with no method limit"
    );
    assert_eq!(with_methods[0]["methods"], serde_json::json!(["GET"]));
    for flow in &flows {
        check_flow_holder("provider-list", flow, &flow["holder"]);
    }
    // Two installed versions of one flow are two holders, so the same subject
    // may appear once under each.
    let flow_ids: BTreeSet<&str> = flows
        .iter()
        .map(|g| g["holder"]["flow_id"].as_str().unwrap())
        .collect();
    assert_eq!(flow_ids.len(), 1, "both flow grants name one invented flow");
    let versions: BTreeSet<u64> = flows
        .iter()
        .map(|g| g["holder"]["flow_version"].as_u64().unwrap())
        .collect();
    assert_eq!(versions.len(), flows.len(), "a version per holder");

    let unknown: Vec<&Value> = base
        .iter()
        .filter(|g| !KNOWN_HOLDER_KINDS.contains(&g["holder"]["kind"].as_str().unwrap()))
        .collect();
    assert_eq!(unknown.len(), 1, "one row with a holder kind from later");
    assert!(
        unknown[0].get("agent_id").is_none(),
        "an unknown holder is never shown as operator-held, which carries no agent"
    );
    let unknown_holder = &unknown[0]["holder"];
    assert!(
        unknown_holder
            .as_object()
            .unwrap()
            .iter()
            .any(|(field, _)| field != "kind"),
        "the unknown holder keeps the fields that name it"
    );

    // Flow rows are listed only when the read is not filtered by agent.
    assert!(cases["provider-list"]["request"].get("agent_id").is_none());
    for name in ["provider-list-after-upgrade", "provider-list-after-expiry"] {
        let case = &cases[name];
        let rows = case["reply"]["grants"].as_array().unwrap();
        if case["request"].get("agent_id").is_some() {
            assert!(
                rows.iter()
                    .all(|g| !matches!(g["holder"]["kind"].as_str(), Some("flow" | "schedule"))),
                "{name}: flow or unknown-kind row in an agent-filtered list"
            );
        } else {
            assert!(
                rows.iter().any(|g| g["holder"]["kind"] == "flow")
                    && rows.iter().any(|g| g["holder"]["kind"] == "schedule"),
                "{name}: an unfiltered read keeps the flow and unknown-kind rows"
            );
        }
    }
}

#[test]
fn upgrade_keeps_the_id_and_expiry_drops_rows_at_the_deadline() {
    let cases = cases(PERMISSIONS);
    let base = cases["provider-list"]["reply"]["grants"]
        .as_array()
        .unwrap();
    let upgraded = cases["provider-list-after-upgrade"]["reply"]["grants"]
        .as_array()
        .unwrap();
    // The upgrade read is filtered to one agent, so it leaves out the rows whose
    // holders name no agent (flow grants and the unknown kind).
    let base_by_id = |id: &Value| base.iter().find(|g| g["id"] == *id).cloned();
    assert_eq!(base.len(), 8);
    assert_eq!(upgraded.len(), 5);
    assert_eq!(upgraded[0]["id"], base[0]["id"]);
    assert!(base[0].get("expires_at_ms").is_some());
    assert!(upgraded[0].get("expires_at_ms").is_none());
    assert_ne!(upgraded[0]["card_ref"], base[0]["card_ref"]);
    for row in &upgraded[1..] {
        assert_eq!(
            Some(row),
            base_by_id(&row["id"]).as_ref(),
            "row {}",
            row["id"]
        );
    }

    let after = &cases["provider-list-after-expiry"];
    let deadline = after["now_ms"].as_i64().unwrap();
    let expected: Vec<&Value> = base
        .iter()
        .filter(|g| {
            g.get("expires_at_ms")
                .is_none_or(|e| e.as_i64().unwrap() > deadline)
        })
        .collect();
    assert!(expected.len() < base.len());
    assert!(base
        .iter()
        .any(|g| g.get("expires_at_ms").and_then(Value::as_i64) == Some(deadline)));
    let listed: Vec<&Value> = after["reply"]["grants"]
        .as_array()
        .unwrap()
        .iter()
        .collect();
    assert_eq!(listed, expected);
    let expired = &cases["provider-revoke-expired"];
    assert_eq!(expired["now_ms"].as_i64().unwrap(), deadline);
    assert!(listed.iter().all(|g| g["id"] != expired["request"]["id"]));
    assert_eq!(
        expired["reply"],
        serde_json::json!({"outcome": "not_found"})
    );
}

#[test]
fn list_all_versions_hash_the_modules_and_unread_modules_carry_no_rows() {
    let cases = cases(PERMISSIONS);
    for name in [
        "list-all-holder-labels",
        "list-all-ok",
        "list-all-unreachable",
        "list-all-unsupported",
    ] {
        let reply = &cases[name]["reply"];
        let modules = &reply["modules"];
        assert_eq!(reply["version"], sha256(jcs(modules).as_bytes()), "{name}");
        let ids: Vec<&str> = modules
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["module"].as_str().unwrap())
            .collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted, "{name}: modules sorted by id");
        for module in modules.as_array().unwrap() {
            let status = module["status"].as_str().unwrap();
            assert!(matches!(status, "ok" | "unreachable" | "unsupported"));
            assert_eq!(module.get("grants").is_some(), status == "ok", "{name}");
        }
    }
    let unchanged = &cases["list-all-unchanged"];
    let version = &cases["list-all-ok"]["reply"]["version"];
    assert_eq!(&unchanged["request"]["since_version"], version);
    assert_eq!(
        unchanged["reply"],
        serde_json::json!({"version": version, "unchanged": true})
    );
}

#[test]
fn revoke_replies_cover_every_outcome_and_admission() {
    let cases = cases(PERMISSIONS);
    let mut aggregator = BTreeSet::new();
    let mut provider = BTreeSet::new();
    for case in cases.values() {
        if case["method"] != "permissions.revoke" {
            continue;
        }
        let principal = case["route"]["principal"].as_str().unwrap();
        let surface = case["route"]["operator_surface"].as_bool().unwrap_or(false);
        let is_aggregator = case["request"].get("module").is_some();
        assert!(case["request"].get("authority").is_none());
        let admitted = if is_aggregator {
            principal == "direct" || (principal == "reserved:callosum" && surface)
        } else {
            principal == "reserved:cingulate" || principal == "direct"
        };
        if let Some(error) = case.get("error") {
            if error["code"] == "permissions_unavailable" {
                assert!(is_aggregator && admitted);
            } else {
                assert!(!admitted);
                assert_eq!(error["code"], "permissions_operator_required");
            }
            continue;
        }
        assert!(admitted);
        let outcome = case["reply"]["outcome"].as_str().unwrap().to_owned();
        if is_aggregator {
            if outcome == "refused" {
                assert!(case["reply"]["code"].is_string());
            }
            aggregator.insert(outcome);
        } else {
            provider.insert(outcome);
        }
    }
    assert_eq!(
        aggregator,
        BTreeSet::from(
            [
                "revoked",
                "already_gone",
                "module_unreachable",
                "refused",
                "stopping"
            ]
            .map(String::from)
        )
    );
    assert_eq!(
        provider,
        BTreeSet::from(["revoked", "not_found"].map(String::from))
    );
}
