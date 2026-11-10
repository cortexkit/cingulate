//! Extended secret statements from the vault's pinned multi-host corpus.

#[path = "support/flow_secrets.rs"]
mod flow_secrets;
#[path = "support/secret_use.rs"]
mod secret_use;
mod support;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use cingulate_decode::ConsentRequest;
use secret_use::{encoded, filer, fixture, phone, DEVICE, NOW};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const USE_SAMPLES: &[u8] =
    include_bytes!("../../../test-vectors/secret-statements-v3/secret-approval-v3.json");
const FLOW_SAMPLES: &[u8] =
    include_bytes!("../../../test-vectors/secret-statements-v3/secret-flow-approval-v3.json");

fn samples(flow: bool) -> Value {
    serde_json::from_slice(if flow { FLOW_SAMPLES } else { USE_SAMPLES }).unwrap()
}

fn statement(flow: bool, case: &str) -> Value {
    samples(flow)["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|sample| sample["case"] == case)
        .unwrap()["statement"]
        .clone()
}

fn body(flow: bool, statement: &Value) -> Value {
    let items = json!([{
        "request_id": statement["request_id"],
        "statement_b64url": encoded(statement)
    }]);
    if flow {
        let mut body = flow_secrets::body();
        body["flow_install"]["flow_identity"] = statement["flow"].clone();
        body["flow_install"]["secrets"] = items;
        body
    } else {
        json!({"kind":"secret_use", "secret_use":{"items":items}})
    }
}

fn refuses(flow: bool, statement: &Value) {
    let error = ConsentRequest::decode(body(flow, statement)).unwrap_err();
    assert_eq!(
        error.code,
        if flow {
            "elicitation_invalid_flow_secrets"
        } else {
            "elicitation_invalid_request"
        },
        "{statement}"
    );
}

#[test]
fn pinned_v3_samples_are_byte_identical_to_claustrum() {
    for (flow, bytes, digest) in [
        (
            false,
            USE_SAMPLES,
            "4a04620b93a162d27ca789ccf732b35e692e7dc07f2624717d8a593e82ac6433",
        ),
        (
            true,
            FLOW_SAMPLES,
            "fb83e06bbe3b4076b196fc1312b4d1ed086c47463efc4569573e17fc8d8ad544",
        ),
    ] {
        assert_eq!(format!("{:x}", Sha256::digest(bytes)), digest);
        for sample in samples(flow)["cases"].as_array().unwrap() {
            if sample.get("statement").is_none() {
                continue;
            }
            let bytes = serde_jcs::to_vec(&sample["statement"]).unwrap();
            let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            assert_eq!(hex, sample["statement_jcs_hex"], "{}", sample["case"]);
            assert_eq!(
                format!("{:x}", Sha256::digest(bytes)),
                sample["statement_sha256"],
                "{}",
                sample["case"]
            );
        }
    }
}

async fn accepts_vault_samples(flow: bool) {
    let mut accepted = 0;
    for sample in samples(flow)["cases"].as_array().unwrap() {
        if sample["expect"]["status"] != "approval_required" {
            continue;
        }
        let request = body(flow, &sample["statement"]);
        ConsentRequest::decode(request.clone()).unwrap();
        let f = fixture().await;
        let sender = filer(
            if flow { "prefrontal-core" } else { "aft" },
            if flow { None } else { Some("agent-A") },
        );
        let id = f.request(&sender, request.clone()).await;
        assert_eq!(f.stored_record(&id).expires_at_ms, Some(1_700_000_600_000));
        let card = f
            .ok(
                &phone(DEVICE),
                "consent.get",
                json!({"elicitationId":id,"clientFeatures":["secret-use-approval"]}),
            )
            .await;
        if flow {
            assert_eq!(card["flowInstall"], request["flow_install"]);
        } else {
            assert_eq!(card["secretUse"], request["secret_use"]);
        }
        let envelope = json!({
            "v":"ck-secret-use-envelope/v1", "decision":"allow",
            "device_fingerprint":DEVICE, "duration":if flow {"version"} else {"hour"},
            "grant_expires_at_ms":if flow {0} else {NOW + 3_600_000}, "issued_at_ms":NOW,
            "items":[{"request_id":sample["statement"]["request_id"],
                      "statement_sha256":sample["statement_sha256"]}]
        });
        let answer = json!({"elicitationId":id,"choiceId":if flow {"install"} else {"allow"},
            "clientFeatures":["secret-use-approval"],"envelope_b64url":encoded(&envelope)});
        assert_eq!(
            f.ok(&phone(DEVICE), "consent.answer", answer).await["ok"],
            true,
            "{}",
            sample["case"]
        );
        let record = f.stored_record(&id);
        assert_eq!(record.answered_device.as_deref(), Some(DEVICE));
        let binding = record.secret_use_answer.unwrap();
        assert_eq!(binding.decision, "allow");
        assert_eq!(
            binding.duration.as_deref(),
            Some(if flow { "version" } else { "hour" })
        );
        assert_eq!(
            binding.envelope_sha256,
            Some(format!(
                "{:x}",
                Sha256::digest(
                    URL_SAFE_NO_PAD
                        .decode(encoded(&envelope).as_str().unwrap())
                        .unwrap()
                )
            ))
        );
        assert_eq!(binding.items.len(), 1);
        assert_eq!(binding.items[0].decision, "allow");
        accepted += 1;
    }
    assert_eq!(accepted, 8);
}

#[tokio::test]
async fn v3_secret_use_samples_file_project_and_approve() {
    accepts_vault_samples(false).await;
}

#[tokio::test]
async fn v3_flow_samples_file_project_and_approve() {
    accepts_vault_samples(true).await;
}

#[test]
fn v3_tag_refuses_legacy_use() {
    for flow in [false, true] {
        for case in ["local-lone-wildcard-no-prefix", "forwarded-two-origins"] {
            let mut s = statement(flow, case);
            s["use"] = if s["secret"]["kind"] == "site_login" {
                json!({"origin":"https://x.test"})
            } else if flow {
                json!({"host":"x.test","path_prefix":"","location":"header:X-Api-Key","methods":["GET"]})
            } else {
                json!({"host":"x.test","path_prefix":"","location":"header:X-Api-Key"})
            };
            refuses(flow, &s);
        }
    }
}

#[test]
fn legacy_statements_still_require_emitted_path_prefix() {
    for flow in [false, true] {
        for (version, prefix) in [("v1", "local"), ("v2", "forwarded")] {
            let mut s = statement(flow, &format!("{prefix}-eight-rules"));
            s["v"] = json!(format!(
                "ck-secret-{}-statement/{version}",
                if flow { "flow" } else { "use" }
            ));
            s["use"] = json!({"host":"x.test","path_prefix":"","location":"header:X-Api-Key"});
            if flow {
                s["use"]["methods"] = json!(["GET"]);
            }
            ConsentRequest::decode(body(flow, &s)).unwrap();
            s["use"].as_object_mut().unwrap().remove("path_prefix");
            refuses(flow, &s);
        }
    }
}

#[test]
fn v1_v2_tags_refuse_extended_use() {
    for flow in [false, true] {
        for (version, prefix) in [("v1", "local"), ("v2", "forwarded")] {
            for scope in ["eight-rules", "two-origins"] {
                let mut s = statement(flow, &format!("{prefix}-{scope}"));
                s["v"] = json!(format!(
                    "ck-secret-{}-statement/{version}",
                    if flow { "flow" } else { "use" }
                ));
                refuses(flow, &s);
            }
        }
    }
}

#[test]
fn v3_forwarded_asker_cannot_be_serving_vault() {
    for flow in [false, true] {
        let mut s = statement(flow, "forwarded-eight-rules");
        s["asking_member_id"] = s["member_id"].clone();
        refuses(flow, &s);
    }
}

#[test]
fn v3_asker_must_be_a_present_computer_string() {
    for flow in [false, true] {
        for asker in [
            Value::Null,
            json!(""),
            json!("fed-phone:11111111111111111111111111111111"),
            json!("fed-computer:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
            json!("fed-computer:1111111111111111111111111111111"),
        ] {
            let mut s = statement(flow, "forwarded-eight-rules");
            s["asking_member_id"] = asker;
            refuses(flow, &s);
        }
    }
}

#[test]
fn v3_api_key_refuses_login_scope() {
    for flow in [false, true] {
        let mut s = statement(flow, "local-two-origins");
        s["secret"]["kind"] = json!("agent_api_key");
        refuses(flow, &s);
    }
}

#[test]
fn v3_login_refuses_api_key_scope() {
    for flow in [false, true] {
        let mut s = statement(flow, "local-eight-rules");
        s["secret"]["kind"] = json!("site_login");
        refuses(flow, &s);
    }
}

#[test]
fn v3_unknown_kind_is_refused() {
    for flow in [false, true] {
        for case in ["local-eight-rules", "local-two-origins"] {
            let mut s = statement(flow, case);
            s["secret"]["kind"] = json!("unknown");
            refuses(flow, &s);
        }
    }
}

#[test]
fn v3_use_shapes_are_closed() {
    for flow in [false, true] {
        for case in ["local-eight-rules", "local-two-origins"] {
            for (key, value) in [
                ("extra", json!(true)),
                ("host", json!("x.test")),
                ("origin", json!("https://x.test")),
                ("path_prefix", json!("/v1")),
            ] {
                let mut s = statement(flow, case);
                s["use"][key] = value;
                refuses(flow, &s);
            }
        }
        for rules in [
            json!([{"host":"*.x.test"}]),
            json!([{"path_prefix":""}]),
            json!([{"host":"*.x.test","path_prefix":"","extra":true}]),
            json!([{"host":"*.x.test","path_prefix":null}]),
            json!([{"host":5,"path_prefix":""}]),
            json!({"host":"*.x.test","path_prefix":""}),
        ] {
            let mut s = statement(flow, "local-eight-rules");
            s["use"]["rules"] = rules;
            refuses(flow, &s);
        }
        for origins in [json!("https://x.test"), json!([5]), json!([null])] {
            let mut s = statement(flow, "local-two-origins");
            s["use"]["origins"] = origins;
            refuses(flow, &s);
        }
        let mut mixed = statement(flow, "local-eight-rules");
        mixed["use"]["origins"] = json!(["https://x.test", "https://y.test"]);
        refuses(flow, &mixed);
        let mut missing = statement(flow, "local-eight-rules");
        missing["use"].as_object_mut().unwrap().remove("location");
        refuses(flow, &missing);
    }
}

#[test]
fn v3_flow_methods_remain_required_sorted_and_valid() {
    for methods in [
        None,
        Some(json!([])),
        Some(json!(["POST", "GET"])),
        Some(json!(["GET", "GET"])),
        Some(json!(["get"])),
    ] {
        let mut s = statement(true, "local-eight-rules");
        if let Some(methods) = methods {
            s["use"]["methods"] = methods;
        } else {
            s["use"].as_object_mut().unwrap().remove("methods");
        }
        refuses(true, &s);
    }
    let mut login = statement(true, "local-two-origins");
    login["use"]["methods"] = json!(["GET"]);
    refuses(true, &login);
}

fn reserved_flow_ids(case: &str, version: &str, tags: &[&str]) {
    for tag in tags {
        let mut s = statement(true, case);
        s["v"] = json!(format!("ck-secret-flow-statement/{version}"));
        if version == "v2" {
            s["use"] = json!({"host":"x.test","path_prefix":"","location":"header:X-Api-Key","methods":["GET"]});
        }
        s["flow"]["flow_id"] = json!(tag);
        refuses(true, &s);
    }
}

const V2_TAGS: &[&str] = &["ck-secret-use-statement/v2", "ck-secret-flow-statement/v2"];
const V3_TAGS: &[&str] = &["ck-secret-use-statement/v3", "ck-secret-flow-statement/v3"];

#[test]
fn v2_forwarded_flow_refuses_v2_reserved_tags() {
    reserved_flow_ids("forwarded-eight-rules", "v2", V2_TAGS);
}

#[test]
fn v3_local_flow_refuses_v3_reserved_tags() {
    reserved_flow_ids("local-eight-rules", "v3", V3_TAGS);
}

#[test]
fn v3_forwarded_flow_refuses_v3_reserved_tags() {
    reserved_flow_ids("forwarded-eight-rules", "v3", V3_TAGS);
}

#[test]
fn v3_forwarded_flow_refuses_v2_reserved_tags() {
    reserved_flow_ids("forwarded-eight-rules", "v3", V2_TAGS);
}

#[test]
fn v3_local_flow_accepts_v2_tag_flow_id() {
    for tag in V2_TAGS {
        let mut s = statement(true, "local-eight-rules");
        s["flow"]["flow_id"] = json!(tag);
        ConsentRequest::decode(body(true, &s)).unwrap();
    }
}

#[test]
fn flow_card_refuses_mixed_serving_vaults_for_every_version() {
    for (version, prefix) in [("v1", "local"), ("v2", "forwarded"), ("v3", "local")] {
        let mut first = statement(true, &format!("{prefix}-eight-rules"));
        first["v"] = json!(format!("ck-secret-flow-statement/{version}"));
        if version != "v3" {
            first["use"] = json!({"host":"x.test","path_prefix":"","location":"header:X-Api-Key","methods":["GET"]});
        }
        let mut request = body(true, &first);
        let mut second = first.clone();
        second["request_id"] = json!("01HF7YAT000000000000000002");
        let append = |request: &mut Value, s: &Value| {
            request["flow_install"]["secrets"]
                .as_array_mut()
                .unwrap()
                .push(json!({"request_id":s["request_id"],"statement_b64url":encoded(s)}));
        };
        append(&mut request, &second);
        ConsentRequest::decode(request.clone()).unwrap();
        second["member_id"] = json!("fed-computer:33333333333333333333333333333333");
        request["flow_install"]["secrets"]
            .as_array_mut()
            .unwrap()
            .pop();
        append(&mut request, &second);
        assert_eq!(
            ConsentRequest::decode(request).unwrap_err().code,
            "elicitation_invalid_flow_secrets",
            "{version}"
        );
    }
}
