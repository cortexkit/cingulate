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
    filing_refusal(flow, body(flow, statement));
}

fn filing_refusal(flow: bool, request: Value) -> String {
    let error = ConsentRequest::decode(request.clone()).unwrap_err();
    assert_eq!(
        error.code,
        if flow {
            "elicitation_invalid_flow_secrets"
        } else {
            "elicitation_invalid_request"
        },
        "{request}"
    );
    error.message
}

fn items(flow: bool, request: &mut Value) -> &mut Value {
    if flow {
        &mut request["flow_install"]["secrets"]
    } else {
        &mut request["secret_use"]["items"]
    }
}

fn refuses_with(flow: bool, request: Value, index: usize, problem: &str) {
    let field = if flow {
        "flow_install.secrets"
    } else {
        "secret_use.items"
    };
    assert_eq!(
        filing_refusal(flow, request),
        format!("{field}[{index}] statement {problem}")
    );
}

fn api_key_statement(flow: bool, version: u8) -> Value {
    let case = if version == 2 {
        "forwarded-eight-rules"
    } else {
        "local-lone-wildcard-no-prefix"
    };
    let mut s = statement(flow, case);
    if version != 3 {
        let family = if flow { "flow" } else { "use" };
        s["v"] = json!(format!("ck-secret-{family}-statement/v{version}"));
        s["use"] = json!({"host":"x.test","path_prefix":"","location":"header:X-Api-Key"});
        if flow {
            s["use"]["methods"] = json!(["GET"]);
        }
    }
    s
}

#[test]
fn statement_bytes_must_equal_canonical_json() {
    for flow in [false, true] {
        for version in 1..=3 {
            let s = api_key_statement(flow, version);
            let mut request = body(flow, &s);
            let mut bytes = serde_jcs::to_vec(&s).unwrap();
            bytes.push(b' ');
            items(flow, &mut request)[0]["statement_b64url"] = json!(URL_SAFE_NO_PAD.encode(bytes));
            refuses_with(
                flow,
                request,
                0,
                "bytes must equal their canonical JSON encoding",
            );
        }
    }
}

#[test]
fn reserved_header_location_refuses_in_either_statement_family() {
    for flow in [false, true] {
        for version in 1..=3 {
            for name in ["Host", "cOoKiE", "CONTENT-LENGTH", "aUtHoRiZaTiOn"] {
                let mut s = api_key_statement(flow, version);
                s["use"]["location"] = json!(format!("header:{name}"));
                refuses_with(flow, body(flow, &s), 0, "use.location must not name a reserved header (Host, Cookie, Content-Length or Authorization)");
            }
        }
    }
}

#[test]
fn api_key_location_requires_one_of_the_supported_forms() {
    for flow in [false, true] {
        for version in 1..=3 {
            let mut s = api_key_statement(flow, version);
            s["use"]["location"] = json!("authorization_bearer:extra");
            refuses_with(flow, body(flow, &s), 0, "use.location must be authorization_bearer, header:, query:, json_pointer: or authorization_basic:");
        }
    }
}

fn api_key_location_examples(valid: &[String], invalid: &[String], message: &str) {
    for flow in [false, true] {
        for version in 1..=3 {
            let mut s = api_key_statement(flow, version);
            for location in valid {
                s["use"]["location"] = json!(location);
                ConsentRequest::decode(body(flow, &s)).unwrap();
            }
            for location in invalid {
                s["use"]["location"] = json!(location);
                refuses_with(flow, body(flow, &s), 0, message);
            }
        }
    }
}

#[test]
fn api_key_header_is_an_http_token_within_byte_limits() {
    api_key_location_examples(
        &[
            "authorization_bearer".into(),
            "header:X!#$%&'*+-.^_`|~".into(),
            format!("header:{}", "a".repeat(64)),
        ],
        &[
            "header:".into(),
            "header:x/y".into(),
            format!("header:{}", "a".repeat(65)),
        ],
        "use.location header name must be a 1–64 byte HTTP token",
    );
}

#[test]
fn api_key_query_name_is_within_byte_limits_and_uses_only_allowed_bytes() {
    api_key_location_examples(
        &["query:aZ0._-".into(), format!("query:{}", "a".repeat(64))],
        &[
            "query:".into(),
            "query:key+".into(),
            format!("query:{}", "a".repeat(65)),
        ],
        "use.location query name must be 1–64 bytes of A-Za-z0-9._-",
    );
}

#[test]
fn api_key_pointer_starts_with_slash_and_fits_within_256_bytes() {
    api_key_location_examples(
        &[
            "json_pointer:/".into(),
            format!("json_pointer:/{}", "a".repeat(255)),
        ],
        &[
            "json_pointer:root".into(),
            format!("json_pointer:/{}", "a".repeat(256)),
        ],
        "use.location JSON pointer must start with / and be at most 256 bytes",
    );
}

#[test]
fn api_key_pointer_uses_only_allowed_bytes() {
    api_key_location_examples(
        &["json_pointer:/!/~0/~1".into()],
        &[
            "json_pointer:/a b".into(),
            "json_pointer:/é".into(),
            "json_pointer:/\u{7f}".into(),
        ],
        "use.location JSON pointer contains a disallowed byte",
    );
}

#[test]
fn api_key_pointer_escapes_tilde() {
    api_key_location_examples(
        &["json_pointer:/~01~1".into()],
        &["json_pointer:/~".into(), "json_pointer:/~2".into()],
        "use.location JSON pointer must escape ~ as ~0 or ~1",
    );
}

#[test]
fn api_key_basic_user_meets_length_and_character_limits() {
    api_key_location_examples(
        &[
            "authorization_basic:aZ0._@+-".into(),
            format!("authorization_basic:{}", "a".repeat(128)),
        ],
        &[
            "authorization_basic:".into(),
            "authorization_basic:a:b".into(),
            format!("authorization_basic:{}", "a".repeat(129)),
        ],
        "use.location Basic user has an invalid length or character",
    );
}

#[test]
fn flow_methods_require_at_least_one_entry() {
    for version in 1..=3 {
        let mut s = api_key_statement(true, version);
        s["use"]["methods"] = json!([]);
        refuses_with(
            true,
            body(true, &s),
            0,
            "use.methods must contain at least one method",
        );
    }
}

#[test]
fn flow_methods_are_one_to_sixteen_uppercase_ascii_letters() {
    for version in 1..=3 {
        let mut s = api_key_statement(true, version);
        s["use"]["methods"] = json!(["ABCDEFGHIJKLMNOP"]);
        ConsentRequest::decode(body(true, &s)).unwrap();
        for method in ["", "Get", "GET2", "ABCDEFGHIJKLMNOPQ"] {
            s["use"]["methods"] = json!([method]);
            refuses_with(
                true,
                body(true, &s),
                0,
                "each use.methods entry must be 1–16 uppercase ASCII letters",
            );
        }
    }
}

#[test]
fn flow_methods_must_be_strictly_ascending() {
    for version in 1..=3 {
        let mut s = api_key_statement(true, version);
        for methods in [["POST", "GET"], ["GET", "GET"]] {
            s["use"]["methods"] = json!(methods);
            refuses_with(
                true,
                body(true, &s),
                0,
                "use.methods must be strictly ascending",
            );
        }
    }
}

#[test]
fn flat_api_key_host_is_nonempty_without_wildcards() {
    for flow in [false, true] {
        for version in [1, 2] {
            let mut s = api_key_statement(flow, version);
            for host in ["", "*.x.test"] {
                s["use"]["host"] = json!(host);
                refuses_with(
                    flow,
                    body(flow, &s),
                    0,
                    "use.host must be nonempty and contain no wildcard",
                );
            }
        }
    }
}

#[test]
fn flat_api_key_path_prefix_is_empty_or_absolute() {
    for flow in [false, true] {
        for version in [1, 2] {
            let mut s = api_key_statement(flow, version);
            s["use"]["path_prefix"] = json!("relative");
            refuses_with(
                flow,
                body(flow, &s),
                0,
                "use.path_prefix must be empty or start with /",
            );
        }
    }
}

fn rules_refuse(rules: Value, problem: &str) {
    for flow in [false, true] {
        let mut s = api_key_statement(flow, 3);
        s["use"]["rules"] = rules.clone();
        refuses_with(flow, body(flow, &s), 0, problem);
    }
}

fn rule(host: &str, path_prefix: &str) -> Value {
    json!({"host": host, "path_prefix": path_prefix})
}

#[test]
fn v3_api_key_has_one_to_eight_rules() {
    let problem = "use.rules must contain 1–8 rules";
    rules_refuse(json!([]), problem);
    rules_refuse(
        json!((1..=9)
            .map(|i| rule(&format!("x{i}.test"), ""))
            .collect::<Vec<_>>()),
        problem,
    );
}

#[test]
fn v3_api_key_rule_has_exactly_host_and_path_prefix_strings() {
    for invalid in [
        json!({"host":"*.x.test"}),
        json!({"host":"*.x.test","path_prefix":"","extra":true}),
    ] {
        rules_refuse(
            json!([invalid]),
            "each use.rules entry must contain exactly host and path_prefix strings",
        );
    }
}

#[test]
fn v3_api_key_host_is_nonempty_with_only_a_leading_wildcard_allowed() {
    for host in ["", "*.", "*.x*.test", "x.*.test"] {
        rules_refuse(json!([rule(host, ""), rule("z.test", "")]), "use.rules host must be nonempty and may use * only as one leading *. with a nonempty suffix");
    }
}

#[test]
fn v3_api_key_single_rule_requires_a_wildcard() {
    rules_refuse(
        json!([rule("x.test", "")]),
        "use.rules requires a wildcard host when there is only one rule",
    );
}

#[test]
fn v3_api_key_path_prefix_is_empty_or_absolute() {
    rules_refuse(
        json!([rule("*.x.test", "relative")]),
        "use.rules path_prefix must be empty or start with /",
    );
}

#[test]
fn v3_api_key_rules_are_strictly_sorted_by_host_then_prefix() {
    for (first, second) in [
        (rule("y.test", "/a"), rule("x.test", "/z")),
        (rule("x.test", "/z"), rule("x.test", "/a")),
        (rule("x.test", "/a"), rule("x.test", "/a")),
    ] {
        rules_refuse(
            json!([first, second]),
            "use.rules must be strictly ascending by host, then path_prefix",
        );
    }
}

fn origins_refuse(origins: &[&str], problem: &str) {
    for flow in [false, true] {
        let mut s = statement(flow, "local-two-origins");
        s["use"]["origins"] = json!(origins);
        refuses_with(flow, body(flow, &s), 0, problem);
    }
}

#[test]
fn v3_login_requires_two_to_four_origins() {
    let problem = "use.origins must contain 2–4 origins";
    origins_refuse(&["https://a.test"], problem);
    origins_refuse(
        &[
            "https://a.test",
            "https://b.test",
            "https://c.test",
            "https://d.test",
            "https://e.test",
        ],
        problem,
    );
}

#[test]
fn v3_login_origin_must_start_with_https_and_have_a_nonempty_wildcard_free_host() {
    for origin in ["http://a.test", "https://", "https://*.a.test"] {
        origins_refuse(&[origin, "https://z.test"], "use.origins entries must start with https://, have a nonempty host and contain no wildcard");
    }
}

#[test]
fn v3_login_origins_must_be_strictly_ascending_as_written() {
    origins_refuse(
        &["https://b.test", "https://a.test"],
        "use.origins must be strictly ascending as written",
    );
}

#[test]
fn v3_login_origins_must_be_distinct_without_port_443() {
    // The colliding entries need not be adjacent in signed order.
    origins_refuse(
        &[
            "https://a.test",
            "https://a.test.example",
            "https://a.test:443",
        ],
        "use.origins must be distinct after removing a trailing :443",
    );
}

fn two_statements(flow: bool, first: &Value, second: &Value) -> Value {
    let mut second = second.clone();
    second["request_id"] = json!("01HF7YAT000000000000000002");
    let mut request = body(flow, first);
    items(flow, &mut request)
        .as_array_mut()
        .unwrap()
        .push(json!({
            "request_id":second["request_id"], "statement_b64url":encoded(&second)
        }));
    request
}

#[test]
fn card_statements_must_match_serving_member() {
    for flow in [false, true] {
        let first = api_key_statement(flow, 3);
        let mut second = first.clone();
        second["member_id"] = json!("fed-computer:33333333333333333333333333333333");
        refuses_with(
            flow,
            two_statements(flow, &first, &second),
            1,
            "member_id must match the first statement",
        );
    }
}

#[test]
fn card_statements_must_match_asking_member_including_absence() {
    for flow in [false, true] {
        let first = api_key_statement(flow, 2);
        let mut another_asker = first.clone();
        another_asker["asking_member_id"] = json!("fed-computer:33333333333333333333333333333333");
        let mut asked_here = first.clone();
        asked_here["v"] = json!(format!(
            "ck-secret-{}-statement/v1",
            if flow { "flow" } else { "use" }
        ));
        asked_here
            .as_object_mut()
            .unwrap()
            .remove("asking_member_id");
        for second in [another_asker, asked_here] {
            refuses_with(
                flow,
                two_statements(flow, &first, &second),
                1,
                "asking_member_id must match the first statement, including when absent",
            );
        }
    }
}

#[tokio::test]
async fn stored_card_can_be_declined_even_if_its_scope_no_longer_passes_filing() {
    for flow in [false, true] {
        let f = fixture().await;
        let sender = if flow {
            filer("prefrontal-core", None)
        } else {
            filer("aft", Some("agent-A"))
        };
        let mut s = api_key_statement(flow, 1);
        let valid = body(flow, &s);
        s["use"]["location"] = json!("header:Host");
        let invalid = body(flow, &s);
        let before = f.rows();
        let error = f
            .call(&sender, "consent.request", invalid)
            .await
            .unwrap_err();
        assert_eq!(
            error["code"],
            if flow {
                "elicitation_invalid_flow_secrets"
            } else {
                "elicitation_invalid_request"
            }
        );
        assert_eq!(f.rows(), before);

        // Simulate the exact same pending record as filed before the new check.
        let id = f.request(&sender, valid).await;
        let mut record = serde_json::to_value(f.stored_record(&id)).unwrap();
        items(flow, &mut record["request"])[0]["statement_b64url"] = encoded(&s);
        f.sql()
            .execute(
                "UPDATE elicitation_records SET record_json = ?1 WHERE elicitation_id = ?2",
                rusqlite::params![record.to_string(), id],
            )
            .unwrap();

        let reply = f
            .ok(&phone(DEVICE), "consent.answer", flow_secrets::decline(&id))
            .await;
        assert_eq!(reply["ok"], true);
        let answer = f.stored_record(&id).secret_use_answer.unwrap();
        assert_eq!(answer.decision, "decline");
        assert_eq!(answer.items.len(), 1);
        assert_eq!(answer.items[0].decision, "decline");
        assert!(answer.envelope_sha256.is_none());
    }
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
