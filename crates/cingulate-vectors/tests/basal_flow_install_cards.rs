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

use std::{collections::BTreeSet, fs, path::Path};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ck_cingulate::Stamp;
use serde_json::Value;
use sha2::{Digest, Sha256};

use basal_flow_install_cards::{ARCHIVED_REQUEST_SUFFIX, CARDS, FILED_AT_MS, SERVED_V1_2};
use support::{corpus, worker, Fixture};

const SERVED_V1: &str = "basal-flow-install-cards-served-v1";
const SERVED_V1_1: &str = "basal-flow-install-cards-served-v1.1";

#[tokio::test]
async fn basal_flow_install_cards_served_v1_stays_pinned_but_its_secret_filing_is_refused() {
    let root = corpus(SERVED_V1);
    assert_served_manifest(
        &root,
        "2753305ec614b7ed8c0ea08022bdbdc5bddde26cf0e683eb706d38ed5a97d8c5",
    );

    let request: Value =
        serde_json::from_slice(&fs::read(root.join("flow-secrets-request.json")).unwrap()).unwrap();
    let fixture = Fixture::open(None).await;
    fixture.clock.set(FILED_AT_MS);
    let requester = Stamp::from_bind(
        &flow_card_structure::bind("sample-flow-runner").with_principal(worker::principal("basal")),
    );
    let before = fixture.rows();
    let error = fixture
        .call(&requester, "consent.request", request["params"].clone())
        .await
        .unwrap_err();
    assert_eq!(error["code"], "elicitation_invalid_flow_secrets");
    assert_eq!(error["message"], "flow_install.secrets[0] statement use.rules requires a wildcard host when there is only one rule");
    assert_eq!(fixture.rows(), before);
}

#[tokio::test]
async fn basal_flow_install_cards_served_v1_1_regenerates_byte_for_byte() {
    let root = corpus(SERVED_V1_1);
    assert_regenerates(&root, &root).await;
}

#[tokio::test]
async fn basal_flow_install_cards_served_v1_2_regenerates_byte_for_byte() {
    let root = corpus(SERVED_V1_2);
    assert_regenerates(&root, &root).await;
}

async fn assert_regenerates(input_dir: &Path, output_dir: &Path) {
    let mut files =
        basal_flow_install_cards::generate(input_dir, output_dir, ARCHIVED_REQUEST_SUFFIX).await;
    let mut actual: BTreeSet<_> = fs::read_dir(output_dir)
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
        assert_eq!(
            fs::read(output_dir.join(&name)).unwrap(),
            bytes,
            "{name} differs"
        );
    }
}

#[test]
fn basal_flow_install_cards_served_v1_1_manifest_is_pinned_and_covers_exact_bytes() {
    assert_served_manifest(
        &corpus(SERVED_V1_1),
        "25060e1900620ecb1dc7f793ecbbcee749fd584e97b0025725aacb751c70e1fa",
    );
}

#[test]
fn basal_flow_install_cards_served_v1_2_manifest_is_pinned_and_covers_exact_bytes() {
    assert_served_manifest(
        &corpus(SERVED_V1_2),
        "e1b5517e5129d20d633177d9c7ff5c094c8a2bcdc91c9c2e1256c910b10aba6a",
    );
}

fn assert_served_manifest(root: &Path, expected_digest: &str) {
    let manifest = fs::read(root.join("SHA256SUMS")).unwrap();
    assert_eq!(format!("{:x}", Sha256::digest(&manifest)), expected_digest);
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
    assert_filing_manifest(
        &corpus(SERVED_V1),
        "58fd4d3c51c9a66b04f54fafd21fd244132406619b8f4a5b1f0b3d9c4c2c0f28",
    );
}

#[test]
fn basal_flow_install_cards_v1_1_inputs_match_filing_samples_manifest() {
    assert_filing_manifest(
        &corpus(SERVED_V1_1),
        "7991cfdb1b99feafe843dba5e0b763b1e9b03ace6a62d941ad930ced5de3e725",
    );
}

#[test]
fn basal_flow_install_cards_v1_2_inputs_match_filing_samples_manifest() {
    assert_filing_manifest(
        &corpus(SERVED_V1_2),
        "0d282ed53ebfc05c1ada76b53e22b7ea53913c9822229b5290bdd8830a43a4e4",
    );
}

fn assert_filing_manifest(root: &Path, expected_digest: &str) {
    // Reconstruct the filing samples' manifest with the original filenames.
    let manifest: String = CARDS
        .into_iter()
        .map(|card| {
            let bytes = fs::read(root.join(format!("{card}{ARCHIVED_REQUEST_SUFFIX}"))).unwrap();
            format!("{:x}  {card}.json\n", Sha256::digest(bytes))
        })
        .collect();
    assert_eq!(
        format!("{:x}", Sha256::digest(manifest.as_bytes())),
        expected_digest
    );
}

#[test]
fn basal_flow_install_cards_serve_full_explanation_and_statement_deadline() {
    assert_explanation_and_deadline(&corpus(SERVED_V1));
}

#[test]
fn basal_flow_install_cards_v1_1_serve_full_explanation_and_statement_deadline() {
    assert_explanation_and_deadline(&corpus(SERVED_V1_1));
}

#[test]
fn basal_flow_install_cards_v1_2_serve_full_explanation_and_statement_deadline() {
    assert_explanation_and_deadline(&corpus(SERVED_V1_2));
}

fn assert_explanation_and_deadline(root: &Path) {
    let read =
        |name| -> Value { serde_json::from_slice(&fs::read(root.join(name)).unwrap()).unwrap() };
    let filing = read("max-explanation-request.json");
    let phone = read("max-explanation-phone-get.json");
    let served = phone["result"]["explanation"]["text"].as_str().unwrap();
    assert_eq!(served, filing["params"]["explanation"]["text"]);
    assert_eq!(served.chars().count(), 1_500);
    assert_eq!(served.len(), 6_000);

    let filing = read("flow-secrets-request.json");
    let secrets = &filing["params"]["flow_install"]["secrets"];
    let deadline = secrets
        .as_array()
        .unwrap()
        .iter()
        .map(|secret| {
            let bytes = URL_SAFE_NO_PAD
                .decode(secret["statement_b64url"].as_str().unwrap())
                .unwrap();
            let statement: Value = serde_json::from_slice(&bytes).unwrap();
            statement["request_expires_at_ms"].as_i64().unwrap()
        })
        .min()
        .unwrap();
    assert_eq!(deadline, 1_777_593_660_000);
    let secret_get = read("flow-secrets-phone-get.json");
    assert_eq!(secret_get["result"]["expiresAtMs"], deadline);
    assert_eq!(&secret_get["result"]["flowInstall"]["secrets"], secrets);
    for name in ["phone-list-pending.json", "desktop-list-pending.json"] {
        let list = read(name);
        let secret_row = list["result"]["records"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["elicitationId"] == secret_get["result"]["elicitationId"])
            .unwrap();
        assert_eq!(secret_row["expiresAtMs"], deadline, "{name}");
        assert_eq!(&secret_row["flowInstall"]["secrets"], secrets, "{name}");
    }
}
