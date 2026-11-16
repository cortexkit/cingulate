#[path = "../tests/support/local_secret_cards.rs"]
mod local_secret_cards;
#[allow(dead_code)]
#[path = "../tests/support/phone.rs"]
mod phone;
#[path = "../tests/support/mod.rs"]
mod support;
#[allow(dead_code)]
#[path = "../tests/support/watch_pending.rs"]
mod watch_pending;

use std::{collections::BTreeSet, fs};

#[tokio::main]
async fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert!(
        args.is_empty() || args == ["--check"],
        "usage: generate_local_secret_cards_vectors [--check]"
    );
    let check = !args.is_empty();
    let root = support::corpus(local_secret_cards::CORPUS);
    let files = local_secret_cards::generate().await;
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
            assert_eq!(fs::read(&path).unwrap(), *bytes, "{name} differs");
        } else if !fs::read(&path).is_ok_and(|existing| existing == *bytes) {
            fs::write(&path, bytes).unwrap();
        }
    }
    println!(
        "{} {} local-secret card vector files through the consent handlers",
        if check { "Checked" } else { "Generated" },
        files.len()
    );
}
