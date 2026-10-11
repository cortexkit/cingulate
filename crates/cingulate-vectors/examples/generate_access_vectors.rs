#[path = "../tests/support/access.rs"]
mod access;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs};
#[tokio::main]
async fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert!(
        args.is_empty() || args == ["--check"],
        "usage: generate_access_vectors [--check]"
    );
    let check = !args.is_empty();
    let root = access::root();
    let mut files = access::generate().await;
    files.insert(
        "README.md".into(),
        fs::read(root.join("README.md")).unwrap(),
    );
    let sums: String = files
        .iter()
        .map(|(name, bytes)| format!("{:x}  {name}\n", Sha256::digest(bytes)))
        .collect();
    files.insert("SHA256SUMS".into(), sums.into_bytes());
    if check {
        let actual: BTreeSet<_> = fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(actual, files.keys().cloned().collect());
    }
    for (name, bytes) in &files {
        if check {
            assert_eq!(fs::read(root.join(name)).unwrap(), *bytes, "{name}");
        } else {
            fs::write(root.join(name), bytes).unwrap();
        }
    }
    println!(
        "{} {} permission files",
        if check { "Checked" } else { "Generated" },
        files.len()
    );
}
