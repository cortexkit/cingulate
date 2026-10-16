#[path = "support/consent_outcomes_refs.rs"]
mod consent_outcomes_refs;
#[allow(dead_code)]
#[path = "support/phone.rs"]
mod phone;
mod support;

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs};

use support::corpus;

#[tokio::test]
async fn consent_outcomes_refs_vectors_regenerate_byte_for_byte() {
    let root = corpus(consent_outcomes_refs::CORPUS);
    let mut generated = consent_outcomes_refs::generate().await;
    generated.insert(
        "README.md".into(),
        fs::read(root.join("README.md")).expect("corpus README"),
    );
    let sums: String = generated
        .iter()
        .map(|(name, bytes)| format!("{:x}  {name}\n", Sha256::digest(bytes)))
        .collect();
    generated.insert("SHA256SUMS".into(), sums.into_bytes());

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
        let actual = fs::read(root.join(&name)).unwrap();
        if name.ends_with("outcomes-reply.json") {
            let reply: Value = serde_json::from_slice(&actual).unwrap();
            assert!(reply["result"]["records"].is_array(), "{name}");
        }
        assert_eq!(actual, bytes, "served vector differs: {name}");
    }
}
