#[path = "support/flow_decision_phone.rs"]
mod flow_decision_phone;
#[allow(dead_code)]
#[path = "support/phone.rs"]
mod phone;
mod support;

use std::{collections::BTreeSet, fs, path::Path};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use support::corpus;

const FILES: [&str; 12] = [
    "README.md",
    "SHA256SUMS",
    "decision-grant-lost-phone-get.json",
    "decision-grant-lost-request-reply.json",
    "decision-grant-lost-request.json",
    "decision-reconcile-phone-get.json",
    "decision-reconcile-request-reply.json",
    "decision-reconcile-request.json",
    "decision-reenable-phone-get.json",
    "decision-reenable-request-reply.json",
    "decision-reenable-request.json",
    "list-pending.json",
];

struct PinnedOption {
    id: &'static str,
    label: &'static str,
    effect: &'static str,
}

struct PinnedDecision {
    kind: &'static str,
    id: &'static str,
    default: &'static str,
    input_file: &'static str,
    options: &'static [PinnedOption],
}

const RECONCILE_OPTIONS: &[PinnedOption] = &[
    PinnedOption {
        id: "not_applied",
        label: "Send the call again",
        effect: "choose",
    },
    PinnedOption {
        id: "applied",
        label: "Continue as applied",
        effect: "choose",
    },
    PinnedOption {
        id: "cancel",
        label: "Cancel the run",
        effect: "choose",
    },
    PinnedOption {
        id: "leave",
        label: "Leave it unresolved",
        effect: "decline",
    },
];
const REENABLE_OPTIONS: &[PinnedOption] = &[
    PinnedOption {
        id: "reenable",
        label: "Re-enable the flow",
        effect: "choose",
    },
    PinnedOption {
        id: "keep",
        label: "Keep disabled",
        effect: "decline",
    },
];
const GRANT_LOST_OPTIONS: &[PinnedOption] = &[
    PinnedOption {
        id: "check_now",
        label: "Check now",
        effect: "choose",
    },
    PinnedOption {
        id: "keep_disabled",
        label: "Keep disabled",
        effect: "decline",
    },
];
const DECISIONS: &[PinnedDecision] = &[
    PinnedDecision {
        kind: "reconcile",
        id: "el_1",
        default: "leave",
        input_file: "decision-reconcile-request.json",
        options: RECONCILE_OPTIONS,
    },
    PinnedDecision {
        kind: "reenable",
        id: "el_2",
        default: "keep",
        input_file: "decision-reenable-request.json",
        options: REENABLE_OPTIONS,
    },
    PinnedDecision {
        kind: "grant-lost",
        id: "el_3",
        default: "keep_disabled",
        input_file: "decision-grant-lost-request.json",
        options: GRANT_LOST_OPTIONS,
    },
];

fn read_json(name: &str) -> Value {
    serde_json::from_slice(&fs::read(corpus(flow_decision_phone::CORPUS).join(name)).unwrap())
        .unwrap()
}

fn assert_phone_card(card: &Value, decision: &PinnedDecision) {
    assert_eq!(card["elicitationId"], decision.id);
    assert_eq!(card["kind"], "flow_decision");
    assert_eq!(card["requesterLabel"], "reserved:basal");
    assert_eq!(card["state"], "pending");
    assert_eq!(card["default"], decision.default);
    let expected_options: Vec<_> = decision
        .options
        .iter()
        .map(|option| {
            json!({
                "id": option.id,
                "label": option.label,
                "effect": option.effect,
            })
        })
        .collect();
    assert_eq!(card["options"], json!(expected_options));

    let request = read_json(&format!("decision-{}-request.json", decision.kind));
    assert_eq!(card["title"], request["title"]);
    assert_eq!(card["prompt"], request["prompt"]);
}

#[tokio::test]
async fn flow_decision_phone_vectors_regenerate_byte_for_byte() {
    let generated = flow_decision_phone::generate().await;
    let expected: BTreeSet<_> = FILES
        .iter()
        .filter(|name| **name != "README.md" && **name != "SHA256SUMS")
        .map(|name| name.to_string())
        .collect();
    assert_eq!(generated.keys().cloned().collect::<BTreeSet<_>>(), expected);
    for (name, bytes) in generated {
        assert_eq!(
            fs::read(corpus(flow_decision_phone::CORPUS).join(&name)).unwrap(),
            bytes,
            "served vector differs: {name}"
        );
    }
    for decision in DECISIONS {
        let name = format!("decision-{}-request.json", decision.kind);
        let input = fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/inputs/flow-decision-phone-v1")
                .join(decision.input_file),
        )
        .unwrap();
        assert_eq!(
            fs::read(corpus(flow_decision_phone::CORPUS).join(&name)).unwrap(),
            input,
            "input bytes differ: {name}"
        );
    }
}

#[test]
fn flow_decision_phone_sha256sums_cover_every_file() {
    let root = corpus(flow_decision_phone::CORPUS);
    let sums = fs::read(root.join("SHA256SUMS")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&sums)),
        "04ff370ecb565a6647ee560f8707e7a40244516d960b7d8d0d73e730975819fb"
    );
    let mut seen = BTreeSet::new();
    for line in std::str::from_utf8(&sums).unwrap().lines() {
        let (hash, name) = line.split_once("  ").unwrap();
        assert!(seen.insert(name.to_owned()), "duplicate hash entry: {name}");
        assert_eq!(
            format!("{:x}", Sha256::digest(fs::read(root.join(name)).unwrap())),
            hash,
            "SHA256SUMS: {name}"
        );
    }
    assert_eq!(
        seen,
        FILES
            .iter()
            .filter(|name| **name != "SHA256SUMS")
            .map(|name| name.to_string())
            .collect()
    );
    let actual = fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, FILES.iter().map(|name| name.to_string()).collect());
}

#[test]
fn phone_serves_pinned_decision_cards() {
    for decision in DECISIONS {
        let phone = read_json(&format!("decision-{}-phone-get.json", decision.kind));
        assert_phone_card(&phone["result"], decision);
    }

    let list = read_json("list-pending.json");
    let records = list["result"]["records"].as_array().unwrap();
    assert_eq!(records.len(), DECISIONS.len());
    for decision in DECISIONS {
        let record = records
            .iter()
            .find(|record| record["elicitationId"] == decision.id)
            .unwrap_or_else(|| panic!("missing {} from phone list", decision.id));
        assert_phone_card(record, decision);
    }
}
