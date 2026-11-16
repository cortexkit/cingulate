use cingulate_decode::{AccessRequest, ConsentRequest};
use serde_json::{json, Value};

fn request(kind: &str) -> Value {
    if kind == "secret_use" {
        return serde_json::from_str::<Value>(include_str!(
            "../../../test-vectors/secret-use-approval-v2/filer-request.json"
        ))
        .unwrap()["params"]
            .clone();
    }
    let mut body = serde_json::from_str::<Value>(include_str!(
        "../../../test-vectors/consent-package-card-v1/request.json"
    ))
    .unwrap()["params"]
        .clone();
    body["kind"] = json!(kind);
    if kind != "package" {
        body.as_object_mut().unwrap().remove("package");
    }
    if kind == "flow_install" {
        body["flow_install"] = json!({"flow_id": "meadow-digest"});
    }
    if kind == "flow_decision" {
        body["flow_decision"] = json!({});
        body["options"][0]["effect"] = json!("choose");
    }
    body
}

#[test]
fn accepts_explanations_on_package_and_flow_install() {
    for kind in ["package", "flow_install"] {
        let mut body = request(kind);
        body["explanation"] = json!({"label": "What this flow does", "text": "Collects a morning digest.\nOnly runs when enabled."});
        body["changes"] = json!({"label": "What changed", "text": "Adds a weekly summary."});
        let decoded = ConsentRequest::decode(body.clone()).unwrap();
        let roundtrip = serde_json::to_value(decoded).unwrap();
        for field in ["explanation", "changes"] {
            assert_eq!(roundtrip[field], body[field], "{kind} {field}");
        }
    }
}

fn invalid(body: Value, field: &str) {
    let shape_error = serde_json::from_value::<ConsentRequest>(body.clone()).is_err();
    let error = ConsentRequest::decode(body).unwrap_err();
    assert_eq!(error.code, "elicitation_invalid_request");
    if error.message == "consent request fields are invalid" {
        assert!(shape_error);
    } else {
        assert!(error.message.contains(field), "{}", error.message);
    }
}

#[test]
fn explanation_fields_are_only_allowed_on_approval_cards() {
    for kind in ["consent", "flow_decision", "secret_use", "unknown"] {
        if kind != "unknown" {
            ConsentRequest::decode(request(kind)).unwrap();
        }
        for field in ["explanation", "changes"] {
            let mut body = request(kind);
            body[field] = json!({"label": "About", "text": "Author text"});
            invalid(body, field);
        }
    }
}

#[test]
fn permission_ask_decoders_refuse_explanation_fields_with_fixed_messages() {
    for sample in [
        include_str!("../../../test-vectors/tool-permission-v1/user-open-request.json"),
        include_str!("../../../test-vectors/tool-permission-v1/macos-open-request.json"),
    ] {
        let body = serde_json::from_str::<Value>(sample).unwrap()["params"].clone();
        AccessRequest::decode(body.clone()).unwrap();
        for field in ["explanation", "changes"] {
            let mut body = body.clone();
            body[field] = json!({"label": "About", "text": "Author text"});
            let error = AccessRequest::decode(body).unwrap_err();
            assert_eq!(error.code, "elicitation_invalid_request");
            assert_eq!(error.message, "permission request fields are invalid");
        }
    }
}

#[test]
fn explanation_caps_count_utf8_bytes_and_unicode_scalars() {
    for field in ["explanation", "changes"] {
        for kind in ["package", "flow_install"] {
            for (label, text) in [
                ("a".repeat(64), "b".repeat(8192)),
                ("🌿".repeat(64), "🌿".repeat(2048)),
                ("e\u{301}".repeat(32), "é".repeat(4096)),
                ("About".into(), "b".repeat(6000)),
                ("\n".into(), " \n".into()),
            ] {
                let mut body = request(kind);
                body[field] = json!({"label": label, "text": text});
                assert!(ConsentRequest::decode(body).is_ok());
            }
        }
        for (member, value) in [
            ("label", String::new()),
            ("text", String::new()),
            ("label", "a".repeat(65)),
            ("label", "🌿".repeat(65)),
            ("label", "e\u{301}".repeat(32) + "e"),
            ("text", "b".repeat(8193)),
            ("text", "🌿".repeat(2048) + "a"),
            ("text", "é".repeat(4096) + "a"),
        ] {
            let mut body = request("package");
            body[field] = json!({"label": "About", "text": "Author text"});
            body[field][member] = json!(value);
            invalid(body, &format!("{field}.{member}"));
        }
    }
}

#[test]
fn explanation_controls_are_rejected_except_newline() {
    for field in ["explanation", "changes"] {
        for member in ["label", "text"] {
            for scalar in (0..=0x1f).chain(0x7f..=0x9f) {
                let character = char::from_u32(scalar).unwrap();
                let mut body = request("package");
                body[field] = json!({"label": "About", "text": "Author text"});
                body[field][member] = json!(format!("a{character}b"));
                if character == '\n' {
                    assert!(ConsentRequest::decode(body).is_ok());
                } else {
                    invalid(body, &format!("{field}.{member}"));
                }
            }
            for character in ['\u{20}', '\u{7e}', '\u{a0}', '🌿'] {
                let mut body = request("package");
                body[field] = json!({"label": "About", "text": "Author text"});
                body[field][member] = json!(character.to_string());
                assert!(ConsentRequest::decode(body).is_ok());
            }
        }
    }
}

#[test]
fn explanation_shapes_and_unknown_members_remain_closed() {
    for field in ["explanation", "changes"] {
        for value in [Value::Null, json!("text"), json!([]), json!(false)] {
            let mut body = request("package");
            body[field] = value;
            invalid(body, field);
        }
        for member in ["label", "text"] {
            for value in [Value::Null, json!(1), json!({}), json!(false)] {
                let mut body = request("package");
                body[field] = json!({"label": "About", "text": "Author text"});
                body[field][member] = value;
                invalid(body, &format!("{field}.{member}"));
            }
            let mut body = request("package");
            body[field] = json!({"label": "About", "text": "Author text"});
            body[field].as_object_mut().unwrap().remove(member);
            invalid(body, &format!("{field}.{member}"));
        }
        let mut body = request("package");
        body[field] = json!({"label": "About", "text": "Author text", "extra": true});
        invalid(body, &format!("{field}.extra"));
    }
    let mut body = request("package");
    body["explanations"] = json!({"label": "About", "text": "Author text"});
    invalid(body, "explanations");
}

#[test]
fn absent_explanation_fields_are_omitted_and_independent() {
    for kind in ["package", "flow_install"] {
        for field in [None, Some("explanation"), Some("changes")] {
            let mut body = request(kind);
            if let Some(field) = field {
                body[field] = json!({"label": "About", "text": "Author text"});
            }
            let decoded =
                serde_json::to_value(ConsentRequest::decode(body.clone()).unwrap()).unwrap();
            for member in ["explanation", "changes"] {
                assert_eq!(decoded.get(member), body.get(member));
            }
        }
    }
}
