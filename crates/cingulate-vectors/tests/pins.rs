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
        "f3f569f6de07091c51e2efe1e66b284ec536c9c9f73b2fb8928b8844652d9296",
    );
}

#[test]
fn requester_manifest_hash_is_pinned() {
    pin(
        "elicitation-requester-v1",
        "93b626ff5a95f99a3d9bcedda0be14e80a8ffcd8c8109721c28faf1beeb3baf6",
    );
}

#[test]
fn consent_phone_settled_manifest_hash_is_pinned() {
    pin(
        "consent-phone-settled-v1",
        "e4e3e90421f5a470636d74ca69c0b0bbad7f878c78ef7f5d7f5164280a2202c9",
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
        "af276ebaccb2dbfecdba095a7b5f13f0b31a3869c24285b8d746705615e1e11c",
    );
}

#[test]
fn danger_option_group_manifest_hash_is_pinned() {
    pin(
        "danger-option-group-v1",
        "2f3fc873cb1d4d2f110f85a369b88efd753ed667fe7a6fe21ef9ec524fd8c171",
    );
}

#[test]
fn permissions_manifest_hash_is_pinned() {
    pin(
        "permissions-v1",
        "57f5ffc1f833b463d76b95015b6828dc6ef777d952d55e0d479565cb45ea5112",
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
        "a9464870dee5e98bf66ea6802b2a5a139031799b42e0880e21f125122ef927d3",
    );
    coverage("consent-outcomes-refs-v1", 34);
}

#[test]
fn permissions_manifest_covers_exact_bytes() {
    coverage("permissions-v1", 100);
}

#[test]
fn cerebellum_agent_permission_manifest_hash_is_pinned() {
    pin(
        "cerebellum-agent-permission-v1",
        "6ca3525683919dc6453ea257eef56cc43993df621ae03793fc666b4389a2d9b0",
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
