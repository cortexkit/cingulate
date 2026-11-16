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

#[tokio::test]
async fn local_secret_cards_served_v1_2_vectors_regenerate_byte_for_byte() {
    let root = corpus(local_secret_cards::CORPUS_V1_2);
    let generated = local_secret_cards::generate(local_secret_cards::CORPUS_V1_2).await;
    assert_eq!(generated.len(), 40);
    let actual: BTreeSet<_> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(
        actual,
        generated.keys().cloned().collect(),
        "corpus file coverage"
    );
    for (name, bytes) in generated {
        assert_eq!(fs::read(root.join(&name)).unwrap(), bytes, "{name} differs");
    }
}

#[test]
fn local_secret_cards_served_v1_2_inputs_match_aft_manifest() {
    let root = corpus(local_secret_cards::CORPUS_V1_2);
    // Reconstruct AFT's manifest using the original request filenames and order.
    let manifest: String = local_secret_cards::CARDS
        .iter()
        .map(|card| {
            let bytes = fs::read(root.join(format!("{card}-request.json"))).unwrap();
            format!("{:x}  {card}.json\n", Sha256::digest(bytes))
        })
        .collect();
    assert_eq!(
        format!("{:x}", Sha256::digest(manifest.as_bytes())),
        "a826415ddc9274000272defc69c485954214df9844783106e5b05e9dee46524e"
    );
}

#[test]
fn local_secret_cards_served_v1_2_vault_statements_match_local_use_v1_2() {
    let pack: Value = serde_json::from_slice(include_bytes!(
        "../../../test-vectors/secret-local-use-v1.2/vectors.json"
    ))
    .unwrap();
    for (card, case) in [
        ("sudo-vault", "sudo_password-local"),
        ("ssh-vault", "ssh_sign-local"),
    ] {
        let request: Value = serde_json::from_slice(
            &fs::read(corpus(local_secret_cards::CORPUS_V1_2).join(format!("{card}-request.json")))
                .unwrap(),
        )
        .unwrap();
        let sample = pack["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|sample| sample["case"] == case)
            .unwrap();
        let item = &request["secret_use"]["items"][0];
        assert_eq!(
            item["statement_b64url"], sample["statement"]["b64url"],
            "{card}"
        );
        assert_eq!(
            item["request_id"], sample["statement"]["json"]["request_id"],
            "{card}"
        );
    }
}

#[test]
fn local_secret_cards_served_v1_2_only_ssh_files_differ_from_v1_1() {
    let old_root = corpus(local_secret_cards::CORPUS_V1_1);
    let new_root = corpus(local_secret_cards::CORPUS_V1_2);
    let json_files = |root: &std::path::Path| -> BTreeSet<String> {
        fs::read_dir(root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| name.ends_with(".json"))
            .collect()
    };
    let names = json_files(&new_root);
    assert_eq!(names.len(), 38);
    assert_eq!(names, json_files(&old_root));
    for name in names {
        let old = fs::read(old_root.join(&name)).unwrap();
        let new = fs::read(new_root.join(&name)).unwrap();
        assert_eq!(old != new, name.starts_with("ssh-"), "{name}");
    }
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

#[test]
fn local_secret_cards_served_v1_2_manifest_is_pinned_and_covers_exact_bytes() {
    assert_manifest(
        local_secret_cards::CORPUS_V1_2,
        "27022959ad2ae1005400e7e21647b030b3ef729ee83a8b41b71a2204522789e2",
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
