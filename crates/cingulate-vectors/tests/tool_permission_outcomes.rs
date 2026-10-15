mod support;

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use serde_json::Value;
use sha2::{Digest, Sha256};
use support::corpus;

#[derive(Debug)]
struct CodeRow {
    code: String,
    emitters: BTreeSet<String>,
    stages: BTreeSet<String>,
}

impl CodeRow {
    /// Vector file stems this code needs, each with the stage whose shape it
    /// holds. A two-stage code keeps `<code>` for the request-time refusal and
    /// adds `<code>.outcome` for the `consent.await` outcome.
    fn vector_stems(&self) -> BTreeMap<String, &'static str> {
        let mut stems = BTreeMap::new();
        if self.stages.contains("request") {
            stems.insert(self.code.clone(), "request");
            if self.stages.contains("outcome") {
                stems.insert(format!("{}.outcome", self.code), "outcome");
            }
        } else {
            stems.insert(self.code.clone(), "outcome");
        }
        stems
    }
}

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn contract_rows() -> BTreeMap<String, CodeRow> {
    let contract =
        fs::read_to_string(repository_root().join("docs/contracts/tool-permission-outcomes-v1.md"))
            .unwrap();
    let mut lines = contract
        .lines()
        .filter(|line| line.trim_start().starts_with('|'));
    let header: Vec<_> = lines
        .next()
        .unwrap()
        .trim()
        .trim_matches('|')
        .split('|')
        .map(str::trim)
        .collect();
    assert_eq!(
        header,
        [
            "code",
            "when it happens (one line)",
            "who emits it",
            "when it arises"
        ]
    );

    let mut rows = BTreeMap::new();
    for line in lines {
        let cells: Vec<_> = line
            .trim()
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        if cells.len() != 4 || cells[0].chars().all(|ch| ch == '-' || ch == ':') {
            continue;
        }
        let code = cells[0].trim_matches('`').to_owned();
        let emitters = cells[2]
            .split(',')
            .map(|emitter| emitter.trim().trim_matches('`').to_owned())
            .collect();
        let stages: BTreeSet<String> = cells[3]
            .split(',')
            .map(|stage| stage.trim().trim_matches('`').to_owned())
            .collect();
        assert!(!stages.is_empty(), "{code}: no stage");
        for stage in &stages {
            assert!(stage == "outcome" || stage == "request", "{code}: {stage}");
        }
        assert!(
            rows.insert(
                code.clone(),
                CodeRow {
                    code: code.clone(),
                    emitters,
                    stages,
                }
            )
            .is_none(),
            "duplicate contract code {code}"
        );
    }
    rows
}

/// Every vector stem the contract requires, mapped to its code and stage.
fn required_stems(rows: &BTreeMap<String, CodeRow>) -> BTreeMap<String, (String, &'static str)> {
    let external = external_only_codes(rows);
    rows.values()
        .filter(|row| external.contains(&row.code))
        .flat_map(|row| {
            row.vector_stems()
                .into_iter()
                .map(|(stem, stage)| (stem, (row.code.clone(), stage)))
        })
        .collect()
}

fn external_only_codes(rows: &BTreeMap<String, CodeRow>) -> BTreeSet<String> {
    rows.values()
        .filter(|row| {
            !row.emitters.contains("cingulate")
                && (row.emitters.contains("thalamus-harness") || row.emitters.contains("aft"))
        })
        .map(|row| row.code.clone())
        .collect()
}

fn vector_files() -> BTreeMap<String, PathBuf> {
    fs::read_dir(corpus("tool-permission-harness-v1"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .map(|path| {
            let code = path.file_stem().unwrap().to_str().unwrap().to_owned();
            (code, path)
        })
        .collect()
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[test]
fn contract_requires_vectors_for_non_cingulate_emitters() {
    let rows = contract_rows();
    let vectors = vector_files();
    for (stem, (code, stage)) in required_stems(&rows) {
        assert!(
            vectors.contains_key(&stem),
            "missing {stage} vector {stem}.json for contract code {code}"
        );
    }
}

#[test]
fn vector_filenames_and_json_codes_match_contract() {
    let required = required_stems(&contract_rows());
    for (stem, path) in vector_files() {
        let (code, stage) = required.get(&stem).unwrap_or_else(|| {
            panic!("vector filename {stem}.json is not a required contract vector")
        });
        let code = code.as_str();
        let bytes = fs::read(&path).unwrap();
        let value: Value = serde_json::from_slice(&bytes)
            .unwrap_or_else(|error| panic!("{} is not JSON: {error}", path.display()));
        match *stage {
            "outcome" => {
                assert_eq!(value["result"]["outcome"], code, "{}", path.display());
                assert_eq!(value["result"]["code"], code, "{}", path.display());
                if code == "access_unrecognized" {
                    let note = value["result"]["note"]
                        .as_str()
                        .expect("access_unrecognized carries a note");
                    assert!(note.chars().count() <= 160, "note exceeds 160 characters");
                }
            }
            "request" => assert_eq!(value["code"], code, "{}", path.display()),
            _ => unreachable!(),
        }
    }
}

#[test]
fn harness_vector_checksums_cover_every_file() {
    let root = corpus("tool-permission-harness-v1");
    let manifest = fs::read_to_string(root.join("SHA256SUMS")).unwrap();
    let mut listed = BTreeSet::new();
    for line in manifest.lines().filter(|line| !line.is_empty()) {
        let (digest, name) = line.split_once("  ").expect("checksum separator");
        assert_eq!(digest.len(), 64, "{name}");
        assert!(
            digest
                .chars()
                .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase()),
            "digest must be lowercase hexadecimal: {name}"
        );
        assert!(!name.contains('/') && name != "SHA256SUMS");
        assert!(
            listed.insert(name.to_owned()),
            "duplicate checksum path {name}"
        );
        assert_eq!(
            sha256(&fs::read(root.join(name)).unwrap()),
            digest,
            "{name}"
        );
    }
    let actual: BTreeSet<String> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            assert!(entry.file_type().unwrap().is_file());
            entry.file_name().into_string().unwrap()
        })
        .filter(|name| name != "SHA256SUMS")
        .collect();
    assert_eq!(
        actual, listed,
        "manifest must cover every file except itself"
    );
}

fn rust_sources(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        let file_type = fs::metadata(&path).unwrap().file_type();
        if file_type.is_dir() {
            rust_sources(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

#[test]
fn harness_only_codes_are_not_string_literals_in_cingulate_sources() {
    let harness_only = external_only_codes(&contract_rows());
    let root = repository_root();
    let mut files = Vec::new();
    rust_sources(&root.join("crates/ck-cingulate/src"), &mut files);
    rust_sources(&root.join("crates/cingulate-store/src"), &mut files);

    for path in files {
        let source = fs::read_to_string(&path).unwrap();
        for code in &harness_only {
            assert!(
                !source.contains(&format!("\"{code}\"")),
                "Cingulate source {} contains harness-only code literal {code}",
                path.display()
            );
        }
    }
}
