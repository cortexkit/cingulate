#[path = "../tests/support/flow_secrets.rs"]
mod flow_secrets;
#[path = "../tests/support/forwarded_secrets.rs"]
mod forwarded_secrets;
#[path = "../tests/support/secret_use.rs"]
mod secret_use;
#[path = "../tests/support/mod.rs"]
mod support;

use sha2::{Digest, Sha256};
use std::fs;

/// Writes (or with `--check`, compares) the v2 forwarded-secret corpora. The
/// vendored vault files and the hand-written READMEs are read from disk rather
/// than generated, then listed in each corpus's `SHA256SUMS` with the rest.
#[tokio::main]
async fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert!(args.is_empty() || args == ["--check"] || args == ["--stdout"]);
    let mut corpora = [
        (
            forwarded_secrets::USE_CORPUS,
            forwarded_secrets::USE_VAULT_FILE,
            forwarded_secrets::generate_use().await,
        ),
        (
            forwarded_secrets::FLOW_CORPUS,
            forwarded_secrets::FLOW_VAULT_FILE,
            forwarded_secrets::generate_flow().await,
        ),
    ];
    for (corpus, _, files) in &mut corpora {
        let root = support::corpus(corpus);
        for name in ["filer-refusals.json", "filing-refusals.json"] {
            if let Some(current) = files.get_mut(name) {
                let pinned = fs::read(root.join(name)).unwrap();
                forwarded_secrets::check_published_refusals(name, &pinned, current);
                *current = pinned;
            }
        }
    }
    if args == ["--stdout"] {
        // Generated files only, keyed `<corpus>/<name>`, for writing them on a
        // machine other than the one that ran the handler.
        let text: std::collections::BTreeMap<_, _> = corpora
            .into_iter()
            .flat_map(|(corpus, _, files)| {
                files.into_iter().map(move |(name, bytes)| {
                    (
                        format!("{corpus}/{name}"),
                        String::from_utf8(bytes).unwrap(),
                    )
                })
            })
            .collect();
        println!("{}", serde_json::to_string(&text).unwrap());
        return;
    }
    let mut count = 0;
    for (corpus, vault_file, mut files) in corpora {
        let root = support::corpus(corpus);
        for name in ["README.md", vault_file] {
            files.insert(name.into(), fs::read(root.join(name)).unwrap());
        }
        let sums: String = files
            .iter()
            .map(|(name, bytes)| format!("{:x}  {name}\n", Sha256::digest(bytes)))
            .collect();
        files.insert("SHA256SUMS".into(), sums.into_bytes());
        for (name, bytes) in &files {
            if args == ["--check"] {
                assert_eq!(
                    fs::read(root.join(name)).unwrap(),
                    *bytes,
                    "{corpus}/{name}"
                );
            } else {
                fs::write(root.join(name), bytes).unwrap();
            }
        }
        count += files.len();
    }
    println!(
        "{} {count} files from the Cingulate handler",
        if args.is_empty() {
            "Generated"
        } else {
            "Checked"
        }
    );
}
