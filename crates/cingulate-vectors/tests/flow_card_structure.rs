#[path = "support/flow_card_structure.rs"]
mod flow_card_structure;
#[allow(dead_code)]
#[path = "support/phone.rs"]
mod phone;
mod support;
#[allow(dead_code)]
#[path = "support/watch_pending.rs"]
mod watch_pending;

use std::{collections::BTreeSet, fs};

use cingulate_decode::ConsentRequest;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use flow_card_structure::{fixture, input, requester, CARDS, CORPUS, NOW};
use support::corpus;

const FIELDS: [&str; 5] = [
    "warnings",
    "comparison",
    "capabilities",
    "explanation",
    "changes",
];

#[tokio::test]
async fn flow_card_structure_regenerates_byte_for_byte() {
    let files = flow_card_structure::generate().await;
    let root = corpus(CORPUS);
    let actual: BTreeSet<_> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(files.len(), 16);
    assert_eq!(
        actual,
        files.keys().cloned().collect(),
        "corpus file coverage"
    );
    for (name, bytes) in files {
        assert_eq!(fs::read(root.join(&name)).unwrap(), bytes, "{name}");
    }
}

#[test]
fn flow_card_structure_manifest_is_pinned_and_covers_exact_bytes() {
    let root = corpus(CORPUS);
    let manifest = fs::read(root.join("SHA256SUMS")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&manifest)),
        "3ee8232d37fe49bea0a3e0baccb179453344f949c5fae5f3e5d119bce3f72d33"
    );
    let mut listed = BTreeSet::new();
    for line in std::str::from_utf8(&manifest).unwrap().lines() {
        let (digest, name) = line.split_once("  ").unwrap();
        assert!(!name.contains('/') && name != "SHA256SUMS");
        assert!(listed.insert(name.to_owned()), "duplicate {name}");
        assert_eq!(
            format!("{:x}", Sha256::digest(fs::read(root.join(name)).unwrap())),
            digest,
            "{name}"
        );
    }
    let actual: BTreeSet<_> = fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name != "SHA256SUMS")
        .collect();
    assert_eq!(actual, listed);
}

#[tokio::test]
async fn flow_card_structure_survives_storage_and_all_served_projections() {
    let files = flow_card_structure::generate().await;
    for card in CARDS {
        let body = input(card);
        let fixture = fixture().await;
        let id = fixture.request(&requester(), body.clone()).await;
        // Read record_json back from SQLite, not the in-memory decoded request.
        let stored = serde_json::to_value(fixture.stored_record(&id).request).unwrap();
        for field in FIELDS {
            assert_eq!(stored.get(field), body.get(field), "stored {card} {field}");
        }
        for suffix in [
            "requester-get",
            "phone-get",
            "phone-list",
            "phone-watch",
            "desktop-get",
            "desktop-list",
        ] {
            let value: Value =
                serde_json::from_slice(&files[&format!("{card}-{suffix}.json")]).unwrap();
            let record = if suffix.ends_with("list") {
                &value["result"]["records"][0]
            } else if suffix.ends_with("watch") {
                &value["card"]
            } else {
                &value["result"]
            };
            for field in FIELDS {
                assert_eq!(
                    record.get(field),
                    body.get(field),
                    "{card} {suffix} {field}"
                );
            }
            if suffix.ends_with("list") {
                assert!(record["package"].get("script").is_none());
                assert!(record["package"].get("manifest_json").is_none());
            }
        }
    }
}

#[tokio::test]
async fn flow_card_structure_identical_content_refreshes_and_changed_content_replaces() {
    for field in ["warnings", "comparison", "capabilities"] {
        let fixture = fixture().await;
        let mut body = input("update");
        body["capabilities"] = input("install")["capabilities"].clone();
        let id = fixture.request(&requester(), body.clone()).await;
        fixture.clock.set(NOW + 1);
        let mut refreshed = body.clone();
        refreshed["expires_in_ms"] = json!(60_000);
        let refresh_id = fixture.request(&requester(), refreshed).await;
        assert_eq!(id, refresh_id, "{field}");
        assert_eq!(fixture.stored_record(&id).expires_at_ms, Some(NOW + 60_001));
        match field {
            "warnings" => body[field][0] = json!("The backup timer still runs."),
            "comparison" => body[field]["rows"][0]["after"] = json!("Every 12 hours"),
            "capabilities" => body[field]["rows"][0]["value"] = json!("Every 12 hours"),
            _ => unreachable!(),
        }
        let replacement = fixture.request(&requester(), body).await;
        assert_ne!(replacement, id, "changed {field}");
        assert_eq!(
            fixture.stored_record(&id).state,
            cingulate_store::State::Withdrawn
        );
        assert_eq!(
            fixture.stored_record(&replacement).state,
            cingulate_store::State::Pending
        );
    }
}

#[test]
fn flow_card_structure_samples_pin_the_real_flow_and_layout() {
    let update = input("update");
    let install = input("install");
    for body in [&update, &install] {
        ConsentRequest::decode(body.clone()).unwrap();
        assert_eq!(body["package"]["package"], "module-self-diagnosis");
        assert_eq!(body["package"]["version"], 3);
        assert_eq!(
            body["package"]["code_hash"],
            "02d1abc2a5f58864fc2399a00cacc0685e3c7bbc29ab14daa1141cd62bc70877"
        );
        assert!(body.get("preview").is_none());
    }
    assert_eq!(update["package"], install["package"]);
    assert_eq!(
        update["prompt"],
        "Update module self-diagnosis to version 3?"
    );
    assert_eq!(update["title"], "Flow update");
    assert_eq!(update["warnings"].as_array().unwrap().len(), 1);
    assert_eq!(update["comparison"]["rows"].as_array().unwrap().len(), 5);
    assert_eq!(
        update["comparison"]["rows"][4],
        json!({"label":"Script", "change":"changed", "after":"New version. Source shows it."})
    );
    let manifest: Value =
        serde_json::from_str(update["package"]["manifest_json"].as_str().unwrap()).unwrap();
    assert_eq!(update["changes"]["text"], manifest["changes"]);
    assert_eq!(install["prompt"], "Install module self-diagnosis?");
    assert_eq!(install["title"], "New flow");
    assert!(install.get("comparison").is_none() && install.get("changes").is_none());
    let labels: Vec<_> = install["capabilities"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["label"].as_str().unwrap())
        .collect();
    assert_eq!(
        labels,
        [
            "Runs",
            "Can read",
            "Can change",
            "Wakes",
            "Can be set up for"
        ]
    );
}
