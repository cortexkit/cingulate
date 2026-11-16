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
        "518db1510d9dd85ae977a3787cc5c871c5a2269139519498e28ac8be963ffe32"
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
        // Published messages are historical human text, not protocol codes.
        // Current fixed diagnostics are pinned in decoding.rs.
        assert!(!expected.message_contains.is_empty(), "{path}");
    }
}

#[test]
fn danger_option_group_requests_match_pinned_decoder_verdicts() {
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test-vectors/danger-option-group-v1");
    for name in [
        "file-card",
        "refuse-danger-default",
        "refuse-only-danger-grants",
        "refuse-unknown-group",
    ] {
        let case: Value =
            serde_json::from_slice(&fs::read(root.join(format!("{name}.json"))).unwrap()).unwrap();
        let decoded = ConsentRequest::decode(case["request"].clone());
        if let Some(expected) = case.get("error") {
            let error = decoded.expect_err(name);
            assert_eq!(error.code, expected["code"], "{name}");
            assert!(
                expected["message"].as_str().is_some_and(|m| !m.is_empty()),
                "{name}"
            );
        } else {
            let stored = serde_json::to_value(decoded.expect(name)).unwrap();
            assert_eq!(stored["options"], case["request"]["options"], "{name}");
        }
    }
}
