#[path = "support/cerebellum_agent_permission.rs"]
mod cerebellum_agent_permission;
#[allow(dead_code)]
#[path = "support/phone.rs"]
mod phone;
mod support;

use std::{collections::BTreeSet, fs};

use serde_json::Value;
use sha2::{Digest, Sha256};

const FILES: [&str; 16] = [
    "README.md",
    "SHA256SUMS",
    "head-app-control-phone-get.json",
    "head-app-control-request-reply.json",
    "head-takeover-phone-get.json",
    "head-takeover-request-reply.json",
    "head-website-public-in-call-phone-get.json",
    "head-website-public-in-call-request-reply.json",
    "list-pending.json",
    "requests/SHA256SUMS",
    "requests/head-app-control.json",
    "requests/head-takeover.json",
    "requests/head-website-public-in-call.json",
    "requests/worker-app-control.json",
    "worker-app-control-phone-get.json",
    "worker-app-control-request-reply.json",
];

const INPUT_HASHES: [(&str, &str); 4] = [
    (
        "head-app-control.json",
        "84df446112d6b074f8ea461daf2edf1e5d45259458d8236a54a8ad678ab6fda0",
    ),
    (
        "head-takeover.json",
        "5616b71c77e144b2cab15eeba1ed9af57ce7cbd8132b4bea824b0d268757a659",
    ),
    (
        "head-website-public-in-call.json",
        "02ca35f0e8d3eb3bddefe130b7a466bb99aaf1f4290d261205e5597eda646972",
    ),
    (
        "worker-app-control.json",
        "2492e1759ff688643306eb87e0d9c3c7fd37076db566570900b39a62a7e34930",
    ),
];

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn read_json(name: &str) -> Value {
    serde_json::from_slice(&fs::read(cerebellum_agent_permission::corpus().join(name)).unwrap())
        .unwrap()
}

async fn regenerate() -> std::collections::BTreeMap<String, Vec<u8>> {
    let root = cerebellum_agent_permission::corpus();
    let mut files = cerebellum_agent_permission::generate().await;
    files.insert(
        "README.md".into(),
        fs::read(root.join("README.md")).unwrap(),
    );
    let sums: String = files
        .iter()
        .map(|(name, bytes)| format!("{}  {name}\n", sha256(bytes)))
        .collect();
    files.insert("SHA256SUMS".into(), sums.into_bytes());
    files
}

#[tokio::test]
async fn cerebellum_agent_permission_vectors_regenerate_byte_for_byte() {
    let generated = regenerate().await;
    assert_eq!(
        generated
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        FILES.iter().copied().collect::<BTreeSet<_>>()
    );
    for (name, bytes) in generated {
        assert_eq!(
            fs::read(cerebellum_agent_permission::corpus().join(&name)).unwrap(),
            bytes,
            "served vector differs: {name}"
        );
    }
}

#[test]
fn cerebellum_agent_permission_request_inputs_are_pinned_byte_for_byte() {
    let requests = cerebellum_agent_permission::corpus().join("requests");
    for (name, expected) in INPUT_HASHES {
        assert_eq!(
            sha256(&fs::read(requests.join(name)).unwrap()),
            expected,
            "Cerebellum consent.request input changed: {name}"
        );
    }
}

fn assert_phone_card(card: &Value, id: &str, via_worker: bool) {
    assert_eq!(card["elicitationId"], id);
    assert_eq!(card["state"], "pending");
    assert_eq!(card["requesterName"], "Cerebellum");
    assert_eq!(card["subject"]["agentId"], "agent-alf");
    assert_eq!(card["subject"]["headAgentId"], "agent-alf");
    assert_eq!(card["subject"]["label"], "ALF");
    assert_eq!(
        card["subject"]["viaWorker"].as_bool().unwrap_or(false),
        via_worker
    );
    assert!(card["options"]
        .as_array()
        .unwrap()
        .iter()
        .any(|option| option["group"] == "danger"));
}

#[test]
fn phone_projections_keep_subject_requester_and_danger_options() {
    for (index, kind) in [
        "head-app-control",
        "head-takeover",
        "head-website-public-in-call",
        "worker-app-control",
    ]
    .into_iter()
    .enumerate()
    {
        let id = format!("el_{}", index + 1);
        let get = read_json(&format!("{kind}-phone-get.json"));
        assert_phone_card(&get["result"], &id, kind == "worker-app-control");
    }

    let records = read_json("list-pending.json")["result"]["records"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(records.len(), 4);
    for (index, kind) in [
        "head-app-control",
        "head-takeover",
        "head-website-public-in-call",
        "worker-app-control",
    ]
    .into_iter()
    .enumerate()
    {
        let id = format!("el_{}", index + 1);
        let card = records
            .iter()
            .find(|card| card["elicitationId"] == id)
            .unwrap_or_else(|| panic!("missing {id} in phone list"));
        assert_phone_card(card, &id, kind == "worker-app-control");
    }
}
