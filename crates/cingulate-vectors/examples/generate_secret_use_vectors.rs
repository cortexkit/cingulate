#[path = "../tests/support/secret_use.rs"]
mod secret_use;
#[path = "../tests/support/mod.rs"]
mod support;

use sha2::{Digest, Sha256};
use std::fs;

#[tokio::main]
async fn main() {
    let root = support::corpus(secret_use::CORPUS);
    let mut files = secret_use::generate().await;
    for name in ["README.md", "secret-approval-v1.json"] {
        files.insert(name.into(), fs::read(root.join(name)).unwrap());
    }
    let sums: String = files
        .iter()
        .map(|(name, bytes)| format!("{:x}  {name}\n", Sha256::digest(bytes)))
        .collect();
    files.insert("SHA256SUMS".into(), sums.into_bytes());
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert!(args.is_empty() || args == ["--check"] || args == ["--stdout"]);
    if args == ["--stdout"] {
        let text: std::collections::BTreeMap<_, _> = files
            .into_iter()
            .filter(|(name, _)| name != "secret-approval-v1.json" && name != "README.md")
            .map(|(name, bytes)| (name, String::from_utf8(bytes).unwrap()))
            .collect();
        println!("{}", serde_json::to_string(&text).unwrap());
        return;
    }
    for (name, bytes) in &files {
        if args == ["--check"] {
            assert_eq!(fs::read(root.join(name)).unwrap(), *bytes);
        } else {
            fs::write(root.join(name), bytes).unwrap();
        }
    }
    println!(
        "{} {} files from the Cingulate handler",
        if args.is_empty() {
            "Generated"
        } else {
            "Checked"
        },
        files.len()
    );
}
