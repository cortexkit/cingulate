use cingulate_decode::{AccessRequest, ConsentRequest};
use serde_json::{json, Value};

fn request(kind: &str) -> Value {
    let mut body: Value = serde_json::from_str::<Value>(include_str!(
        "../../../test-vectors/consent-package-card-v1/request.json"
    ))
    .unwrap()["params"]
        .clone();
    body["kind"] = json!(kind);
    if kind != "package" {
        body.as_object_mut().unwrap().remove("package");
    }
    if kind == "flow_install" {
        body["flow_install"] = json!({"flow_id":"meadow-digest"});
    }
    body
}

fn structured() -> Value {
    let mut body = request("package");
    body["warnings"] = json!(["Runs only on its timer."]);
    body["comparison"] = json!({"against":"version 2", "note":"Author note", "rows":[
        {"label":"Runs", "change":"changed", "before":"Every 15 minutes", "after":"Every 6 hours"}
    ]});
    body["capabilities"] = json!({"rows":[{"label":"Can read", "value":"Health findings"}]});
    body
}

fn refuses(body: Value) {
    let error = ConsentRequest::decode(body).unwrap_err();
    assert_eq!(error.code, "elicitation_invalid_request");
    assert!(
        !error.message.contains("CALLER_MARKER"),
        "{}",
        error.message
    );
}

#[test]
fn structure_roundtrips_on_both_approval_kinds_and_omits_absent_members() {
    for kind in ["package", "flow_install"] {
        for field in [
            None,
            Some("warnings"),
            Some("comparison"),
            Some("capabilities"),
        ] {
            let mut body = request(kind);
            if let Some(field) = field {
                body[field] = structured()[field].clone();
            }
            let decoded =
                serde_json::to_value(ConsentRequest::decode(body.clone()).unwrap()).unwrap();
            for member in ["warnings", "comparison", "capabilities"] {
                assert_eq!(decoded.get(member), body.get(member));
            }
        }
    }
}

#[test]
fn structure_array_bounds_refuse_empty_and_excess_entries() {
    for (path, max) in [
        ("/warnings", 4),
        ("/comparison/rows", 24),
        ("/capabilities/rows", 24),
    ] {
        for count in [1, max] {
            let mut body = structured();
            let entry = body.pointer(path).unwrap()[0].clone();
            *body.pointer_mut(path).unwrap() = Value::Array(vec![entry; count]);
            ConsentRequest::decode(body).unwrap();
        }
        for count in [0, max + 1] {
            let mut body = structured();
            let entry = body.pointer(path).unwrap()[0].clone();
            *body.pointer_mut(path).unwrap() = Value::Array(vec![entry; count]);
            refuses(body);
        }
    }
}

#[test]
fn structure_text_bounds_count_utf8_bytes() {
    for (path, max) in [
        ("/warnings/0", 300),
        ("/comparison/against", 120),
        ("/comparison/note", 1200),
        ("/comparison/rows/0/label", 64),
        ("/comparison/rows/0/before", 1200),
        ("/comparison/rows/0/after", 1200),
        ("/capabilities/rows/0/label", 64),
        ("/capabilities/rows/0/value", 1200),
    ] {
        for value in ["a".repeat(max), "é".repeat(max / 2)] {
            let mut body = structured();
            *body.pointer_mut(path).unwrap() = json!(value);
            ConsentRequest::decode(body).unwrap();
        }
        for value in [
            String::new(),
            "a".repeat(max + 1),
            "é".repeat(max / 2) + "a",
        ] {
            let mut body = structured();
            *body.pointer_mut(path).unwrap() = json!(value);
            refuses(body);
        }
    }
}

#[test]
fn structure_short_text_refuses_newline_and_all_other_controls() {
    for path in [
        "/warnings/0",
        "/comparison/against",
        "/comparison/rows/0/label",
        "/capabilities/rows/0/label",
    ] {
        for scalar in (0..=0x1f).chain(0x7f..=0x9f) {
            let mut body = structured();
            *body.pointer_mut(path).unwrap() =
                json!(format!("a{}b", char::from_u32(scalar).unwrap()));
            refuses(body);
        }
    }
}

#[test]
fn structure_long_text_allows_newline_but_refuses_other_controls() {
    for path in [
        "/comparison/note",
        "/comparison/rows/0/before",
        "/comparison/rows/0/after",
        "/capabilities/rows/0/value",
    ] {
        for scalar in (0..=0x1f).chain(0x7f..=0x9f) {
            let mut body = structured();
            let character = char::from_u32(scalar).unwrap();
            *body.pointer_mut(path).unwrap() = json!(format!("a{character}b"));
            if character == '\n' {
                ConsentRequest::decode(body).unwrap();
            } else {
                refuses(body);
            }
        }
    }
}

#[test]
fn comparison_change_is_an_open_lowercase_ascii_token_with_byte_bounds() {
    for change in [
        "a".into(),
        "_".into(),
        "a".repeat(32),
        "future_change".into(),
    ] {
        let mut body = structured();
        body["comparison"]["rows"][0]["change"] = json!(change);
        ConsentRequest::decode(body).unwrap();
    }
    for change in [
        "",
        "Changed",
        "changed-thing",
        "a b",
        "é",
        "a\n",
        "a0",
        &"a".repeat(33),
    ] {
        let mut body = structured();
        body["comparison"]["rows"][0]["change"] = json!(change);
        refuses(body);
    }
}

#[test]
fn comparison_change_presence_rules_are_enforced() {
    for change in ["changed", "added", "removed", "future_change"] {
        for before in [false, true] {
            for after in [false, true] {
                let mut body = structured();
                let row = body["comparison"]["rows"][0].as_object_mut().unwrap();
                row.insert("change".into(), json!(change));
                if !before {
                    row.remove("before");
                }
                if !after {
                    row.remove("after");
                }
                let valid = match change {
                    "changed" => after,
                    "added" => after && !before,
                    "removed" => before && !after,
                    _ => before || after,
                };
                if valid {
                    ConsentRequest::decode(body).unwrap();
                } else {
                    refuses(body);
                }
            }
        }
    }
}

#[test]
fn structure_shapes_are_closed_and_null_is_not_absence() {
    for path in [
        "/warnings",
        "/warnings/0",
        "/comparison",
        "/comparison/against",
        "/comparison/note",
        "/comparison/rows",
        "/comparison/rows/0",
        "/comparison/rows/0/label",
        "/comparison/rows/0/change",
        "/comparison/rows/0/before",
        "/comparison/rows/0/after",
        "/capabilities",
        "/capabilities/rows",
        "/capabilities/rows/0",
        "/capabilities/rows/0/label",
        "/capabilities/rows/0/value",
    ] {
        for value in [Value::Null, json!(false), json!(12)] {
            let mut body = structured();
            *body.pointer_mut(path).unwrap() = value;
            refuses(body);
        }
    }
    for path in [
        "/comparison",
        "/comparison/rows/0",
        "/capabilities",
        "/capabilities/rows/0",
    ] {
        let mut body = structured();
        body.pointer_mut(path).unwrap()["CALLER_MARKER"] = json!(true);
        refuses(body);
    }
    for (path, member) in [
        ("/comparison", "against"),
        ("/comparison", "rows"),
        ("/comparison/rows/0", "label"),
        ("/comparison/rows/0", "change"),
        ("/capabilities", "rows"),
        ("/capabilities/rows/0", "label"),
        ("/capabilities/rows/0", "value"),
    ] {
        let mut body = structured();
        body.pointer_mut(path)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(member);
        refuses(body);
    }
}

#[test]
fn structure_is_refused_on_other_kinds_before_normalization() {
    for kind in [
        "consent",
        "flow_decision",
        "secret_use",
        "config_plan",
        "manifest_binding",
        "unknown",
    ] {
        for field in ["warnings", "comparison", "capabilities"] {
            for value in [structured()[field].clone(), Value::Null] {
                let mut body = request(kind);
                body[field] = value;
                let error = ConsentRequest::decode(body).unwrap_err();
                assert_eq!(
                    error.message,
                    format!("{field} is only valid for flow_install or package cards")
                );
            }
        }
    }
    for kind in ["secret_use", "config_plan", "manifest_binding"] {
        for field in ["explanation", "changes"] {
            let mut body = request(kind);
            body[field] = Value::Null;
            let error = ConsentRequest::decode(body).unwrap_err();
            assert_eq!(
                error.message,
                format!("{field} is only valid for flow_install or package cards")
            );
        }
    }
    for sample in [
        include_str!("../../../test-vectors/tool-permission-v1/user-open-request.json"),
        include_str!("../../../test-vectors/tool-permission-v1/macos-open-request.json"),
    ] {
        let base = serde_json::from_str::<Value>(sample).unwrap()["params"].clone();
        for field in ["warnings", "comparison", "capabilities"] {
            let mut body = base.clone();
            body[field] = structured()[field].clone();
            let error = AccessRequest::decode(body).unwrap_err();
            assert_eq!(error.message, "permission request fields are invalid");
        }
    }
}
