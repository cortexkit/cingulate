mod support;

use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path},
};

use sha2::{Digest, Sha256};
use support::{corpus, REQUESTER_ORDER};

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn pin(name: &str, expected: &str) {
    assert_eq!(
        sha256(&fs::read(corpus(name).join("SHA256SUMS")).unwrap()),
        expected,
        "{name} manifest pin"
    );
}

fn coverage(name: &str, count: usize) {
    let manifest = fs::read_to_string(corpus(name).join("SHA256SUMS")).unwrap();
    let mut listed = BTreeSet::new();
    for line in manifest.lines().filter(|line| !line.is_empty()) {
        let (digest, path) = line.split_once("  ").unwrap();
        assert_eq!(digest.len(), 64);
        assert!(digest.chars().all(|ch| ch.is_ascii_hexdigit()));
        assert!(!path.contains('/') && path != "SHA256SUMS");
        assert!(listed.insert(path.to_owned()), "duplicate path {path}");
        assert_eq!(
            sha256(&fs::read(corpus(name).join(path)).unwrap()),
            digest,
            "{name}/{path}"
        );
    }
    let actual: BTreeSet<String> = fs::read_dir(corpus(name))
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            assert!(entry.file_type().unwrap().is_file());
            entry.file_name().into_string().unwrap()
        })
        .filter(|path| path != "SHA256SUMS")
        .collect();
    assert_eq!(listed.len(), count, "{name}");
    assert_eq!(
        actual, listed,
        "manifest must cover every file except itself"
    );
}

#[test]
fn records_manifest_hash_is_pinned() {
    pin(
        "elicitation-records-v1",
        "2254ccebc065107a5df2fb876932bddf7e28f3f960d21c906c1d8c3e87412f34",
    );
}

#[test]
fn requester_manifest_hash_is_pinned() {
    pin(
        "elicitation-requester-v1",
        "0701f1f3f1ec489a616a4b1ceb46328e93b4f9e524c265036b69880c81b50095",
    );
}

#[test]
fn consent_phone_settled_manifest_hash_is_pinned() {
    pin(
        "consent-phone-settled-v1",
        "9345562b76f33784cbd2cefe2f8cfddfbfb16fbf012bf7d3f5dc81cd20877c8c",
    );
}

#[test]
fn consent_phone_settled_manifest_covers_exact_bytes() {
    coverage("consent-phone-settled-v1", 10);
}

#[test]
fn consent_manifest_hash_is_pinned() {
    pin(
        "consent-records-v1",
        "383fc5632149c47abf9e8c3c7f0e6e8ea8b23b7a0525d57644e196e57dd894aa",
    );
}

#[test]
fn danger_option_group_manifest_hash_is_pinned() {
    pin(
        "danger-option-group-v1",
        "d69d02a1fb00a20582dcaefb8eda78baa07cb4237a2cede26228ff2651e54426",
    );
}

#[test]
fn permissions_manifest_hash_is_pinned() {
    pin(
        "permissions-v1",
        "a27998d8aa8a643a73eaafb945e16760f1db7645b69c0e8c741c9602ca6c0284",
    );
}

#[test]
fn danger_option_group_manifest_covers_exact_bytes() {
    coverage("danger-option-group-v1", 31);
}

#[test]
fn consent_outcomes_refs_manifest_covers_exact_bytes() {
    pin(
        "consent-outcomes-refs-v1",
        "58bca98402570d3b62cb7d938d99644bb59f574d315c50e0e09b0f0ee234e28a",
    );
    coverage("consent-outcomes-refs-v1", 41);
}

#[test]
fn permissions_manifest_covers_exact_bytes() {
    coverage("permissions-v1", 130);
}

#[test]
fn cerebellum_agent_permission_manifest_hash_is_pinned() {
    pin(
        "cerebellum-agent-permission-v1",
        "f31a149982d10526e2cf221e14cbb2fd52ad314c82ecee4af03707454c9bde7b",
    );
}

fn collect_corpus_files(root: &Path, directory: &Path, files: &mut BTreeSet<String>) {
    for entry in fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if entry.file_type().unwrap().is_dir() {
            collect_corpus_files(root, &path, files);
        } else {
            assert!(entry.file_type().unwrap().is_file());
            files.insert(
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
}

#[test]
fn cerebellum_agent_permission_manifest_covers_exact_bytes() {
    let root = corpus("cerebellum-agent-permission-v1");
    let manifest = fs::read_to_string(root.join("SHA256SUMS")).unwrap();
    let mut listed = BTreeSet::new();
    for line in manifest.lines().filter(|line| !line.is_empty()) {
        let (digest, path) = line.split_once("  ").unwrap();
        let relative = Path::new(path);
        assert!(relative
            .components()
            .all(|component| matches!(component, Component::Normal(_))));
        assert_ne!(path, "SHA256SUMS");
        assert!(listed.insert(path.to_owned()), "duplicate path {path}");
        assert_eq!(
            sha256(&fs::read(root.join(relative)).unwrap()),
            digest,
            "cerebellum-agent-permission-v1/{path}"
        );
    }

    let mut actual = BTreeSet::new();
    collect_corpus_files(&root, &root, &mut actual);
    actual.remove("SHA256SUMS");
    assert_eq!(listed.len(), 15);
    assert_eq!(
        actual, listed,
        "manifest must cover every file except itself"
    );
}

#[test]
fn records_manifest_covers_exact_bytes() {
    coverage("elicitation-records-v1", 9);
}

#[test]
fn requester_manifest_covers_exact_bytes() {
    coverage("elicitation-requester-v1", 18);
}

#[test]
fn consent_manifest_covers_exact_bytes() {
    coverage("consent-records-v1", 6);
}

#[test]
fn requester_source_sha_table_pins_json_bytes_and_scenario_order() {
    let readme = fs::read_to_string(corpus("elicitation-requester-v1").join("README.md")).unwrap();
    let mut files = Vec::new();
    for line in readme
        .lines()
        .filter(|line| line.starts_with("| `") && line.contains(".json` | `"))
    {
        let columns: Vec<&str> = line.split('`').collect();
        let (file, digest) = (columns[1], columns[3]);
        assert_eq!(
            sha256(&fs::read(corpus("elicitation-requester-v1").join(file)).unwrap()),
            digest,
            "{file}"
        );
        files.push(file);
    }
    assert_eq!(files, REQUESTER_ORDER);
}
