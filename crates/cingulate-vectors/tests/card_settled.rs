#[path = "support/card_settled.rs"]
mod card_settled;

use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs};

#[test]
fn card_settled_v1_production_bytes_and_checksums() {
    let root = card_settled::corpus();
    let mut files = card_settled::generate();
    assert_eq!(
        files.len(),
        24,
        "two messages per filer, each with subject, headers and body"
    );
    for (index, filer) in ["aft", "plexus", "basal", "broca"].iter().enumerate() {
        for reader in [*filer, "prefrontal-core"] {
            let prefix = format!("{filer}.{reader}");
            assert_eq!(
                files[&format!("{prefix}.subject.txt")],
                format!("ck.box_example.event.cingulate.card_settled.v1.{reader}").as_bytes()
            );
            let body: serde_json::Value =
                serde_json::from_slice(&files[&format!("{prefix}.body.json")]).unwrap();
            assert_eq!(body.as_object().unwrap().len(), 3);
            assert_eq!(body["seq"], index + 1);
            assert_eq!(
                body["feed"],
                if reader == "prefrontal-core" {
                    "outcomes"
                } else {
                    "answers"
                }
            );
        }
    }
    files.insert(
        "README.md".into(),
        fs::read(root.join("README.md")).unwrap(),
    );
    let sums: String = files
        .iter()
        .map(|(name, bytes)| format!("{:x}  {name}\n", Sha256::digest(bytes)))
        .collect();
    files.insert("SHA256SUMS".into(), sums.into_bytes());
    let names: BTreeSet<_> = fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, files.keys().cloned().collect());
    for (name, bytes) in files {
        assert_eq!(fs::read(root.join(&name)).unwrap(), bytes, "{name}");
    }
}
