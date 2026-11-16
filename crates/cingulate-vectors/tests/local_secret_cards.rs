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

use sha2::{Digest, Sha256};
use support::corpus;

#[tokio::test]
async fn local_secret_cards_served_vectors_regenerate_byte_for_byte() {
    let root = corpus(local_secret_cards::CORPUS);
    let generated = local_secret_cards::generate().await;
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
        assert_eq!(
            fs::read(root.join(&name)).unwrap(),
            bytes,
            "served vector differs: {name}"
        );
    }
}

#[test]
fn local_secret_cards_served_manifest_is_pinned_and_covers_exact_bytes() {
    let root = corpus(local_secret_cards::CORPUS);
    let manifest = fs::read(root.join("SHA256SUMS")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&manifest)),
        "ae024d1b8b165f21ee0ed451a610892a9a2c8ac08092db67e2df5e80ce817188",
        "local-secret-cards-served-v1 manifest pin"
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
