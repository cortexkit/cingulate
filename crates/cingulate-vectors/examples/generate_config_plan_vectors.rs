#[path = "../tests/support/config_plan.rs"]
mod config_plan;
#[path = "../tests/support/mod.rs"]
mod support;

use sha2::{Digest, Sha256};
use std::fs;

#[tokio::main]
async fn main() {
    let root = support::corpus(config_plan::CORPUS);
    fs::create_dir_all(&root).unwrap();
    let mut files = config_plan::generate().await;
    files.insert(
        "README.md".into(),
        fs::read(root.join("README.md")).unwrap(),
    );
    let sums: String = files
        .iter()
        .map(|(name, bytes)| format!("{:x}  {name}\n", Sha256::digest(bytes)))
        .collect();
    files.insert("SHA256SUMS".into(), sums.into_bytes());
    let check = std::env::args().skip(1).collect::<Vec<_>>() == ["--check"];
    for (name, bytes) in &files {
        if check {
            assert_eq!(fs::read(root.join(name)).unwrap(), *bytes);
        } else {
            fs::write(root.join(name), bytes).unwrap();
        }
    }
    println!(
        "{} {} files from the Cingulate handler",
        if check { "Checked" } else { "Generated" },
        files.len()
    );
}
