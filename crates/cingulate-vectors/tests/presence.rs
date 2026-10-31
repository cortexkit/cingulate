#[path = "support/presence.rs"]
mod presence;

use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs};

#[tokio::test(start_paused = true)]
async fn presence_vectors_regenerate_byte_for_byte() {
    let root = presence::root();
    let mut files = presence::generate().await;
    files.insert(
        "README.md".into(),
        fs::read(root.join("README.md")).unwrap(),
    );
    let sums: String = files
        .iter()
        .map(|(name, bytes)| format!("{:x}  {name}\n", Sha256::digest(bytes)))
        .collect();
    files.insert("SHA256SUMS".into(), sums.into_bytes());
    let actual: BTreeSet<_> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(actual, files.keys().cloned().collect());
    for (name, bytes) in files {
        assert_eq!(fs::read(root.join(&name)).unwrap(), bytes, "{name}");
    }
}

#[test]
fn presence_checksum_manifest_pin() {
    let root = presence::root();
    let manifest = fs::read(root.join("SHA256SUMS")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&manifest)),
        "9bb85e38455bb6cb8ff1c42b294cd3b89081ce48cf11b0c6cedac702ce3ac5aa"
    );
    for line in std::str::from_utf8(&manifest).unwrap().lines() {
        let (digest, name) = line.split_once("  ").unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(fs::read(root.join(name)).unwrap())),
            digest,
            "{name}"
        );
    }
}
