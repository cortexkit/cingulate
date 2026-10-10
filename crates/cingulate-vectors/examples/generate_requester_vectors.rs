#[path = "../tests/support/phone.rs"]
#[allow(dead_code)]
mod phone;
#[path = "../tests/support/requester.rs"]
mod requester;
#[path = "../tests/support/mod.rs"]
mod support;

use sha2::{Digest, Sha256};
use std::fs;

#[tokio::main]
async fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert!(
        args.is_empty() || args == ["--check"],
        "usage: generate_requester_vectors [--check]"
    );
    let check = !args.is_empty();
    let root = support::corpus(requester::CORPUS);
    let mut files = requester::generate().await;
    files.insert(
        "README.md".into(),
        fs::read(root.join("README.md")).expect("corpus README"),
    );
    let sums: String = files
        .iter()
        .map(|(name, bytes)| format!("{:x}  {name}\n", Sha256::digest(bytes)))
        .collect();
    files.insert("SHA256SUMS".into(), sums.into_bytes());
    if check {
        let actual: std::collections::BTreeSet<_> = fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(
            actual,
            files.keys().cloned().collect(),
            "corpus file coverage"
        );
    }
    for (name, bytes) in &files {
        let path = root.join(name);
        if check {
            assert_eq!(
                fs::read(&path).unwrap(),
                *bytes,
                "{} differs",
                path.display()
            );
        } else {
            fs::write(&path, bytes).unwrap();
        }
    }
    println!(
        "{} {} files from the requester handler",
        if check { "Checked" } else { "Generated" },
        files.len()
    );
}
