#[path = "support/card_text.rs"]
mod card_text;
#[path = "support/phone.rs"]
#[allow(dead_code)]
mod phone;
mod support;

use std::{collections::BTreeSet, fs};

use serde_json::Value;
use sha2::{Digest, Sha256};
use support::corpus;

const FILES: [&str; 17] = [
    "dry-run-text-on-consent-reply.json",
    "dry-run-text-on-consent-request.json",
    "flow-install-answer.json",
    "flow-install-list-pending.json",
    "flow-install-phone-get.json",
    "flow-install-request-reply.json",
    "flow-install-request.json",
    "flow-install-requester-get.json",
    "package-answer.json",
    "package-list-pending.json",
    "package-phone-get.json",
    "package-request-reply.json",
    "package-request.json",
    "package-requester-get.json",
    "would-over-cap-reply.json",
    "would-over-cap-request.json",
    "README.md",
];

fn json_file(name: &str) -> Value {
    serde_json::from_slice(&fs::read(corpus(card_text::CORPUS).join(name)).unwrap()).unwrap()
}

#[tokio::test]
async fn consent_card_text_vectors_regenerate_byte_for_byte() {
    let generated = card_text::generate().await;
    let expected: BTreeSet<_> = FILES
        .iter()
        .filter(|name| **name != "README.md")
        .map(|name| name.to_string())
        .collect();
    assert_eq!(generated.keys().cloned().collect::<BTreeSet<_>>(), expected);
    for (name, bytes) in generated {
        assert_eq!(
            fs::read(corpus(card_text::CORPUS).join(&name)).unwrap(),
            bytes,
            "served vector differs: {name}"
        );
    }
}

#[test]
fn text_fields_survive_requester_and_phone_readbacks_with_native_casing() {
    for prefix in ["flow-install", "package"] {
        let phone_list =
            json_file(&format!("{prefix}-list-pending.json"))["result"]["records"][0].clone();
        let phone_get = json_file(&format!("{prefix}-phone-get.json"))["result"].clone();
        let requester_get = json_file(&format!("{prefix}-requester-get.json"))["result"].clone();
        let answer = json_file(&format!("{prefix}-answer.json"))["result"]["elicitation"].clone();
        let source = json_file(&format!("{prefix}-request.json"))["params"]["source"].clone();
        let dry_run =
            json_file(&format!("{prefix}-request.json"))["params"]["dry_run_text"].clone();

        for card in [&phone_list, &phone_get, &answer] {
            assert_eq!(card["source"], source, "{prefix} source");
            assert_eq!(card["dryRunText"], dry_run, "{prefix} dryRunText");
            assert!(card.get("dry_run_text").is_none());
        }
        assert_eq!(requester_get["source"], source, "{prefix} requester source");
        assert_eq!(
            requester_get["dry_run_text"], dry_run,
            "{prefix} requester dry_run_text"
        );
        assert!(requester_get.get("dryRunText").is_none());
        let labels: Vec<_> = phone_get["facts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|fact| fact["label"].as_str().unwrap())
            .collect();
        assert_eq!(labels, ["Runs", "Can wake", "Token limit"]);
        for fact in phone_get["facts"].as_array().unwrap() {
            assert!(fact["value"].is_string());
        }
    }

    assert_eq!(
        json_file("dry-run-text-on-consent-reply.json")["code"],
        "elicitation_invalid_request"
    );
    assert!(json_file("dry-run-text-on-consent-reply.json")["message"]
        .as_str()
        .unwrap()
        .contains("dry_run_text"));
    assert_eq!(
        json_file("would-over-cap-reply.json")["code"],
        "elicitation_invalid_request"
    );
    assert!(json_file("would-over-cap-reply.json")["message"]
        .as_str()
        .unwrap()
        .contains("would"));
}

#[test]
fn consent_card_text_sha256sums_cover_every_file() {
    let root = corpus(card_text::CORPUS);
    let sums = fs::read(root.join("SHA256SUMS")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&sums)),
        "ac74238a79262de6b2641386e290e01d05bff7329555bb55c6e60f8c4655c43d"
    );
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
    assert_eq!(seen, FILES.iter().map(|name| name.to_string()).collect());
    seen.insert("SHA256SUMS".into());
    let actual = fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, seen, "unexpected corpus file");
}
