use cingulate_decode::ConsentRequest;
use serde_json::{json, Value};

const INVALID: &str = "elicitation_invalid_request";
const FLOW_OPTIONS: &str =
    "flow_decision needs 2 to 4 options: exactly one decline and otherwise choose";
const NONEMPTY: &str = "title, prompt and nonempty options are required";
const OPTION_IDS: &str = "option ids must be unique and nonempty; labels must be nonempty";
const SCHEMA: &str = "requestedSchema decision enum must equal option ids";
const FAIL_CLOSED: &str = "consent requires a decline default and on_expiry deny";

fn consent() -> Value {
    serde_json::from_str(include_str!(
        "../../../test-vectors/elicitation-request-v1/valid/app-grant-once.json"
    ))
    .unwrap()
}

fn flow_install() -> Value {
    let mut body = consent();
    body["kind"] = json!("flow_install");
    body["flow_install"] = json!({"not_a_card": true});
    body
}

fn flow_decision() -> Value {
    let mut body = consent();
    body["kind"] = json!("flow_decision");
    body["options"][0]["effect"] = json!("choose");
    body["flow_decision"] = json!({"extra": {"key": true}});
    body
}

fn refusal(body: Value, code: &str, message: &str) {
    let error = ConsentRequest::decode(body).unwrap_err();
    assert_eq!(error.code, code);
    assert_eq!(error.message, message);
}

fn invalid(body: Value, message: &str) {
    refusal(body, INVALID, message);
}

fn serde_refusal(body: Value, message_contains: &str) {
    let error = ConsentRequest::decode(body.clone()).unwrap_err();
    assert_eq!(error.code, INVALID);
    assert!(
        error.message.contains(message_contains),
        "expected {message_contains:?}, got {:?}",
        error.message
    );
    let serde_error = serde_json::from_value::<ConsentRequest>(body).unwrap_err();
    assert_eq!(error.message, serde_error.to_string());
}

#[test]
fn serde_errors_precede_flow_rules_and_are_verbatim() {
    let mut body = flow_decision();
    body["session_ref"] = json!("session");
    body["unknown"] = json!(true);
    serde_refusal(body, "unknown field `unknown`");

    let mut body = flow_decision();
    body["session_ref"] = json!("session");
    body.as_object_mut().unwrap().remove("urgency");
    serde_refusal(body, "missing field `urgency`");

    let mut body = flow_decision();
    body["session_ref"] = json!(7);
    serde_refusal(body, "invalid type: integer `7`, expected a string");

    let mut body = flow_install();
    body["flow_install"] = json!([1, 2]);
    body["options"] = json!([]);
    serde_refusal(body, "invalid type: sequence, expected a map");
}

#[test]
fn every_required_field_is_required_and_typed() {
    for key in [
        "kind",
        "title",
        "prompt",
        "options",
        "default",
        "urgency",
        "on_expiry",
        "material_damage",
        "late_execution",
        "target",
        "expires_in_ms",
    ] {
        let mut missing = consent();
        missing.as_object_mut().unwrap().remove(key);
        serde_refusal(missing, &format!("missing field `{key}`"));

        let mut mistyped = consent();
        mistyped[key] = Value::Null;
        serde_refusal(mistyped, "invalid type: null");
    }
    for pointer in [
        "/options/0/id",
        "/options/0/label",
        "/options/0/effect",
        "/target/kind",
        "/target/label",
        "/facts/0/label",
        "/facts/0/value",
        "/preview/label",
        "/preview/text",
    ] {
        let mut body = consent();
        body["facts"] = json!([{"label": "Source", "value": "local"}]);
        body["preview"] = json!({"label": "Change", "text": "diff"});
        *body.pointer_mut(pointer).unwrap() = json!(true);
        serde_refusal(body, "invalid type: boolean `true`, expected a string");
    }
}

#[test]
fn closed_objects_refuse_unknown_keys_and_body_identity() {
    for key in [
        "unknown",
        "agent_id",
        "subject",
        "requester",
        "requested_schema",
    ] {
        let mut body = consent();
        body[key] = json!("forged");
        serde_refusal(body, &format!("unknown field `{key}`"));
    }
    for pointer in ["/options/0", "/target", "/facts/0", "/preview"] {
        let mut body = consent();
        body["facts"] = json!([{"label": "Source", "value": "local"}]);
        body["preview"] = json!({"label": "Change", "text": "diff"});
        body.pointer_mut(pointer).unwrap()["unknown"] = json!(true);
        serde_refusal(body, "unknown field `unknown`");
    }
}

#[test]
fn opaque_values_preserve_keys_and_non_object_json() {
    let nested = json!({"unknown": {"agent_id": 7, "requester": [false, null]}, "subject": true});
    for opaque in [
        nested.clone(),
        json!([nested.clone(), 7]),
        json!(false),
        json!("scope"),
    ] {
        let mut body = consent();
        body["scope"] = opaque.clone();
        body["duration"] = opaque.clone();
        body["binding"] = opaque.clone();
        let stored = serde_json::to_value(ConsentRequest::decode(body).unwrap()).unwrap();
        for key in ["scope", "duration", "binding"] {
            assert_eq!(stored[key], opaque, "{key}");
        }
    }
    for (mut body, key) in [
        (flow_install(), "flow_install"),
        (flow_decision(), "flow_decision"),
    ] {
        body[key] = nested.clone();
        let stored = serde_json::to_value(ConsentRequest::decode(body).unwrap()).unwrap();
        assert_eq!(stored[key], nested);
        assert_eq!(stored["facts"], json!([]));
    }
}

#[test]
fn preview_and_detail_survive_but_schema_is_checked_then_removed() {
    let mut body = consent();
    body["preview"] = json!({"label": "Diff", "text": "-old\n+new"});
    body["options"][0]["detail"] = json!("Only for this call");
    body["requestedSchema"] = json!({
        "unknown": {"anything": true},
        "properties": {"decision": {"enum": ["deny", "allow_once"], "extra": 7}}
    });
    let request = ConsentRequest::decode(body.clone()).unwrap();
    assert!(request.requested_schema.is_none());
    let stored = serde_json::to_value(&request).unwrap();
    assert_eq!(stored["preview"], body["preview"]);
    assert_eq!(stored["options"][0]["detail"], body["options"][0]["detail"]);
    assert!(stored.get("requestedSchema").is_none());
    assert!(stored.get("requested_schema").is_none());
    assert_eq!(
        serde_json::from_value::<ConsentRequest>(stored).unwrap(),
        request
    );

    body.as_object_mut().unwrap().remove("requestedSchema");
    assert_eq!(ConsentRequest::decode(body.clone()).unwrap(), request);
    body["requestedSchema"] = json!({"properties": {"decision": {"enum": ["allow_once", "deny"]}}});
    assert_eq!(ConsentRequest::decode(body.clone()).unwrap(), request);
    for schema in [
        json!({}),
        json!(false),
        json!({"properties": {"decision": {"enum": [7, "deny"]}}}),
        json!({"properties": {"decision": {"enum": ["deny", "deny", "allow_once"]}}}),
    ] {
        body["requestedSchema"] = schema;
        invalid(body.clone(), SCHEMA);
    }
}

#[test]
fn flow_decision_session_ref_uses_the_full_post_serde_message() {
    for key in ["session_ref", "parent_session_ref", "flow_install"] {
        let mut body = flow_decision();
        body[key] = if key == "flow_install" {
            json!({})
        } else {
            json!("session")
        };
        invalid(
            body,
            "flow_decision cannot carry session references or flow_install",
        );
    }
}

#[test]
fn flow_rules_check_option_effects_counts_and_required_objects() {
    for key in ["flow_install", "flow_decision"] {
        let valid = if key == "flow_install" {
            flow_install()
        } else {
            flow_decision()
        };
        let message = if key == "flow_install" {
            "missing flow_install"
        } else {
            "flow_decision body is required"
        };
        let mut absent = valid.clone();
        absent.as_object_mut().unwrap().remove(key);
        invalid(absent, message);
        let mut null = valid.clone();
        null[key] = Value::Null;
        invalid(null, message);
        for non_object in [json!([]), json!(true), json!("not a card"), json!(42)] {
            let mut body = valid.clone();
            body[key] = non_object;
            serde_refusal(body, "expected a map");
        }
    }
    for count in 0..=5 {
        let mut body = flow_decision();
        let mut options = vec![body["options"][1].clone()];
        for index in 1..count {
            options.push(
                json!({"id": format!("choice_{index}"), "label": "Choose", "effect": "choose"}),
            );
        }
        options.truncate(count);
        body["options"] = json!(options);
        if (2..=4).contains(&count) {
            ConsentRequest::decode(body).unwrap();
        } else {
            invalid(body, FLOW_OPTIONS);
        }
    }
    for effect in ["grant", "decline", "unknown"] {
        let mut body = flow_decision();
        body["options"][0]["effect"] = json!(effect);
        invalid(body, FLOW_OPTIONS);
    }
    let mut body = flow_decision();
    body["options"][1]["effect"] = json!("choose");
    invalid(body, FLOW_OPTIONS);

    for count in [0, 1, 3] {
        let mut body = flow_install();
        let mut options = body["options"].as_array().unwrap().clone();
        options.resize(count, options[0].clone());
        body["options"] = json!(options);
        invalid(body, "flow install needs exactly grant and decline");
    }
    for (index, effect) in [(0, "decline"), (0, "unknown"), (1, "grant"), (1, "unknown")] {
        let mut body = flow_install();
        body["options"][index]["effect"] = json!(effect);
        invalid(body, "flow install needs exactly grant and decline");
    }
    let mut body = consent();
    body["options"][0]["effect"] = json!("choose");
    invalid(body, "choose is only valid for flow_decision");
}

#[test]
fn semantic_refusal_precedence_follows_all_eleven_rules() {
    let mut body = flow_decision();
    body["session_ref"] = json!("session");
    body["options"] = json!([]);
    invalid(
        body,
        "flow_decision cannot carry session references or flow_install",
    );

    let mut body = consent();
    body["flow_decision"] = json!({});
    body["flow_install"] = json!({});
    body["options"][0]["effect"] = json!("choose");
    invalid(body, "flow_decision body requires flow_decision kind");

    let mut body = consent();
    body["flow_install"] = json!({});
    body["options"][0]["effect"] = json!("choose");
    invalid(body, "consent cannot carry flow_install");

    let mut body = flow_decision();
    body.as_object_mut().unwrap().remove("flow_decision");
    body["options"] = json!([]);
    invalid(body, FLOW_OPTIONS);

    let mut body = flow_install();
    body.as_object_mut().unwrap().remove("flow_install");
    body["options"][0]["effect"] = json!("choose");
    invalid(body, "choose is only valid for flow_decision");

    let mut body = flow_install();
    body.as_object_mut().unwrap().remove("flow_install");
    body["options"] = json!([]);
    invalid(body, "missing flow_install");

    let mut body = flow_install();
    body["options"] = json!([]);
    body["title"] = json!("");
    invalid(body, "flow install needs exactly grant and decline");

    let mut body = consent();
    body["kind"] = json!("unknown");
    body["options"][0]["effect"] = json!("choose");
    invalid(body, "choose is only valid for flow_decision");

    let mut body = consent();
    body["kind"] = json!("unknown");
    body["title"] = json!("");
    refusal(
        body,
        "elicitation_unknown_kind",
        "only consent is supported",
    );

    let mut body = consent();
    body["title"] = json!(" ");
    body["options"][0]["id"] = json!("");
    invalid(body, NONEMPTY);

    let mut body = consent();
    body["options"][0]["id"] = json!("deny");
    body["requestedSchema"] = json!({});
    invalid(body, OPTION_IDS);

    let mut body = consent();
    body["requestedSchema"] = json!({});
    body["default"] = json!("missing");
    invalid(body, SCHEMA);

    let mut body = consent();
    body["default"] = json!("missing");
    body["on_expiry"] = json!("allow");
    invalid(body, "default must name an option");

    let mut body = consent();
    body["default"] = json!("allow_once");
    body["urgency"] = json!("urgent");
    refusal(
        body.clone(),
        "elicitation_fail_closed_required",
        FAIL_CLOSED,
    );
    body["options"] = json!([]);
    invalid(body, NONEMPTY);

    let mut body = consent();
    body["urgency"] = json!("urgent");
    body["expires_in_ms"] = json!(0);
    invalid(body, "invalid urgency or late_execution");

    let mut body = consent();
    body["expires_in_ms"] = json!(0);
    body["scope"] = Value::Null;
    invalid(body, "expires_in_ms must be positive and at most 24 hours");

    let mut body = consent();
    body["scope"] = Value::Null;
    body["target"]["label"] = json!("");
    invalid(body, "provide args_digest or scope");
}

#[test]
fn semantic_boundaries_and_optional_defaults() {
    let minimal = ConsentRequest::decode(consent()).unwrap();
    assert!(minimal.facts.is_empty());
    assert!(minimal.preview.is_none());
    assert!(minimal.options[0].detail.is_none());
    assert!(minimal.binding.is_none());
    assert!(minimal.at_console.is_none());

    for expiry in [1, 86_400_000] {
        for urgency in ["low", "normal", "high"] {
            for late_execution in ["execute", "notify_only"] {
                let mut body = consent();
                body["expires_in_ms"] = json!(expiry);
                body["urgency"] = json!(urgency);
                body["late_execution"] = json!(late_execution);
                ConsentRequest::decode(body).unwrap();
            }
        }
    }
    for expiry in [-1, 0, 86_400_001, i64::MAX] {
        let mut body = consent();
        body["expires_in_ms"] = json!(expiry);
        invalid(body, "expires_in_ms must be positive and at most 24 hours");
    }
    for pointer in [
        "/title",
        "/prompt",
        "/options/0/id",
        "/options/0/label",
        "/target/kind",
        "/target/label",
    ] {
        let mut body = consent();
        *body.pointer_mut(pointer).unwrap() = json!(" \t\n");
        let message = match pointer {
            "/title" | "/prompt" => NONEMPTY,
            "/target/kind" | "/target/label" => "target kind and label must be nonempty",
            _ => OPTION_IDS,
        };
        invalid(body, message);
    }
    let mut body = consent();
    body["scope"] = Value::Null;
    body["args_digest"] = json!(" \t");
    invalid(body.clone(), "provide args_digest or scope");
    body["args_digest"] = json!("digest");
    ConsentRequest::decode(body).unwrap();
}

#[test]
fn at_console_is_an_optional_nonblank_string() {
    for value in [json!(""), json!(" \t\n")] {
        let mut body = consent();
        body["at_console"] = value;
        serde_refusal(body, "at_console must be nonempty");
    }
    let mut body = consent();
    body["at_console"] = json!(true);
    serde_refusal(body, "invalid type: boolean `true`, expected a string");
    let mut body = consent();
    body["at_console"] = json!("m1");
    let stored = serde_json::to_value(ConsentRequest::decode(body).unwrap()).unwrap();
    assert_eq!(stored["at_console"], "m1");
}

#[test]
fn session_references_are_preserved_without_selecting_an_identity() {
    let mut body = consent();
    body["session_ref"] = json!("session");
    body["parent_session_ref"] = json!("parent");
    body["dedup_key"] = json!("retry");
    body["facts"] = json!([{"label": "Origin", "value": "local"}]);
    let stored = serde_json::to_value(ConsentRequest::decode(body.clone()).unwrap()).unwrap();
    for key in ["session_ref", "parent_session_ref", "dedup_key", "facts"] {
        assert_eq!(stored[key], body[key]);
    }
    for key in ["agent_id", "subject", "requester"] {
        assert!(stored.get(key).is_none());
    }
}

fn package() -> Value {
    serde_json::from_str::<Value>(include_str!(
        "../../../test-vectors/consent-package-card-v1/request.json"
    ))
    .unwrap()["params"]
        .clone()
}

#[test]
fn package_payload_requires_closed_typed_fields() {
    ConsentRequest::decode(package()).unwrap();
    for field in ["package", "version", "code_hash", "script", "manifest_json"] {
        let mut request = package();
        request["package"].as_object_mut().unwrap().remove(field);
        serde_refusal(request, &format!("missing field `{field}`"));
        let mut request = package();
        request["package"][field] = Value::Null;
        serde_refusal(request, "invalid type: null");
    }
    for field in ["package", "code_hash", "script", "manifest_json"] {
        let mut request = package();
        request["package"][field] = json!({});
        serde_refusal(request, "expected a string");
    }
    for value in [json!("4"), json!(true), json!({})] {
        let mut request = package();
        request["package"]["version"] = value;
        assert_eq!(ConsentRequest::decode(request).unwrap_err().code, INVALID);
    }
    let mut request = package();
    request["package"]["unknown"] = json!(true);
    serde_refusal(request, "unknown field `unknown`");
    for value in [Value::Null, json!([]), json!("package")] {
        let mut request = package();
        request["package"] = value;
        assert_eq!(ConsentRequest::decode(request).unwrap_err().code, INVALID);
    }
}

#[test]
fn package_version_refuses_fractional_and_float_encoded_integers() {
    for version in [json!(4.1), json!(4.0), json!(4e30)] {
        let mut request = package();
        request["package"]["version"] = version;
        invalid(request, "package version must be a JSON integer");
    }
    for version in [json!(0), json!(-1), json!(u64::MAX)] {
        let mut request = package();
        request["package"]["version"] = version;
        ConsentRequest::decode(request).unwrap();
    }
}

#[test]
fn package_hash_requires_exact_lowercase_64_hex() {
    for hash in [
        "a".repeat(63),
        "a".repeat(65),
        "A".repeat(64),
        "g".repeat(64),
        "é".repeat(32),
        format!("{} ", "a".repeat(63)),
    ] {
        let mut request = package();
        request["package"]["code_hash"] = json!(hash);
        invalid(request, "package code_hash must be lowercase 64-hex BLAKE3");
    }
}

#[test]
fn package_manifest_must_be_json_encoded_string() {
    let mut request = package();
    request["package"]["manifest_json"] = json!("not JSON");
    invalid(
        request,
        "package manifest_json must be JSON encoded as a string",
    );
}

#[test]
fn package_kind_requires_its_own_body_and_fail_closed_options() {
    let mut request = package();
    request.as_object_mut().unwrap().remove("package");
    invalid(request, "missing package");
    for kind in ["consent", "flow_install", "flow_decision"] {
        let mut request = package();
        request["kind"] = json!(kind);
        invalid(request, "package body requires package kind");
    }
    for field in ["flow_install", "flow_decision"] {
        let mut request = package();
        request[field] = json!({});
        invalid(request, "package cannot carry flow bodies");
    }
    let mut request = package();
    request["options"][0]["effect"] = json!("other");
    invalid(request, "package needs exactly grant and decline");
    let mut request = package();
    request["default"] = json!("allow");
    refusal(request, "elicitation_fail_closed_required", FAIL_CLOSED);
}
