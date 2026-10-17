#[path = "support/no_expiry.rs"]
mod no_expiry;
#[allow(dead_code)]
#[path = "support/phone.rs"]
mod phone;
mod support;

use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs};

#[tokio::test]
async fn no_expiry_and_execution_reason_vectors_regenerate_byte_for_byte() {
    let root = support::corpus(no_expiry::CORPUS);
    let mut generated = no_expiry::generate().await;
    generated.insert(
        "README.md".into(),
        fs::read(root.join("README.md")).unwrap(),
    );
    let sums: String = generated
        .iter()
        .map(|(name, bytes)| format!("{:x}  {name}\n", Sha256::digest(bytes)))
        .collect();
    assert_eq!(
        format!("{:x}", Sha256::digest(sums.as_bytes())),
        "51414a6e018f9440f746d9b7b85f5ae49a39bd0f7903e08e61c8a7c9d4c26894"
    );
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
        assert_eq!(
            fs::read(root.join(&name)).unwrap(),
            bytes,
            "served vector differs: {name}"
        );
    }
}
