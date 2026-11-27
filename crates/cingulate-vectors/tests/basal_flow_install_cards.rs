#[path = "support/basal_flow_install_cards.rs"]
mod basal_flow_install_cards;
#[allow(dead_code)]
#[path = "support/flow_card_structure.rs"]
mod flow_card_structure;
#[allow(dead_code)]
#[path = "support/phone.rs"]
mod phone;
mod support;
#[allow(dead_code)]
#[path = "support/watch_pending.rs"]
mod watch_pending;

use std::{collections::BTreeSet, fs};

use serde_json::Value;
use sha2::{Digest, Sha256};

use basal_flow_install_cards::{CARDS, CORPUS};
use support::corpus;

#[tokio::test]
async fn basal_flow_install_cards_served_vectors_regenerate_byte_for_byte() {
    let root = corpus(CORPUS);
    let mut files = basal_flow_install_cards::generate().await;
    let mut actual: BTreeSet<_> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(actual.len(), 25);
    if !ck_cingulate::DEVELOPMENT_LOCAL_OPERATOR {
        // Only the plain local list needs development admission. The manifest
        // pin still verifies every file, including that page, in this build.
        assert!(actual.remove("desktop-list-pending.json"));
        assert!(actual.remove("SHA256SUMS"));
        files.remove("SHA256SUMS");
    }
    assert_eq!(
        actual,
        files.keys().cloned().collect(),
        "corpus file coverage"
    );
    for (name, bytes) in files {
        assert_eq!(fs::read(root.join(&name)).unwrap(), bytes, "{name} differs");
    }
}

#[test]
fn basal_flow_install_cards_served_manifest_is_pinned_and_covers_exact_bytes() {
    let root = corpus(CORPUS);
    let manifest = fs::read(root.join("SHA256SUMS")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&manifest)),
        "2753305ec614b7ed8c0ea08022bdbdc5bddde26cf0e683eb706d38ed5a97d8c5"
    );
    let mut listed = BTreeSet::new();
    for line in std::str::from_utf8(&manifest).unwrap().lines() {
        let (digest, name) = line.split_once("  ").unwrap();
        assert!(!name.contains('/') && name != "SHA256SUMS");
        assert!(listed.insert(name.to_owned()), "duplicate {name}");
        assert_eq!(
            format!("{:x}", Sha256::digest(fs::read(root.join(name)).unwrap())),
            digest,
            "{name}"
        );
    }
    let actual: BTreeSet<_> = fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name != "SHA256SUMS")
        .collect();
    assert_eq!(actual, listed);
}

#[test]
fn basal_flow_install_cards_inputs_match_filing_samples_manifest() {
    let root = corpus(CORPUS);
    // Reconstruct the filing samples' manifest with the original filenames.
    let manifest: String = CARDS
        .into_iter()
        .map(|card| {
            let bytes = fs::read(root.join(format!("{card}-request.json"))).unwrap();
            format!("{:x}  {card}.json\n", Sha256::digest(bytes))
        })
        .collect();
    assert_eq!(
        format!("{:x}", Sha256::digest(manifest.as_bytes())),
        "58fd4d3c51c9a66b04f54fafd21fd244132406619b8f4a5b1f0b3d9c4c2c0f28"
    );
}

#[test]
fn basal_flow_install_cards_serve_full_explanation_and_statement_deadline() {
    let root = corpus(CORPUS);
    let read =
        |name| -> Value { serde_json::from_slice(&fs::read(root.join(name)).unwrap()).unwrap() };
    let filing = read("max-explanation-request.json");
    let phone = read("max-explanation-phone-get.json");
    let served = phone["result"]["explanation"]["text"].as_str().unwrap();
    assert_eq!(served, filing["params"]["explanation"]["text"]);
    assert_eq!(served.chars().count(), 1_500);
    assert_eq!(served.len(), 6_000);

    let secret_get = read("flow-secrets-phone-get.json");
    let deadline = 1_777_593_660_000_i64;
    assert_eq!(secret_get["result"]["expiresAtMs"], deadline);
    for name in ["phone-list-pending.json", "desktop-list-pending.json"] {
        let list = read(name);
        let secret_row = list["result"]["records"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["elicitationId"] == secret_get["result"]["elicitationId"])
            .unwrap();
        assert_eq!(secret_row["expiresAtMs"], deadline, "{name}");
        let filing = read("flow-secrets-request.json");
        assert_eq!(
            secret_row["flowInstall"]["secrets"], filing["params"]["flow_install"]["secrets"],
            "{name}"
        );
    }
}
