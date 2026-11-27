//! v2 vault statements, written by the home vault for a request an agent on
//! another paired computer forwarded to it. Each rule has its own test so a
//! regression names the rule it broke.

#[path = "support/flow_secrets.rs"]
mod flow_secrets;
#[path = "support/forwarded_secrets.rs"]
mod forwarded_secrets;
#[path = "support/secret_use.rs"]
mod secret_use;
mod support;

use std::{collections::BTreeSet, fs};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use cingulate_store::State;
use forwarded_secrets::*;
use secret_use::{decoded, filer, fixture, phone, DEVICE, NOW};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const USE_INVALID: &str = "elicitation_invalid_request";
const FLOW_INVALID: &str = "elicitation_invalid_flow_secrets";

fn use_sender() -> ck_cingulate::Stamp {
    filer("aft", Some("agent-A"))
}

fn flow_sender() -> ck_cingulate::Stamp {
    filer("prefrontal-core", None)
}

/// Files `request` and expects the named refusal with nothing stored.
async fn refused(sender: &ck_cingulate::Stamp, request: Value, code: &str, name: &str) {
    let f = fixture().await;
    let before = f.rows();
    let error = f
        .call(sender, "consent.request", request)
        .await
        .expect_err(name);
    assert_eq!(error["code"], code, "{name}: {error}");
    assert_eq!(f.rows(), before, "{name}");
}

/// Runs the named cases from both refusal lists against their own card kind.
async fn refuse_cases(names: &[&str]) {
    for (list, sender, code) in [
        (use_refusals(), use_sender(), USE_INVALID),
        (flow_refusals(), flow_sender(), FLOW_INVALID),
    ] {
        let mut ran = 0;
        for (name, request) in list {
            if names.contains(&name) {
                refused(&sender, request, code, name).await;
                ran += 1;
            }
        }
        assert_eq!(ran, names.len());
    }
}

#[test]
fn forwarded_vault_vectors_are_vendored_byte_for_byte() {
    for (corpus, file, sha) in [
        (USE_CORPUS, USE_VAULT_FILE, USE_VAULT_SHA256),
        (FLOW_CORPUS, FLOW_VAULT_FILE, FLOW_VAULT_SHA256),
    ] {
        let bytes = fs::read(support::corpus(corpus).join(file)).unwrap();
        assert_eq!(format!("{:x}", Sha256::digest(bytes)), sha, "{file}");
    }
    // Each vault record's statement, encoding and digest agree, and the
    // statement is the v1 shape plus asking_member_id.
    for case in USE_CASES.iter().chain(&FLOW_CASES) {
        let v = vault(case);
        let bytes = URL_SAFE_NO_PAD
            .decode(v["statement_b64url"].as_str().unwrap())
            .unwrap();
        assert_eq!(bytes, serde_jcs::to_vec(&v["statement"]).unwrap(), "{case}");
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            v["statement_sha256"],
            "{case}"
        );
        assert_eq!(v["statement"]["asking_member_id"], v["asking_member_id"]);
        assert_eq!(v["statement"]["member_id"], v["member_id"]);
        assert!(v["statement"]["v"].as_str().unwrap().ends_with("/v2"));
    }
}

#[tokio::test]
async fn forwarded_secret_use_vault_vectors_file_and_bind_answers() {
    for cases in [&USE_CASES[..1], &USE_CASES[1..], &USE_CASES[..]] {
        let f = fixture().await;
        let sender = use_sender();
        let body = use_body(cases);
        let id = f.request(&sender, body.clone()).await;
        // Expiry is the earliest statement deadline, as for v1.
        assert_eq!(f.stored_record(&id).expires_at_ms, Some(1_700_000_600_000));
        // The phone sees the vault's bytes unchanged.
        let card = f
            .ok(
                &phone(DEVICE),
                "consent.get",
                json!({"elicitationId":id,"clientFeatures":["secret-use-approval"]}),
            )
            .await;
        assert_eq!(card["secretUse"], body["secret_use"]);
        // The envelope binds each statement by the vault's own digest.
        let envelope = use_envelope(&body);
        for (item, case) in envelope["items"].as_array().unwrap().iter().zip(cases) {
            assert_eq!(item["statement_sha256"], vault(case)["statement_sha256"]);
        }
        let params = use_answer(&id, &envelope);
        assert_eq!(
            f.ok(&phone(DEVICE), "consent.answer", params.clone()).await["ok"],
            true
        );
        let record = f.stored_record(&id);
        assert_eq!(record.answered_device.as_deref(), Some(DEVICE));
        let binding = record.secret_use_answer.unwrap();
        assert_eq!(binding.decision, "allow");
        assert_eq!(binding.duration.as_deref(), Some("hour"));
        assert_eq!(
            binding.envelope_sha256,
            Some(format!(
                "{:x}",
                Sha256::digest(serde_jcs::to_vec(&envelope).unwrap())
            ))
        );
        assert!(binding.items.iter().all(|i| i.decision == "allow"));
        assert_eq!(
            f.ok(&phone(DEVICE), "consent.answer", params).await["alreadyAnswered"],
            true
        );
    }
}

#[tokio::test]
async fn forwarded_secret_use_answer_checks_digest_device_and_expiry() {
    let body = use_body(&USE_CASES);
    let mut wrong_digest = use_envelope(&body);
    wrong_digest["items"][1]["statement_sha256"] = json!("0".repeat(64));
    for (stamp, envelope) in [
        (phone(DEVICE), wrong_digest),
        (
            phone("22222222222222222222222222222222"),
            use_envelope(&body),
        ),
    ] {
        let f = fixture().await;
        let id = f.request(&use_sender(), body.clone()).await;
        assert!(f
            .call(&stamp, "consent.answer", use_answer(&id, &envelope))
            .await
            .is_err());
        assert_eq!(f.stored_record(&id).state, State::Pending);
    }
    let f = fixture().await;
    let id = f.request(&use_sender(), body.clone()).await;
    f.clock.set(1_700_000_600_000);
    assert_eq!(
        f.call(
            &phone(DEVICE),
            "consent.answer",
            use_answer(&id, &use_envelope(&body))
        )
        .await
        .unwrap_err()["code"],
        "elicitation_expired"
    );
}

#[tokio::test]
async fn forwarded_secret_use_agent_must_match_filing_stamp() {
    let f = fixture().await;
    assert!(f
        .call(
            &filer("aft", Some("another-agent")),
            "consent.request",
            use_body(&USE_CASES)
        )
        .await
        .is_err());
    assert!(f.rows().iter().all(|table| table.is_empty()));
}

#[tokio::test]
async fn forwarded_flow_vault_vectors_file_and_bind_answers() {
    for cases in [&FLOW_CASES[..1], &FLOW_CASES[1..], &FLOW_CASES[..]] {
        let f = fixture().await;
        let body = flow_body(cases);
        let id = f.request(&flow_sender(), body.clone()).await;
        assert_eq!(f.stored_record(&id).expires_at_ms, Some(1_700_000_600_000));
        let envelope = flow_secrets::envelope(&body);
        for (item, case) in envelope["items"].as_array().unwrap().iter().zip(cases) {
            assert_eq!(item["statement_sha256"], vault(case)["statement_sha256"]);
        }
        let params = flow_secrets::answer(&id, &envelope);
        assert_eq!(
            f.ok(&phone(DEVICE), "consent.answer", params.clone()).await["ok"],
            true
        );
        let record = f.stored_record(&id);
        assert_eq!(record.answered_device.as_deref(), Some(DEVICE));
        let binding = record.secret_use_answer.unwrap();
        assert_eq!(binding.duration.as_deref(), Some("version"));
        assert_eq!(
            binding.envelope_sha256,
            Some(format!(
                "{:x}",
                Sha256::digest(serde_jcs::to_vec(&envelope).unwrap())
            ))
        );
        assert!(binding.items.iter().all(|i| i.decision == "allow"));
    }
    // A wrong digest or another phone leaves the card pending; expiry refuses.
    let body = flow_body(&FLOW_CASES);
    let mut wrong_digest = flow_secrets::envelope(&body);
    wrong_digest["items"][0]["statement_sha256"] = json!("0".repeat(64));
    for (stamp, envelope) in [
        (phone(DEVICE), wrong_digest),
        (
            phone("22222222222222222222222222222222"),
            flow_secrets::envelope(&body),
        ),
    ] {
        let f = fixture().await;
        let id = f.request(&flow_sender(), body.clone()).await;
        assert!(f
            .call(
                &stamp,
                "consent.answer",
                flow_secrets::answer(&id, &envelope)
            )
            .await
            .is_err());
        assert_eq!(f.stored_record(&id).state, State::Pending);
    }
    let f = fixture().await;
    let id = f.request(&flow_sender(), body).await;
    f.clock.set(1_700_000_600_000);
    assert_eq!(
        f.call(&phone(DEVICE), "consent.answer", flow_secrets::decline(&id))
            .await
            .unwrap_err()["code"],
        "elicitation_expired"
    );
}

#[tokio::test]
async fn forwarded_flow_existing_rules_still_apply() {
    let mut r = flow_body(&FLOW_CASES);
    alter(&mut r, 0, |s| s["flow"]["version"] = json!(8));
    refused(
        &flow_sender(),
        r,
        "elicitation_flow_identity_mismatch",
        "flow",
    )
    .await;
    let mut r = flow_body(&FLOW_CASES);
    alter(&mut r, 0, |s| s["durations"] = json!(["hour"]));
    refused(
        &flow_sender(),
        r,
        "elicitation_invalid_flow_durations",
        "durations",
    )
    .await;
    let mut r = flow_body(&FLOW_CASES);
    alter(&mut r, 0, |s| s["request_expires_at_ms"] = json!(NOW));
    refused(&flow_sender(), r, "elicitation_expired", "expired").await;
    let mut r = flow_body(&FLOW_CASES);
    alter(&mut r, 0, |s| s["extra"] = json!(true));
    refused(&flow_sender(), r, FLOW_INVALID, "extra").await;
}

#[tokio::test]
async fn forwarded_v1_statement_with_asking_member_is_refused() {
    refuse_cases(&["v1-with-asking-member", "v1-with-null-asking-member"]).await;
}

#[tokio::test]
async fn forwarded_v2_statement_requires_asking_member() {
    refuse_cases(&["v2-without-asking-member", "asking-member-null"]).await;
}

#[tokio::test]
async fn forwarded_asking_member_must_be_fed_computer_hex() {
    refuse_cases(&[
        "asking-member-uppercase",
        "asking-member-short",
        "asking-member-long",
        "asking-member-not-computer",
        "asking-member-bare-hex",
        "asking-member-number",
    ])
    .await;
}

#[tokio::test]
async fn forwarded_statement_version_from_other_family_is_refused() {
    refuse_cases(&["other-family-version"]).await;
}

#[tokio::test]
async fn forwarded_asking_member_cannot_be_serving_member() {
    refuse_cases(&["asking-member-is-serving-member"]).await;
}

#[tokio::test]
async fn forwarded_secret_use_card_shares_one_asking_member() {
    for (name, request) in use_refusals() {
        if name.starts_with("mixed-") {
            refused(&use_sender(), request, USE_INVALID, name).await;
        }
    }
}

#[tokio::test]
async fn forwarded_flow_card_shares_one_asking_member() {
    for (name, request) in flow_refusals() {
        if name.starts_with("mixed-") {
            refused(&flow_sender(), request, FLOW_INVALID, name).await;
        }
    }
}

#[tokio::test]
async fn forwarded_v1_statements_still_file_without_asking_member() {
    // Home requests stay v1: the same card with every statement turned back
    // into v1 (no asking_member_id) files on both card kinds.
    for (mut request, sender) in [
        (use_body(&USE_CASES), use_sender()),
        (flow_body(&FLOW_CASES), flow_sender()),
    ] {
        alter_all(&mut request, |s| {
            let v = s["v"].as_str().unwrap().replace("/v2", "/v1");
            s["v"] = json!(v);
            s.as_object_mut().unwrap().remove("asking_member_id");
        });
        let f = fixture().await;
        f.request(&sender, request).await;
    }
    // The v1 corpus statement decodes with no asking member.
    let body = secret_use::body("approval");
    let statement = decoded(&body["secret_use"]["items"][0]["statement_b64url"]);
    assert!(statement.get("asking_member_id").is_none());
    let request = cingulate_decode::ConsentRequest::decode(body).unwrap();
    let statements = request.secret_use.unwrap().statements().unwrap();
    assert_eq!(statements[0].asking_member_id, None);
    assert_eq!(statements[0].v, "ck-secret-use-statement/v1");
}

async fn corpus_matches(corpus: &str, generated: std::collections::BTreeMap<String, Vec<u8>>) {
    let root = support::corpus(corpus);
    for (name, bytes) in &generated {
        let pinned = fs::read(root.join(name)).unwrap();
        if matches!(
            name.as_str(),
            "filer-refusals.json" | "filing-refusals.json"
        ) {
            forwarded_secrets::check_published_refusals(name, &pinned, bytes);
        } else {
            assert_eq!(pinned, *bytes, "{name}");
        }
    }
    let mut listed = BTreeSet::new();
    for line in fs::read_to_string(root.join("SHA256SUMS")).unwrap().lines() {
        let (hash, name) = line.split_once("  ").unwrap();
        assert!(listed.insert(name.to_owned()), "{name}");
        assert_eq!(
            format!("{:x}", Sha256::digest(fs::read(root.join(name)).unwrap())),
            hash,
            "{name}"
        );
    }
    let on_disk: BTreeSet<String> = fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|name| name != "SHA256SUMS")
        .collect();
    assert_eq!(
        listed, on_disk,
        "{corpus}: SHA256SUMS must cover every file"
    );
    for name in generated.keys() {
        assert!(listed.contains(name), "{name}");
    }
}

#[tokio::test]
async fn forwarded_secret_use_vectors_match_published_corpus() {
    corpus_matches(USE_CORPUS, generate_use().await).await;
}

#[tokio::test]
async fn forwarded_flow_vectors_match_published_corpus() {
    corpus_matches(FLOW_CORPUS, generate_flow().await).await;
}
