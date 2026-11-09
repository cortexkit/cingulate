mod support;

use serde_json::{json, Value};
use std::collections::BTreeSet;

fn canonical_claim(claim: &Value) -> Option<Value> {
    let fields: &[&str] = match claim["kind"].as_str()? {
        "agent" => &["kind", "agent_id"],
        "worker" => &["kind", "owner_module_id", "worker_ref"],
        "flow" => &[
            "kind",
            "flow_id",
            "flow_version",
            "flow_install_id",
            "flow_code_hash",
        ],
        _ => return None,
    };
    let mut identity = serde_json::Map::new();
    for field in fields {
        identity.insert((*field).into(), claim.get(*field)?.clone());
    }
    Some(Value::Object(identity))
}

// Test-only reference for already authenticated federated calls and rows of one
// subject with a verified live base scope. Names and worker attribution are not
// canonical claim identity; a run has no grantable identity at all.
fn standing_answer(rows: &[Value], call: &Value) -> &'static str {
    let claim = call.get("claim");
    let identity = claim.and_then(canonical_claim);
    if claim.is_some() && identity.is_none() {
        return "none";
    }
    let now = call["now_ms"].as_i64().unwrap();
    let matching: Vec<_> = rows
        .iter()
        .filter(|row| {
            row["pairing_id"] == call["pairing_id"]
                && row["peer_class"] == call["peer_class"]
                && row
                    .get("expires_at_ms")
                    .is_none_or(|expiry| now < expiry.as_i64().unwrap())
                && match row["selector"].as_str() {
                    Some("exact_claim") => {
                        identity.is_some() && row.get("claim").and_then(canonical_claim) == identity
                    }
                    Some("claimless") => row.get("claim").is_none() && claim.is_none(),
                    Some("all_claims") => row.get("claim").is_none(),
                    _ => false,
                }
        })
        .collect();

    // Step 1: exact_claim and claimless matches are specific; all_claims is broad.
    let specific: Vec<_> = matching
        .iter()
        .copied()
        .filter(|row| row["selector"] != "all_claims")
        .collect();
    // Step 2: any specific matches decide, ignoring broad rows entirely.
    if !specific.is_empty() {
        return if specific.iter().any(|row| row["effect"] == "deny") {
            "refuse"
        } else {
            "admit"
        };
    }
    // Step 3: otherwise the broad rows decide, with denial winning a tie.
    if !matching.is_empty() {
        return if matching.iter().any(|row| row["effect"] == "deny") {
            "refuse"
        } else {
            "admit"
        };
    }
    // Step 4: no matching row means no standing answer, not a grant.
    "none"
}

#[test]
fn federated_precedence_table_matches_reference_evaluator() {
    let table = support::vector("permissions-v1", "federated-precedence.json");
    let cases = table["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 30);
    let mut names = BTreeSet::new();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        assert!(names.insert(name), "duplicate case {name}");
        assert_eq!(
            standing_answer(case["rows"].as_array().unwrap(), &case["call"]),
            case["expected"].as_str().unwrap(),
            "{name}"
        );
    }
}

#[test]
fn provider_listing_contains_the_contract_denial_and_a_federated_grant() {
    let listing = support::vector("permissions-v1", "provider-list-federated-deny.json");
    let rows = listing["reply"]["grants"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    let denial = rows.iter().find(|row| row["effect"] == "deny").unwrap();
    assert_eq!(
        denial,
        &json!({"id":"grt_09","effect":"deny",
            "holder":{"kind":"federated","peer_fingerprint":"a".repeat(32),
                "pairing_id":"pairing-invented-1","peer_class":"computer",
                "selector":"exact_claim","claim":{"kind":"agent","agent_id":"Sage"},
                "label":"Juniper · Sage"},
            "module":"plexus","subject_label":"Mail","scope":"mail_read",
            "scope_label":"never read your mail","granted_at_ms":1000000,"card_ref":"el_9"})
    );
    let grant = rows.iter().find(|row| row.get("effect").is_none()).unwrap();
    assert_eq!(grant["holder"]["kind"], "federated");
    assert_eq!(
        grant["holder"]["pairing_id"],
        denial["holder"]["pairing_id"]
    );
    assert_eq!(grant["holder"]["selector"], "all_claims");
    for row in rows {
        assert!(row.get("agent_id").is_none());
        assert!(row["holder"].get("agent_id").is_none());
        assert!(row.get("group").is_none());
    }
}
