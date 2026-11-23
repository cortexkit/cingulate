#[path = "../tests/support/sandbox_escape.rs"]
mod sandbox_escape;
#[path = "../tests/support/mod.rs"]
mod support;

use std::fs;

#[tokio::main]
async fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert!(args.is_empty() || args == ["--check"]);
    let root = support::corpus(sandbox_escape::CORPUS);
    let files = sandbox_escape::generate().await;
    for (name, bytes) in &files {
        if args == ["--check"] {
            assert_eq!(&fs::read(root.join(name)).unwrap(), bytes, "{name}");
        } else if *name != "README.md" {
            fs::write(root.join(name), bytes).unwrap();
        }
    }
    println!(
        "{} {} sandbox-escape vector files",
        if args.is_empty() {
            "Generated"
        } else {
            "Checked"
        },
        files.len()
    );
}
