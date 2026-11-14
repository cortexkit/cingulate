use cingulate_decode::{AccessRequest, ConsentRequest, Refusal, SecretUse};
use serde_json::{json, Value};

#[test]
fn reasons_are_optional_and_do_not_change_refusal_text() {
    let refusal = Refusal::new("test_code", "private message");
    assert_eq!(refusal.reason, None);
    let labeled = refusal.clone().reason("statement_invalid");
    assert_eq!(labeled.reason, Some("statement_invalid"));
    assert_eq!(labeled.code, refusal.code);
    assert_eq!(labeled.message, refusal.message);
    assert_eq!(labeled.to_string(), refusal.to_string());
}

#[test]
fn malformed_request_shapes_have_fixed_reasons() {
    for refusal in [
        ConsentRequest::decode(json!({})).unwrap_err(),
        ConsentRequest::decode(json!({"kind":"secret_use", "secret_use":{}})).unwrap_err(),
        ConsentRequest::decode(json!({"kind":"manifest_binding"})).unwrap_err(),
        AccessRequest::decode(json!({})).unwrap_err(),
    ] {
        assert_eq!(refusal.code, "elicitation_invalid_request");
        assert_eq!(refusal.reason, Some("envelope_invalid"));
    }
}

#[test]
fn secret_statements_and_answer_envelopes_have_distinct_reasons() {
    let vectors: Value = serde_json::from_str(include_str!(
        "../../../test-vectors/secret-use-approval-v1/secret-approval-v1.json"
    ))
    .unwrap();
    let vector = vectors["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|vector| vector["case"] == "approval")
        .unwrap();
    let mut secret_use: SecretUse = serde_json::from_value(json!({"items":[{
        "request_id":vector["statements"][0]["request_id"],
        "statement_b64url":vector["statements"][0]["statement_b64url"]
    }]}))
    .unwrap();
    let envelope_refusal = secret_use
        .answer("allow", None, Some("not base64"))
        .unwrap_err();
    assert_eq!(envelope_refusal.reason, Some("envelope_invalid"));
    secret_use.items[0].statement_b64url = "not base64".into();
    let statement_refusal = secret_use.statements().unwrap_err();
    assert_eq!(statement_refusal.reason, Some("statement_invalid"));
    assert_eq!(statement_refusal.code, "elicitation_invalid_request");
    assert_eq!(envelope_refusal.code, statement_refusal.code);
    assert_eq!(envelope_refusal.message, statement_refusal.message);
}
