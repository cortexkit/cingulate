#[path = "../tests/support/broker_action.rs"]
mod broker_action;
#[path = "../tests/support/mod.rs"]
mod support;

use std::fs;

#[tokio::main]
async fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert!(args.is_empty() || args == ["--check"]);
    let root = support::corpus(broker_action::CORPUS);
    let files = broker_action::generate().await;
    for (name, bytes) in &files {
        if args == ["--check"] {
            assert_eq!(&fs::read(root.join(name)).unwrap(), bytes, "{name}");
        } else if *name != "README.md" {
            fs::write(root.join(name), bytes).unwrap();
        }
    }
    println!(
        "{} {} broker-action vector files",
        if args.is_empty() {
            "Generated"
        } else {
            "Checked"
        },
        files.len()
    );
}
