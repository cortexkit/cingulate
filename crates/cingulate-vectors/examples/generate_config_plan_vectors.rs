#[path = "../tests/support/config_plan.rs"]
mod config_plan;
#[path = "../tests/support/mod.rs"]
mod support;

use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};

#[tokio::main]
async fn main() {
    let source = support::corpus(config_plan::CORPUS);
    let args: Vec<_> = std::env::args().skip(1).collect();
    let (root, check) = match args.as_slice() {
        [] => (source.clone(), false),
        [flag] if flag == "--check" => (source.clone(), true),
        [flag, path] if flag == "--output" => (PathBuf::from(path), false),
        _ => panic!("usage: generate_config_plan_vectors [--check | --output DIRECTORY]"),
    };
    fs::create_dir_all(&root).unwrap();
    let mut files = config_plan::generate().await;
    files.insert(
        "README.md".into(),
        fs::read(source.join("README.md")).unwrap(),
    );
    let sums: String = files
        .iter()
        .map(|(name, bytes)| format!("{:x}  {name}\n", Sha256::digest(bytes)))
        .collect();
    files.insert("SHA256SUMS".into(), sums.into_bytes());
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
