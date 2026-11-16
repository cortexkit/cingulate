#[path = "support/local_secret_cards.rs"]
mod local_secret_cards;
#[allow(dead_code)]
#[path = "support/phone.rs"]
mod phone;
mod support;
#[allow(dead_code)]
#[path = "support/watch_pending.rs"]
mod watch_pending;

use std::{collections::BTreeSet, fs};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use cingulate_decode::ConsentRequest;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use support::corpus;

#[tokio::test]
async fn local_secret_cards_served_vectors_regenerate_byte_for_byte() {
    assert_regenerates(local_secret_cards::CORPUS).await;
}

#[tokio::test]
async fn local_secret_cards_served_v1_1_vectors_regenerate_byte_for_byte() {
    assert_regenerates(local_secret_cards::CORPUS_V1_1).await;
}

async fn assert_regenerates(corpus_name: &str) {
    let root = corpus(corpus_name);
    // Only the historical SSH vault statement is invalid under samples v1.2.
    // Keep byte-for-byte regeneration for every other card, including ordinary SSH.
    let cards: Vec<_> = local_secret_cards::CARDS
        .into_iter()
        .filter(|card| *card != "ssh-vault")
        .collect();
    let mut generated = local_secret_cards::generate_cards(corpus_name, &cards).await;
    // The full, unchanged manifest is checked separately; this map is a subset.
    generated.remove("SHA256SUMS");
    assert_eq!(generated.len(), 35);
    let actual: BTreeSet<_> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name != "SHA256SUMS" && !name.starts_with("ssh-vault-"))
        .collect();
    assert_eq!(
        actual,
        generated.keys().cloned().collect(),
        "{corpus_name} file coverage"
    );
    for (name, bytes) in generated {
        assert_eq!(
            fs::read(root.join(&name)).unwrap(),
            bytes,
            "{corpus_name} served vector differs: {name}"
        );
    }
}

#[test]
fn local_secret_cards_served_v1_ssh_with_purpose_is_historically_refused() {
    // Since the vault's samples v1.2, an SSH statement carrying purpose is refused.
    assert_historical_ssh_purpose_refused(local_secret_cards::CORPUS);
}

#[test]
fn local_secret_cards_served_v1_1_ssh_with_purpose_is_historically_refused() {
    // Since the vault's samples v1.2, an SSH statement carrying purpose is refused.
    assert_historical_ssh_purpose_refused(local_secret_cards::CORPUS_V1_1);
}

fn assert_historical_ssh_purpose_refused(corpus_name: &str) {
    let input = fs::read(corpus(corpus_name).join("ssh-vault-request.json")).unwrap();
    let mut params: Value = serde_json::from_slice(&input).unwrap();
    let error = ConsentRequest::decode(params.clone()).unwrap_err();
    assert_eq!(error.code, "elicitation_invalid_request");
    let item = &mut params["secret_use"]["items"][0];
    let bytes = URL_SAFE_NO_PAD
        .decode(item["statement_b64url"].as_str().unwrap())
        .unwrap();
    let mut statement: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(statement["program"], "ssh");
    assert!(statement["purpose"].is_string());
    // Removing only purpose must make the same historical statement valid.
    statement.as_object_mut().unwrap().remove("purpose");
    item["statement_b64url"] =
        json!(URL_SAFE_NO_PAD.encode(serde_jcs::to_vec(&statement).unwrap()));
    ConsentRequest::decode(params).unwrap();
}

#[test]
fn local_secret_cards_served_manifest_is_pinned_and_covers_exact_bytes() {
    assert_manifest(
        local_secret_cards::CORPUS,
        "ae024d1b8b165f21ee0ed451a610892a9a2c8ac08092db67e2df5e80ce817188",
    );
}

#[test]
fn local_secret_cards_served_v1_1_manifest_is_pinned_and_covers_exact_bytes() {
    assert_manifest(
        local_secret_cards::CORPUS_V1_1,
        "c1082cb37173e1e0fb298cb2962f18942c929d0dcaf5d81676a89adc6a585061",
    );
}

fn assert_manifest(corpus_name: &str, pin: &str) {
    let root = corpus(corpus_name);
    let manifest = fs::read(root.join("SHA256SUMS")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&manifest)),
        pin,
        "{corpus_name} manifest pin"
    );
    let mut listed = BTreeSet::new();
    for line in std::str::from_utf8(&manifest).unwrap().lines() {
        let (digest, name) = line.split_once("  ").unwrap();
        assert!(!name.contains('/') && name != "SHA256SUMS");
        assert!(listed.insert(name.to_owned()), "duplicate path {name}");
        assert_eq!(
            format!("{:x}", Sha256::digest(fs::read(root.join(name)).unwrap())),
            digest,
            "{name}"
        );
    }
    let actual: BTreeSet<_> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name != "SHA256SUMS")
        .collect();
    assert_eq!(
        actual, listed,
        "manifest must cover every file except itself"
    );
}
