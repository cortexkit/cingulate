#[path = "support/consent_phone_settled.rs"]
mod consent_phone_settled;
#[allow(dead_code)]
#[path = "support/phone.rs"]
mod phone;
mod support;

use std::{collections::BTreeSet, fs};

use serde_json::Value;
use sha2::{Digest, Sha256};
use support::corpus;

const FILES: [&str; 11] = [
    "README.md",
    "SHA256SUMS",
    "answer-acknowledged-conflict.json",
    "answered-after-ack-executed-phone-get.json",
    "answered-after-ack-not-run-phone-get.json",
    "answered-after-ack-pending-execution-phone-get.json",
    "expired-after-ack-phone-get.json",
    "pruned-after-ack-phone-get.json",
    "request-reply.json",
    "request.json",
    "withdrawn-after-ack-phone-get.json",
];

fn generated_json(files: &std::collections::BTreeMap<String, Vec<u8>>, name: &str) -> Value {
    serde_json::from_slice(files.get(name).unwrap()).unwrap()
}

#[tokio::test]
async fn consent_phone_settled_vectors_regenerate_byte_for_byte() {
    let generated = consent_phone_settled::generate().await;
    let expected: BTreeSet<_> = FILES
        .iter()
        .filter(|name| **name != "README.md" && **name != "SHA256SUMS")
        .map(|name| name.to_string())
        .collect();
    assert_eq!(generated.keys().cloned().collect::<BTreeSet<_>>(), expected);
    for (name, bytes) in generated {
        assert_eq!(
            fs::read(corpus(consent_phone_settled::CORPUS).join(&name)).unwrap(),
            bytes,
            "served vector differs: {name}"
        );
    }
}

#[tokio::test]
async fn answered_after_ack_phone_get_serves_answer_and_options() {
    let generated = consent_phone_settled::generate().await;
    let envelope = generated_json(
        &generated,
        "answered-after-ack-pending-execution-phone-get.json",
    );
    let card = &envelope["result"];
    assert_eq!(card["state"], "answered");
    assert_eq!(card["answeredChoiceId"], "allow");
    assert_eq!(card["options"][0]["id"], "allow");
    assert_eq!(card["options"][0]["label"], "Approve");
    assert_eq!(card["options"][1]["id"], "deny");
    assert_eq!(card["options"][1]["label"], "Decline");
    assert!(card.get("execution").is_none());

    let conflict = generated_json(&generated, "answer-acknowledged-conflict.json");
    assert_eq!(conflict["result"]["ok"], false);
    assert_eq!(conflict["result"]["code"], "conflict");
    assert_eq!(conflict["result"]["elicitation"], *card);
}

#[tokio::test]
async fn acknowledged_phone_get_keeps_execution_and_terminal_projections() {
    let generated = consent_phone_settled::generate().await;
    let executed = generated_json(&generated, "answered-after-ack-executed-phone-get.json");
    assert_eq!(executed["result"]["execution"], "executed");
    assert_eq!(executed["result"]["answeredChoiceId"], "allow");

    let not_run = generated_json(&generated, "answered-after-ack-not-run-phone-get.json");
    assert_eq!(not_run["result"]["execution"], "approved_not_executed");
    assert_eq!(not_run["result"]["answeredChoiceId"], "allow");

    for (name, state) in [
        ("expired-after-ack-phone-get.json", "expired"),
        ("withdrawn-after-ack-phone-get.json", "withdrawn"),
    ] {
        let card = generated_json(&generated, name);
        assert_eq!(card["result"]["state"], state);
        assert!(card["result"].get("answeredChoiceId").is_none());
    }

    let pruned = generated_json(&generated, "pruned-after-ack-phone-get.json");
    assert_eq!(pruned["code"], "elicitation_not_found");
}

#[test]
fn consent_phone_settled_sha256sums_cover_every_file() {
    let root = corpus(consent_phone_settled::CORPUS);
    let sums = fs::read(root.join("SHA256SUMS")).unwrap();
    let mut seen = BTreeSet::new();
    for line in std::str::from_utf8(&sums).unwrap().lines() {
        let (hash, name) = line.split_once("  ").unwrap();
        assert!(seen.insert(name.to_owned()), "duplicate hash entry: {name}");
        assert_eq!(
            format!("{:x}", Sha256::digest(fs::read(root.join(name)).unwrap())),
            hash,
            "SHA256SUMS: {name}"
        );
    }
    assert_eq!(
        seen,
        FILES
            .iter()
            .filter(|name| **name != "SHA256SUMS")
            .map(|name| name.to_string())
            .collect()
    );
}
