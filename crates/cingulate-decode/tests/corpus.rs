use std::{collections::BTreeSet, fs, path::PathBuf};

use cingulate_decode::ConsentRequest;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

fn corpus() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test-vectors/elicitation-request-v1")
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn json(path: &str) -> Value {
    serde_json::from_slice(&fs::read(corpus().join(path)).unwrap()).unwrap()
}

fn paths_under(directory: &str) -> BTreeSet<String> {
    fs::read_dir(corpus().join(directory))
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            assert!(entry.file_type().unwrap().is_file());
            format!("{directory}/{}", entry.file_name().to_str().unwrap())
        })
        .collect()
}

#[test]
fn corpus_manifest_hash_is_pinned() {
    assert_eq!(
        sha256(&fs::read(corpus().join("SHA256SUMS")).unwrap()),
        "70617099cddb426b8ce4b6cf49d04337e58dd61230f909bc0bcba31c73c64c88"
    );
}

#[test]
fn corpus_manifest_covers_exact_bytes() {
    let manifest = fs::read_to_string(corpus().join("SHA256SUMS")).unwrap();
    let mut listed = BTreeSet::new();
    for line in manifest.lines() {
        let (digest, path) = line.split_once("  ").unwrap();
        assert!(listed.insert(path.to_owned()), "duplicate manifest path");
        assert_eq!(
            sha256(&fs::read(corpus().join(path)).unwrap()),
            digest,
            "{path}"
        );
    }
    let mut actual = BTreeSet::new();
    for entry in fs::read_dir(corpus()).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().into_string().unwrap();
        if entry.file_type().unwrap().is_dir() {
            actual.extend(paths_under(&name));
        } else if name != "SHA256SUMS" {
            actual.insert(name);
        }
    }
    assert_eq!(actual, listed);
    assert_eq!(listed.len(), 56);
}

#[test]
fn corpus_valid_bodies_decode() {
    let paths = paths_under("valid");
    assert_eq!(paths.len(), 7);
    for path in paths {
        let request =
            ConsentRequest::decode(json(&path)).unwrap_or_else(|error| panic!("{path}: {error}"));
        assert!(request.requested_schema.is_none(), "{path}");
        let stored = serde_json::to_value(request).unwrap();
        assert!(stored.get("requestedSchema").is_none(), "{path}");
        assert!(stored.get("requested_schema").is_none(), "{path}");
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedRefusal {
    code: String,
    message_contains: String,
}

#[test]
fn corpus_refused_bodies_match_verdicts() {
    let paths = paths_under("refused");
    let bodies: BTreeSet<_> = paths
        .iter()
        .filter(|path| !path.ends_with(".expected.json"))
        .collect();
    let expectations: BTreeSet<_> = paths
        .iter()
        .filter(|path| path.ends_with(".expected.json"))
        .map(|path| path.replace(".expected.json", ".json"))
        .collect();
    assert_eq!(bodies.len(), 24);
    assert_eq!(
        bodies.iter().copied().cloned().collect::<BTreeSet<_>>(),
        expectations
    );
    for path in bodies {
        let expected: ExpectedRefusal =
            serde_json::from_value(json(&path.replace(".json", ".expected.json"))).unwrap();
        let error = ConsentRequest::decode(json(path)).expect_err(path);
        assert_eq!(error.code, expected.code, "{path}");
        assert!(
            error.message.contains(&expected.message_contains),
            "{path}: expected {:?}, got {:?}",
            expected.message_contains,
            error.message
        );
    }
}
