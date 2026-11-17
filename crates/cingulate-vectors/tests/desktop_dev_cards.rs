#[path = "support/config_plan.rs"]
mod config_plan;
#[path = "support/desktop_dev_cards.rs"]
mod desktop_dev_cards;
#[path = "support/flow_secrets.rs"]
mod flow_secrets;
#[path = "support/manifest_binding.rs"]
mod manifest_binding;
#[path = "support/secret_use.rs"]
mod secret_use;
mod support;

#[tokio::test]
async fn desktop_dev_cards_regenerate_byte_for_byte() {
    if !ck_cingulate::DEVELOPMENT_LOCAL_OPERATOR {
        return;
    }
    let root = support::corpus(desktop_dev_cards::CORPUS);
    let files = desktop_dev_cards::generate().await;
    let actual: std::collections::BTreeSet<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(actual, files.keys().cloned().collect());
    for (name, bytes) in files {
        assert_eq!(std::fs::read(root.join(&name)).unwrap(), bytes, "{name}");
    }
}
