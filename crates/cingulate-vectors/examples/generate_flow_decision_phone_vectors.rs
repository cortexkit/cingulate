#[path = "../tests/support/flow_decision_phone.rs"]
mod flow_decision_phone;
#[allow(dead_code)]
#[path = "../tests/support/phone.rs"]
mod phone;
#[path = "../tests/support/mod.rs"]
mod support;

use std::{collections::BTreeSet, fs};

use sha2::{Digest, Sha256};

#[tokio::main]
async fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert!(
        args.is_empty() || args == ["--check"],
        "usage: generate_flow_decision_phone_vectors [--check]"
    );
    let check = !args.is_empty();
    let root = support::corpus(flow_decision_phone::CORPUS);
    let mut files = flow_decision_phone::generate().await;
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
        let actual: BTreeSet<_> = fs::read_dir(&root)
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
        "{} {} files from the Cingulate handler",
        if check { "Checked" } else { "Generated" },
        files.len()
    );
}
