#[path = "support/secret_use.rs"]
mod secret_use;
mod support;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use cingulate_decode::ConsentRequest;
use secret_use::{decoded, encoded, filer, filer_with_scope, fixture, phone, DEVICE};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use subc_protocol::scope::{ScopeAttributes, ScopeKind};

const VECTORS: &[u8] = include_bytes!("../../../test-vectors/secret-local-use-v1/vectors.json");
const README: &[u8] = include_bytes!("../../../test-vectors/secret-local-use-v1/README.md");
const SUMS: &str = include_str!("../../../test-vectors/secret-local-use-v1/SHA256SUMS");
const VECTORS_V1_1: &[u8] =
    include_bytes!("../../../test-vectors/secret-local-use-v1.1/vectors.json");
const README_V1_1: &[u8] = include_bytes!("../../../test-vectors/secret-local-use-v1.1/README.md");
const SUMS_V1_1: &str = include_str!("../../../test-vectors/secret-local-use-v1.1/SHA256SUMS");

fn vectors() -> Value {
    serde_json::from_slice(VECTORS_V1_1).unwrap()
}

fn v1_vectors() -> Value {
    serde_json::from_slice(VECTORS).unwrap()
}

fn case(name: &str) -> Value {
    tagged_case(&vectors(), name)
}

fn tagged_case(pack: &Value, name: &str) -> Value {
    pack["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["case"] == name)
        .unwrap()
        .clone()
}

fn body(statement: &Value) -> Value {
    json!({"kind":"secret_use", "secret_use":{"items":[{
        "request_id":statement["request_id"], "statement_b64url":encoded(statement)
    }]}})
}

fn refuses(statement: &Value) {
    let error = ConsentRequest::decode(body(statement)).unwrap_err();
    assert_eq!(error.code, "elicitation_invalid_request", "{statement}");
}

fn check_record(record: &Value) {
    let bytes = URL_SAFE_NO_PAD
        .decode(record["b64url"].as_str().unwrap())
        .unwrap();
    assert_eq!(format!("{:x}", Sha256::digest(&bytes)), record["sha256"]);
    assert_eq!(decoded(&record["b64url"]), record["json"]);
    assert_eq!(serde_jcs::to_vec(&record["json"]).unwrap(), bytes);
}

#[test]
fn local_use_tagged_files_have_pinned_checksums() {
    let expected = [
        (
            VECTORS,
            "vectors.json",
            "e3291166cf41e16549587d0ff7bfae4654f79f830c00c818f1b7130e7c994a73",
        ),
        (
            README,
            "README.md",
            "3752ebf3e266f8e9ab290f8895d4713ac2a12bba5334bad51e3e45c8fdfcfd8a",
        ),
    ];
    assert_eq!(SUMS.lines().count(), expected.len());
    for (bytes, name, digest) in expected {
        assert_eq!(format!("{:x}", Sha256::digest(bytes)), digest);
        assert!(SUMS.lines().any(|line| line == format!("{digest}  {name}")));
    }
    assert_eq!(vectors()["cases"].as_array().unwrap().len(), 4);
    assert_eq!(vectors()["refusals"].as_array().unwrap().len(), 11);
}

#[test]
fn local_use_v1_1_tagged_files_have_pinned_checksums() {
    for (bytes, digest) in [
        (
            VECTORS_V1_1,
            "daa5f74bcf8e5bcebe1ec6dd0137a4214dca19fceafe152438beb11f9bac965a",
        ),
        (
            README_V1_1,
            "c242ec2067679b703363aa6913707c9e2f7bc8085157f665c23afeee5316ed46",
        ),
        (
            SUMS_V1_1.as_bytes(),
            "257d5e7d6b169a4d84ce0b555e5e74fe2b5ef727e2f80d609bbbc7b0993ed374",
        ),
    ] {
        assert_eq!(format!("{:x}", Sha256::digest(bytes)), digest);
    }
    assert_eq!(
        SUMS_V1_1,
        "daa5f74bcf8e5bcebe1ec6dd0137a4214dca19fceafe152438beb11f9bac965a  vectors.json\n"
    );
}

async fn sample_files_and_binds(name: &str) {
    let sample = case(name);
    let statement = &sample["statement"]["json"];
    for record in [
        &sample["statement"],
        &sample["approval_required"],
        &sample["approval"]["envelope"],
        &sample["approval"]["reply"],
        &sample["request"],
        &sample["reply"],
    ] {
        check_record(record);
    }
    let f = fixture().await;
    let sender = filer("aft", statement["agent_id"].as_str());
    let mut request = body(statement);
    // File the producer's exact canonical bytes, not a reconstructed statement.
    request["secret_use"]["items"][0]["statement_b64url"] = sample["statement"]["b64url"].clone();
    assert_eq!(
        sample["approval_required"]["json"]["statement_b64url"],
        sample["statement"]["b64url"]
    );
    let id = f.request(&sender, request.clone()).await;
    assert_eq!(f.stored_record(&id).expires_at_ms, Some(1_700_000_600_000));
    let card = f
        .ok(
            &phone(DEVICE),
            "consent.get",
            json!({
                "elicitationId":id, "clientFeatures":["secret-use-approval"]
            }),
        )
        .await;
    assert_eq!(card["secretUse"], request["secret_use"]);
    let envelope_b64url = &sample["approval"]["envelope"]["b64url"];
    let answer = json!({"elicitationId":id, "choiceId":"allow",
        "clientFeatures":["secret-use-approval"],
        "envelope_b64url":envelope_b64url});
    assert_eq!(
        f.ok(&phone(DEVICE), "consent.answer", answer.clone()).await["ok"],
        true
    );
    let stored = f.stored_record(&id);
    assert_eq!(stored.answered_device.as_deref(), Some(DEVICE));
    let binding = stored.secret_use_answer.unwrap();
    assert_eq!(binding.decision, "allow");
    assert_eq!(
        binding.duration.as_deref(),
        sample["approval"]["envelope"]["json"]["duration"].as_str()
    );
    assert_eq!(
        binding.envelope_sha256.as_deref(),
        sample["approval"]["envelope"]["sha256"].as_str()
    );
    assert_eq!(binding.items.len(), 1);
    assert_eq!(binding.items[0].request_id, statement["request_id"]);
    assert_eq!(binding.items[0].decision, "allow");
    assert_eq!(
        f.ok(&phone(DEVICE), "consent.answer", answer).await["alreadyAnswered"],
        true
    );
}

macro_rules! approval_case {
    ($test:ident, $case:literal) => {
        #[tokio::test]
        async fn $test() {
            sample_files_and_binds($case).await;
        }
    };
}

approval_case!(sudo_password_local_sample, "sudo_password-local");
approval_case!(sudo_password_forwarded_sample, "sudo_password-forwarded");
approval_case!(ssh_sign_local_sample, "ssh_sign-local");
approval_case!(ssh_sign_forwarded_sample, "ssh_sign-forwarded");

async fn known_sample_fingerprint_defect(name: &str) {
    let sample = tagged_case(&v1_vectors(), name);
    let statement = &sample["statement"]["json"];
    let f = fixture().await;
    let id = f
        .request(
            &filer("aft", statement["agent_id"].as_str()),
            body(statement),
        )
        .await;
    // V1's 64-hex phone fingerprint defect, fixed in v1.1, must stay refused.
    let device = sample["approval"]["envelope"]["json"]["device_fingerprint"]
        .as_str()
        .unwrap();
    assert_eq!(device.len(), 64);
    let before = f.rows();
    assert!(f
        .call(
            &phone(device),
            "consent.answer",
            json!({
                "elicitationId":id,"choiceId":"allow","clientFeatures":["secret-use-approval"],
                "envelope_b64url":sample["approval"]["envelope"]["b64url"]
            })
        )
        .await
        .is_err());
    assert_eq!(f.rows(), before);
}

macro_rules! fingerprint_defect_case {
    ($test:ident, $case:literal) => {
        #[tokio::test]
        async fn $test() {
            known_sample_fingerprint_defect($case).await;
        }
    };
}

fingerprint_defect_case!(
    known_v1_vault_sample_defect_sudo_local_fingerprint,
    "sudo_password-local"
);
fingerprint_defect_case!(
    known_v1_vault_sample_defect_ssh_forwarded_fingerprint,
    "ssh_sign-forwarded"
);

#[test]
fn known_v1_vault_sample_defect_sudo_forwarded_omits_agent_id() {
    // V1's omitted agent_id defect, fixed in v1.1, must stay refused.
    let sample = tagged_case(&v1_vectors(), "sudo_password-forwarded");
    assert!(sample["statement"]["json"].get("agent_id").is_none());
    refuses(&sample["statement"]["json"]);
}

#[test]
fn known_v1_vault_sample_defect_ssh_local_omits_agent_id() {
    // V1's omitted agent_id defect, fixed in v1.1, must stay refused.
    let sample = tagged_case(&v1_vectors(), "ssh_sign-local");
    assert!(sample["statement"]["json"].get("agent_id").is_none());
    refuses(&sample["statement"]["json"]);
}

#[tokio::test]
async fn sudo_hour_refused_sample_leaves_card_open() {
    let sample = case("sudo_password-local");
    let refusal = &vectors()["sudo_hour_refused"];
    check_record(&refusal["envelope"]);
    check_record(&refusal["reply"]);
    assert_eq!(
        refusal["reply"]["json"]["results"][0]["code"],
        "approval_invalid"
    );
    let f = fixture().await;
    let id = f
        .request(
            &filer("aft", Some("agent-A")),
            body(&sample["statement"]["json"]),
        )
        .await;
    let before = f.rows();
    let error = f
        .call(
            &phone(DEVICE),
            "consent.answer",
            json!({
                "elicitationId":id,"choiceId":"allow","clientFeatures":["secret-use-approval"],
            "envelope_b64url":refusal["envelope"]["b64url"]
            }),
        )
        .await
        .unwrap_err();
    assert_eq!(error["code"], "elicitation_invalid_request");
    assert_eq!(f.rows(), before);
}

#[test]
fn local_use_exact_members_and_nullability_are_closed() {
    for name in ["sudo_password-local", "ssh_sign-forwarded"] {
        let statement = case(name)["statement"]["json"].clone();
        ConsentRequest::decode(body(&statement)).unwrap();
        for field in statement.as_object().unwrap().keys() {
            let mut changed = statement.clone();
            changed[field] = Value::Null;
            if field == "agent_id" {
                ConsentRequest::decode(body(&changed)).unwrap();
            } else {
                refuses(&changed);
            }
            if field != "asking_member_id" {
                changed.as_object_mut().unwrap().remove(field);
                refuses(&changed);
            }
        }
        for field in ["extra", "program_extra"] {
            let mut changed = statement.clone();
            changed[field] = json!("unexpected");
            refuses(&changed);
        }
        for field in [
            "scope",
            "target",
            if name.starts_with("sudo") {
                "secret"
            } else {
                "key"
            },
        ] {
            let object = statement[field].as_object().unwrap();
            for key in object.keys() {
                let mut changed = statement.clone();
                changed[field][key] = Value::Null;
                refuses(&changed);
                changed[field].as_object_mut().unwrap().remove(key);
                refuses(&changed);
            }
            let mut changed = statement.clone();
            changed[field]["extra"] = json!("unexpected");
            refuses(&changed);
        }
    }
    for (name, fields) in [
        ("sudo_password-local", vec!["key"]),
        ("ssh_sign-local", vec!["secret", "expects", "undoable"]),
    ] {
        for field in fields {
            for value in [Value::Null, json!("unexpected")] {
                let mut statement = case(name)["statement"]["json"].clone();
                statement[field] = value;
                refuses(&statement);
            }
        }
    }
}

#[test]
fn local_use_program_use_and_target_must_agree() {
    for name in ["sudo_password-local", "ssh_sign-local"] {
        let original = case(name)["statement"]["json"].clone();
        ConsentRequest::decode(body(&original)).unwrap();
        for (field, value) in [
            ("v", json!("ck-secret-local-use-statement/v2")),
            ("program", json!("git")),
            ("use", json!("git_token")),
            (
                "program",
                json!(if name.starts_with("sudo") {
                    "ssh"
                } else {
                    "sudo"
                }),
            ),
            (
                "use",
                json!(if name.starts_with("sudo") {
                    "ssh_sign"
                } else {
                    "sudo_password"
                }),
            ),
            (
                "target",
                if name.starts_with("sudo") {
                    json!({"user":"root", "host":"host", "port":22})
                } else {
                    json!({"user":"alice"})
                },
            ),
        ] {
            let mut statement = original.clone();
            statement[field] = value;
            refuses(&statement);
        }
    }
    for kind in ["agent_api_key", "site_login", "unknown"] {
        let mut statement = case("sudo_password-local")["statement"]["json"].clone();
        statement["secret"]["kind"] = json!(kind);
        refuses(&statement);
    }
    let mut statement = case("sudo_password-local")["statement"]["json"].clone();
    statement["undoable"] = json!("maybe");
    refuses(&statement);
}

#[test]
fn local_use_durations_are_closed_per_use() {
    for durations in [
        json!([]),
        json!(["hour"]),
        json!(["once", "hour"]),
        json!(["once", "once"]),
    ] {
        let mut statement = case("sudo_password-local")["statement"]["json"].clone();
        statement["durations"] = durations;
        refuses(&statement);
    }
    for durations in [
        json!([]),
        json!(["permanent"]),
        json!(["once", "permanent"]),
        json!(["hour", "hour"]),
    ] {
        let mut statement = case("ssh_sign-local")["statement"]["json"].clone();
        statement["durations"] = durations;
        refuses(&statement);
    }
    for durations in [
        json!(["once"]),
        json!(["hour"]),
        json!(["session"]),
        json!(["session", "once"]),
    ] {
        let mut statement = case("ssh_sign-local")["statement"]["json"].clone();
        statement["durations"] = durations;
        ConsentRequest::decode(body(&statement)).unwrap();
    }
}

#[test]
fn local_use_statement_bytes_must_be_canonical() {
    let statement = case("sudo_password-local")["statement"]["json"].clone();
    for bytes in [
        serde_json::to_vec_pretty(&statement).unwrap(),
        serde_jcs::to_string(&statement)
            .unwrap()
            .replacen("{", "{\"program\":\"sudo\",", 1)
            .into_bytes(),
    ] {
        let mut request = body(&statement);
        request["secret_use"]["items"][0]["statement_b64url"] =
            json!(URL_SAFE_NO_PAD.encode(bytes));
        assert!(ConsentRequest::decode(request).is_err());
    }
    let mut statement = statement;
    statement["request_expires_at_ms"] = json!(9_007_199_254_740_992_u64);
    refuses(&statement);
    statement["request_expires_at_ms"] = json!(1.5);
    refuses(&statement);
}

#[test]
fn local_use_cards_keep_item_identity_and_batch_rules() {
    let statement = case("sudo_password-local")["statement"]["json"].clone();
    let request = body(&statement);
    for count in [0, 2, 17] {
        let mut changed = request.clone();
        changed["secret_use"]["items"] =
            json!(vec![request["secret_use"]["items"][0].clone(); count]);
        assert!(ConsentRequest::decode(changed).is_err());
    }
    for field in ["agent_id", "member_id", "asking_member_id"] {
        let mut second = statement.clone();
        second["request_id"] = json!("01HF7YAT000000000000000104");
        second[field] = json!(if field == "agent_id" {
            "other-agent"
        } else {
            "fed-computer:33333333333333333333333333333333"
        });
        let mut changed = request.clone();
        changed["secret_use"]["items"]
            .as_array_mut()
            .unwrap()
            .push(body(&second)["secret_use"]["items"][0].clone());
        assert!(ConsentRequest::decode(changed).is_err(), "{field}");
    }
    let mut no_agent = statement.clone();
    no_agent["request_id"] = json!("01HF7YAT000000000000000104");
    no_agent["agent_id"] = Value::Null;
    let mut changed = request.clone();
    changed["secret_use"]["items"]
        .as_array_mut()
        .unwrap()
        .push(body(&no_agent)["secret_use"]["items"][0].clone());
    assert!(ConsentRequest::decode(changed).is_err());
    let mut changed = request.clone();
    changed["secret_use"]["items"][0]["request_id"] = json!("01HF7YAT000000000000000104");
    assert!(ConsentRequest::decode(changed).is_err());
    for id in [
        "01hf7yat000000000000000100",
        "81HF7YAT000000000000000100",
        "short",
    ] {
        let mut changed = statement.clone();
        changed["request_id"] = json!(id);
        refuses(&changed);
    }
    for asking in [json!("other"), statement["member_id"].clone(), Value::Null] {
        let mut changed = statement.clone();
        changed["asking_member_id"] = asking;
        refuses(&changed);
    }
    let mut valid = request;
    let items = valid["secret_use"]["items"].as_array_mut().unwrap();
    for number in 1..16 {
        let mut statement = statement.clone();
        statement["request_id"] = json!(format!("01HF7YAT0000000000000001{number:02}"));
        items.push(body(&statement)["secret_use"]["items"][0].clone());
    }
    ConsentRequest::decode(valid).unwrap();
}

#[test]
fn local_and_existing_statement_families_cannot_mix() {
    let statement = case("sudo_password-local")["statement"]["json"].clone();
    let mut request = body(&statement);
    let mut legacy =
        decoded(&secret_use::body("approval")["secret_use"]["items"][0]["statement_b64url"]);
    for field in ["agent_id", "member_id"] {
        legacy[field] = statement[field].clone();
    }
    ConsentRequest::decode(body(&legacy)).unwrap();
    request["secret_use"]["items"]
        .as_array_mut()
        .unwrap()
        .push(body(&legacy)["secret_use"]["items"][0].clone());
    assert!(ConsentRequest::decode(request).is_err());
}

#[test]
fn legacy_statements_still_refuse_local_use_members() {
    let statement =
        decoded(&secret_use::body("approval")["secret_use"]["items"][0]["statement_b64url"]);
    ConsentRequest::decode(body(&statement)).unwrap();
    for field in [
        "program", "key", "target", "command", "workdir", "purpose", "expects", "undoable",
    ] {
        let mut changed = statement.clone();
        changed[field] = Value::Null;
        refuses(&changed);
    }
}

#[tokio::test]
async fn local_use_only_aft_can_file_and_head_agent_match_is_exact() {
    for name in ["sudo_password-local", "ssh_sign-local"] {
        let statement = case(name)["statement"]["json"].clone();
        let agent = statement["agent_id"].as_str();
        let f = fixture().await;
        let request = body(&statement);
        for module in ["cerebellum", "plexus", "prefrontal-core"] {
            let before = f.rows();
            assert!(
                f.call(&filer(module, agent), "consent.request", request.clone())
                    .await
                    .is_err(),
                "{module}"
            );
            assert_eq!(f.rows(), before);
        }
        for wrong in [
            Some("other-agent"),
            if agent.is_some() {
                None
            } else {
                Some("agent-A")
            },
        ] {
            let before = f.rows();
            let error = f
                .call(&filer("aft", wrong), "consent.request", request.clone())
                .await
                .unwrap_err();
            assert_eq!(
                error["message"],
                "secret use agent_id must match the stamped agent_id"
            );
            assert_eq!(f.rows(), before);
        }
        let f = fixture().await;
        f.request(&filer("aft", agent), request).await;
    }
}

#[tokio::test]
async fn local_use_worker_agent_binding_accepts_on_behalf_or_null() {
    for (on_behalf, statement_agent, accepted) in [
        (Some("agent-A"), json!("agent-A"), true),
        (Some("agent-A"), Value::Null, true),
        (None, Value::Null, true),
        (Some("agent-A"), json!("agent-B"), false),
        (None, json!("agent-A"), false),
    ] {
        let f = fixture().await;
        let sender = filer_with_scope(
            "aft",
            ScopeKind::Worker,
            ScopeAttributes::new().with_on_behalf_of_agent(on_behalf.map(str::to_owned)),
        );
        let mut statement = case("sudo_password-local")["statement"]["json"].clone();
        statement["agent_id"] = statement_agent;
        let before = f.rows();
        let result = f.call(&sender, "consent.request", body(&statement)).await;
        assert_eq!(result.is_ok(), accepted, "{on_behalf:?}: {result:?}");
        if !accepted {
            assert_eq!(f.rows(), before);
        }
    }
}

#[tokio::test]
async fn local_use_envelopes_bind_digests_device_and_offered_duration() {
    let sample = case("ssh_sign-forwarded");
    let f = fixture().await;
    let id = f
        .request(
            &filer("aft", Some("agent-A")),
            body(&sample["statement"]["json"]),
        )
        .await;
    let envelope = sample["approval"]["envelope"]["json"].clone();
    let mut invalid = Vec::new();
    for (field, value) in [
        ("v", json!("ck-secret-use-envelope/v2")),
        ("extra", json!(true)),
        ("decision", json!("decline")),
        ("duration", json!("permanent")),
        ("device_fingerprint", json!("b".repeat(32))),
        ("items", json!([])),
    ] {
        let mut changed = envelope.clone();
        changed[field] = value;
        invalid.push(encoded(&changed));
    }
    for (field, value) in [
        ("statement_sha256", json!("0".repeat(64))),
        ("request_id", json!("01HF7YAT000000000000000104")),
    ] {
        let mut changed = envelope.clone();
        changed["items"][0][field] = value;
        invalid.push(encoded(&changed));
    }
    for field in envelope.as_object().unwrap().keys() {
        let mut changed = envelope.clone();
        changed.as_object_mut().unwrap().remove(field);
        invalid.push(encoded(&changed));
    }
    invalid.push(json!(
        URL_SAFE_NO_PAD.encode(serde_json::to_vec_pretty(&envelope).unwrap())
    ));
    for envelope in invalid {
        let before = f.rows();
        assert!(f.call(&phone(DEVICE), "consent.answer", json!({
            "elicitationId":id, "choiceId":"allow", "clientFeatures":["secret-use-approval"],
            "envelope_b64url":envelope
        })).await.is_err());
        assert_eq!(f.rows(), before);
    }
    assert_eq!(
        f.ok(
            &phone(DEVICE),
            "consent.answer",
            json!({
                "elicitationId":id,"choiceId":"decline","clientFeatures":["secret-use-approval"]
            })
        )
        .await["ok"],
        true
    );
    let binding = f.stored_record(&id).secret_use_answer.unwrap();
    assert_eq!(binding.decision, "decline");
    assert_eq!(binding.envelope_sha256, None);
}

#[test]
fn local_use_text_caps_and_grammar_remain_vault_side_rules() {
    let mut sudo = case("sudo_password-local")["statement"]["json"].clone();
    sudo["command"] = json!("x".repeat(2049));
    sudo["purpose"] = json!("raw\u{200e}format");
    sudo["expects"] = json!("raw\ncontrol");
    sudo["target"]["user"] = json!("-root");
    sudo["workdir"] = json!("not absolute");
    ConsentRequest::decode(body(&sudo)).unwrap();
    let mut ssh = case("ssh_sign-local")["statement"]["json"].clone();
    ssh["target"]["host"] = json!("LOCALHOST");
    ConsentRequest::decode(body(&ssh)).unwrap();
}

// These are vault request refusals, not approval statements. Check their pinned
// request shapes without duplicating vault grammar, signing or settings policy.
fn vault_side_refusal(name: &str, operation: &str, code: &str) {
    let pack = vectors();
    let sample = pack["refusals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|sample| sample["case"] == name)
        .unwrap();
    assert_eq!(sample["operation"], operation);
    assert_eq!(sample["code"], code);
    check_record(&sample["request"]);
    if let Some(reply) = sample.get("reply") {
        check_record(reply);
    }
    let request = decoded(&sample["request"]["b64url"]);
    let fields: &[&str] = match operation {
        "secret.use" => {
            assert!(request["name"].is_string());
            for field in [
                "program", "use", "command", "workdir", "purpose", "expects", "undoable",
            ] {
                assert!(request["target"][field].is_string(), "{name}: {field}");
            }
            assert!(request["target"]["target"]["user"].is_string());
            &["name", "stamp", "target"]
        }
        "secret.ssh_sign" => {
            for field in ["key", "data", "command", "workdir", "purpose"] {
                assert!(request[field].is_string(), "{name}: {field}");
            }
            assert!(request["target"]["user"].is_string());
            assert!(request["target"]["host"].is_string());
            assert!(request["target"]["port"].as_u64().is_some());
            &[
                "key", "data", "command", "workdir", "purpose", "stamp", "target",
            ]
        }
        "secret.deposit" => {
            for field in [
                "ciphertext_b64url",
                "enc_b64url",
                "deposit_id",
                "kind",
                "member_id",
                "name",
                "program",
                "use",
            ] {
                assert!(request[field].is_string(), "{name}: {field}");
            }
            assert!(request["targets"]
                .as_array()
                .unwrap()
                .iter()
                .all(|target| target["user"].is_string()));
            &[
                "ciphertext_b64url",
                "enc_b64url",
                "deposit_id",
                "kind",
                "member_id",
                "name",
                "program",
                "use",
                "targets",
            ]
        }
        _ => unreachable!(),
    };
    assert_eq!(request.as_object().unwrap().len(), fields.len());
    assert!(fields.iter().all(|field| request.get(field).is_some()));
    if operation != "secret.deposit" {
        serde_json::from_value::<subc_protocol::scope::ScopeStamp>(request["stamp"].clone())
            .unwrap();
    }
    assert!(request.get("statement_b64url").is_none());
}

macro_rules! vault_refusal_case {
    ($test:ident, $case:literal, $operation:literal, $code:literal) => {
        #[test]
        fn $test() {
            vault_side_refusal($case, $operation, $code);
        }
    };
}

vault_refusal_case!(
    vault_side_rule_off_grammar_sudo_target,
    "off-grammar-sudo-target",
    "secret.use",
    "invalid_params"
);
vault_refusal_case!(
    vault_side_rule_cc_command,
    "cc-command",
    "secret.use",
    "invalid_params"
);
vault_refusal_case!(
    vault_side_rule_cf_purpose,
    "cf-purpose",
    "secret.use",
    "invalid_params"
);
vault_refusal_case!(
    vault_side_rule_over_cap_command,
    "over-cap-command",
    "secret.use",
    "limit_exceeded"
);
vault_refusal_case!(
    vault_side_rule_off_grammar_ssh_target,
    "off-grammar-ssh-target",
    "secret.ssh_sign",
    "invalid_params"
);
vault_refusal_case!(
    vault_side_rule_non_userauth_data,
    "non-userauth-data",
    "secret.ssh_sign",
    "invalid_params"
);
vault_refusal_case!(
    vault_side_rule_wrong_user,
    "wrong-user",
    "secret.ssh_sign",
    "invalid_params"
);
vault_refusal_case!(
    vault_side_rule_unmarked_key,
    "unmarked-key",
    "secret.ssh_sign",
    "not_found"
);
vault_refusal_case!(
    vault_side_rule_setting_off_sudo,
    "setting-off-sudo",
    "secret.use",
    "secret_local_programs_disabled"
);
vault_refusal_case!(
    vault_side_rule_setting_off_ssh,
    "setting-off-ssh",
    "secret.ssh_sign",
    "secret_local_programs_disabled"
);
vault_refusal_case!(
    vault_side_rule_setting_off_deposit_replay,
    "setting-off-deposit-replay",
    "secret.deposit",
    "secret_local_programs_disabled"
);
