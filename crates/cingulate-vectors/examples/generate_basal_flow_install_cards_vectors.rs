#[path = "../tests/support/basal_flow_install_cards.rs"]
mod basal_flow_install_cards;
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

use std::{collections::BTreeSet, fs, path::Path};

use basal_flow_install_cards::{ARCHIVED_REQUEST_SUFFIX, SERVED_V1};

#[tokio::main]
async fn main() {
    #[allow(clippy::assertions_on_constants)]
    {
        assert!(
            ck_cingulate::DEVELOPMENT_LOCAL_OPERATOR,
            "generator requires ck-cingulate/dev-local-operator for the plain local desktop list"
        );
    }
    let args: Vec<_> = std::env::args().skip(1).collect();
    let (check, paths) = match args.as_slice() {
        [flag, paths @ ..] if flag == "--check" => (true, paths),
        paths => (false, paths),
    };
    let v1 = support::corpus(SERVED_V1);
    let (input_dir, output_dir, input_suffix) = match paths {
        [] => (v1.as_path(), v1.as_path(), ARCHIVED_REQUEST_SUFFIX),
        [served] => (Path::new(served), Path::new(served), ARCHIVED_REQUEST_SUFFIX),
        [filings, served] => (Path::new(filings), Path::new(served), ".json"),
        _ => panic!(
            "usage: generate_basal_flow_install_cards_vectors [--check] [[filing-samples-dir] served-dir]"
        ),
    };
    let files = basal_flow_install_cards::generate(input_dir, output_dir, input_suffix).await;
    if check {
        let actual: BTreeSet<_> = fs::read_dir(output_dir)
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
        let path = output_dir.join(name);
        if check {
            assert_eq!(fs::read(path).unwrap(), *bytes, "{name} differs");
        } else if !fs::read(&path).is_ok_and(|existing| existing == *bytes) {
            fs::write(path, bytes).unwrap();
        }
    }
    println!(
        "{} {} files in {} through the consent handlers",
        if check { "Checked" } else { "Generated" },
        files.len(),
        output_dir.display(),
    );
}
