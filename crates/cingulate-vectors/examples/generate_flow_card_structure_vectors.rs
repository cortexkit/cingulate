#[allow(dead_code)]
#[path = "../tests/support/flow_card_structure.rs"]
mod flow_card_structure;
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
    let check = match args.as_slice() {
        [] => false,
        [flag] if flag == "--check" => true,
        _ => panic!("usage: generate_flow_card_structure_vectors [--check]"),
    };
    let root = support::corpus(flow_card_structure::CORPUS);
    let files = flow_card_structure::generate().await;
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
            assert_eq!(fs::read(path).unwrap(), *bytes, "{name} differs");
        } else if !fs::read(&path).is_ok_and(|existing| existing == *bytes) {
            fs::write(path, bytes).unwrap();
        }
    }
    println!(
        "{} {} flow-card-structure-v1 files through the consent handlers",
        if check { "Checked" } else { "Generated" },
        files.len()
    );
}
