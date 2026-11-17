#[path = "../tests/support/config_plan.rs"]
mod config_plan;
#[path = "../tests/support/desktop_dev_cards.rs"]
mod desktop_dev_cards;
#[path = "../tests/support/flow_secrets.rs"]
mod flow_secrets;
#[path = "../tests/support/manifest_binding.rs"]
mod manifest_binding;
#[path = "../tests/support/secret_use.rs"]
mod secret_use;
#[path = "../tests/support/mod.rs"]
mod support;

#[tokio::main]
async fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert!(args.is_empty() || args == ["--check"]);
    let root = support::corpus(desktop_dev_cards::CORPUS);
    let files = desktop_dev_cards::generate().await;
    for (name, bytes) in &files {
        if args.is_empty() {
            std::fs::write(root.join(name), bytes).unwrap();
        } else {
            assert_eq!(std::fs::read(root.join(name)).unwrap(), *bytes, "{name}");
        }
    }
    println!(
        "{} {} files",
        if args.is_empty() {
            "Generated"
        } else {
            "Checked"
        },
        files.len()
    );
}
