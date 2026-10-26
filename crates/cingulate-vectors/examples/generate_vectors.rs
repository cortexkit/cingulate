//! Writes the contract corpora `danger-option-group-v1` and `permissions-v1`.
//!
//! Original cases are written from the contracts in `docs/contracts/` and their
//! bytes remain pinned. Additive `served-*` permissions cases are captured from
//! the real aggregator handler with fake catalog entries and providers.
//!
//! Run from the workspace root:
//!
//! ```sh
//! # Write the vectors.
//! cargo run -p cingulate-vectors --example generate_vectors --locked
//! # Check without writing.
//! cargo run -p cingulate-vectors --example generate_vectors --locked -- --check
//! ```
//!
//! Without arguments it rewrites every case triple and each corpus's
//! `SHA256SUMS`. With `--check` it writes nothing and exits 1 when any file on
//! disk differs from what it would write, or when a corpus holds a file it would
//! not write. Each corpus `README.md` is hand-written; the generator only hashes
//! it into `SHA256SUMS`.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

#[path = "../tests/support/permissions.rs"]
mod served_permissions;

const DANGER: &str = "danger-option-group-v1";
const PERMISSIONS: &str = "permissions-v1";

/// Fixed clock: the moment the grants below were minted.
const NOW_MS: i64 = 1_000_000;
const HALF_HOUR_MS: i64 = 1_800_000;

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// RFC 8785 for this corpus's value domain. serde_json's default map is a
/// BTreeMap, so keys serialize sorted; with ASCII strings and integers only,
/// compact serde_json output is the canonical form. `assert_domain` refuses
/// anything outside that domain instead of emitting non-canonical bytes.
fn jcs(value: &Value) -> String {
    assert_domain(value);
    serde_json::to_string(value).expect("serialize")
}

fn assert_domain(value: &Value) {
    match value {
        Value::String(text) => assert!(text.is_ascii(), "non-ASCII string {text:?}"),
        Value::Number(number) => assert!(
            number.is_i64() || number.is_u64(),
            "non-integer number {number}"
        ),
        Value::Array(items) => items.iter().for_each(assert_domain),
        Value::Object(map) => map.iter().for_each(|(key, value)| {
            assert!(key.is_ascii(), "non-ASCII key {key:?}");
            assert_domain(value);
        }),
        Value::Null | Value::Bool(_) => {}
    }
}

fn case(tag: &str, method: &str, route: Value, request: Value, outcome: (&str, Value)) -> Value {
    let mut value = json!({"tag": tag, "method": method, "route": route, "request": request});
    value[outcome.0] = outcome.1;
    value
}

fn reply(value: Value) -> (&'static str, Value) {
    ("reply", value)
}

fn error(code: &str, message: &str) -> (&'static str, Value) {
    ("error", json!({"code": code, "message": message}))
}

// ---------------------------------------------------------------------------
// danger-option-group-v1

fn cerebellum_route() -> Value {
    json!({"principal": "reserved:cerebellum", "carried_agent_id": "agent-alf"})
}

fn phone_route() -> Value {
    json!({"principal": "reserved:callosum", "operator_surface": true})
}

fn takeover_detail() -> &'static str {
    "Includes taking over your mouse and keyboard."
}

fn app_card_options() -> Value {
    json!([
        {"id": "app_30m", "label": "Use TextEdit for 30 minutes", "effect": "grant"},
        {"id": "app_always", "label": "Always allow TextEdit", "effect": "grant"},
        {"id": "deny", "label": "Don't allow", "effect": "decline"},
        {"id": "computer_30m", "label": "Use the whole computer for 30 minutes",
         "effect": "grant", "group": "danger", "detail": takeover_detail()},
        {"id": "computer_always", "label": "Always allow the whole computer",
         "effect": "grant", "group": "danger", "detail": takeover_detail()}
    ])
}

fn app_card(options: Value) -> Value {
    json!({
        "kind": "consent",
        "title": "Allow TextEdit",
        "prompt": "ALF wants to use TextEdit.",
        "options": options,
        "default": "deny",
        "urgency": "normal",
        "on_expiry": "deny",
        "material_damage": true,
        "scope": {"app": "com.apple.TextEdit"},
        "late_execution": "execute",
        "target": {"kind": "app", "label": "TextEdit"},
        "expires_in_ms": 60_000
    })
}

fn strip_danger(options: &Value) -> Value {
    Value::Array(
        options
            .as_array()
            .expect("options")
            .iter()
            .filter(|option| option.get("group").is_none())
            .cloned()
            .collect(),
    )
}

/// Expected phone projection of ALF's TextEdit access card.
/// `requesterName` is the admitted requester's human-readable display name.
fn phone_record(options: Value) -> Value {
    json!({
        "elicitationId": "el_1",
        "createdAtMs": NOW_MS,
        "expiresAtMs": NOW_MS + 60_000,
        "subject": {"agentId": "agent-alf", "headAgentId": "agent-alf", "label": "ALF"},
        "state": "pending",
        "kind": "consent",
        "title": "Allow TextEdit",
        "prompt": "ALF wants to use TextEdit.",
        "options": options,
        "default": "deny",
        "urgency": "normal",
        "onExpiry": "deny",
        "materialDamage": true,
        "scope": {"app": "com.apple.TextEdit"},
        "lateExecution": "execute",
        "target": {"kind": "app", "label": "TextEdit"},
        "expiresInMs": 60_000,
        "facts": [],
        "requesterLabel": "reserved:cerebellum",
        "requesterName": "Cerebellum"
    })
}

fn danger_cases() -> Vec<(&'static str, Value)> {
    let declared = json!(["option-group"]);
    let full = app_card_options();
    let stripped = strip_danger(&full);

    let mut danger_default = app_card(app_card_options());
    danger_default["options"][2]["group"] = json!("danger");

    let only_danger = app_card(json!([
        {"id": "deny", "label": "Don't allow", "effect": "decline"},
        {"id": "computer_30m", "label": "Use the whole computer for 30 minutes",
         "effect": "grant", "group": "danger", "detail": takeover_detail()}
    ]));

    let mut unknown_group = app_card(app_card_options());
    unknown_group["options"][3]["group"] = json!("caution");

    let mut answered = phone_record(full.clone());
    answered["state"] = json!("answered");
    answered["answeredChoiceId"] = json!("computer_30m");
    answered["answeredAtMs"] = json!(NOW_MS + 5_000);

    vec![
        (
            "file-card",
            case(
                DANGER,
                "consent.request",
                cerebellum_route(),
                app_card(full.clone()),
                reply(json!({"elicitation_id": "el_1"})),
            ),
        ),
        (
            "list-pending-declared",
            case(
                DANGER,
                "consent.list_pending",
                phone_route(),
                json!({"clientFeatures": declared}),
                reply(json!({"records": [phone_record(full.clone())]})),
            ),
        ),
        (
            "list-pending-undeclared",
            case(
                DANGER,
                "consent.list_pending",
                phone_route(),
                json!({}),
                reply(json!({"records": [phone_record(stripped.clone())]})),
            ),
        ),
        (
            "get-declared",
            case(
                DANGER,
                "consent.get",
                phone_route(),
                json!({"elicitationId": "el_1", "clientFeatures": declared}),
                reply(phone_record(full.clone())),
            ),
        ),
        (
            "get-undeclared",
            case(
                DANGER,
                "consent.get",
                phone_route(),
                json!({"elicitationId": "el_1"}),
                reply(phone_record(stripped)),
            ),
        ),
        (
            "refuse-danger-default",
            case(
                DANGER,
                "consent.request",
                cerebellum_route(),
                danger_default,
                error(
                    "elicitation_fail_closed_required",
                    "the default option cannot be in the danger group",
                ),
            ),
        ),
        (
            "refuse-only-danger-grants",
            case(
                DANGER,
                "consent.request",
                cerebellum_route(),
                only_danger,
                error(
                    "elicitation_invalid_request",
                    "at least one grant option must be outside the danger group",
                ),
            ),
        ),
        (
            "refuse-unknown-group",
            case(
                DANGER,
                "consent.request",
                cerebellum_route(),
                unknown_group,
                error(
                    "elicitation_invalid_request",
                    "unknown variant `caution`, expected `danger`",
                ),
            ),
        ),
        (
            "answer-danger-undeclared",
            case(
                DANGER,
                "consent.answer",
                phone_route(),
                json!({"elicitationId": "el_1", "choiceId": "computer_30m"}),
                error(
                    "elicitation_option_group_required",
                    "choice computer_30m is in the danger option group; declare clientFeatures option-group",
                ),
            ),
        ),
        (
            "answer-danger-declared",
            case(
                DANGER,
                "consent.answer",
                phone_route(),
                json!({"elicitationId": "el_1", "choiceId": "computer_30m",
                       "clientFeatures": declared}),
                reply(json!({"ok": true, "alreadyAnswered": false, "elicitation": answered})),
            ),
        ),
    ]
}

// ---------------------------------------------------------------------------
// permissions-v1

fn agent_holder() -> Value {
    json!({"kind": "agent", "agent_id": "agent-alf"})
}

fn grant_app() -> Value {
    json!({
        "id": "grt_01", "agent_id": "agent-alf", "holder": agent_holder(),
        "module": "cerebellum", "subject_label": "TextEdit",
        "scope": "app", "scope_label": "the app TextEdit",
        "expires_at_ms": NOW_MS + HALF_HOUR_MS, "granted_at_ms": NOW_MS,
        "card_ref": "el_1"
    })
}

fn grant_whole_computer() -> Value {
    json!({
        "id": "grt_02", "agent_id": "agent-alf", "holder": agent_holder(),
        "module": "cerebellum", "subject_label": "Whole computer",
        "scope": "whole_computer",
        "scope_label": "the whole computer, including your mouse and keyboard",
        "group": "danger", "granted_at_ms": NOW_MS, "card_ref": "el_2"
    })
}

fn grant_site() -> Value {
    json!({
        "id": "grt_03", "agent_id": "agent-alf", "holder": agent_holder(),
        "module": "cerebellum", "subject_label": "github.com",
        "scope": "site", "scope_label": "the site github.com in the agent browser",
        "granted_at_ms": NOW_MS, "card_ref": "el_3"
    })
}

fn grant_all_sites() -> Value {
    json!({
        "id": "grt_04", "agent_id": "agent-alf", "holder": agent_holder(),
        "module": "cerebellum", "subject_label": "All sites",
        "scope": "all_sites", "scope_label": "every site in the agent browser",
        "group": "danger", "expires_at_ms": NOW_MS + HALF_HOUR_MS,
        "granted_at_ms": NOW_MS, "card_ref": "el_4"
    })
}

fn grant_worker() -> Value {
    json!({
        "id": "grt_05", "agent_id": "agent-alf",
        "holder": {"kind": "worker", "agent_id": "agent-alf", "scope_ref": "scope:wkr-7f3a"},
        "module": "cerebellum", "subject_label": "Preview",
        "scope": "app", "scope_label": "the app Preview",
        "expires_at_ms": NOW_MS + HALF_HOUR_MS, "granted_at_ms": NOW_MS,
        "card_ref": "el_5"
    })
}

fn live_grants() -> Value {
    json!([
        grant_app(),
        grant_whole_computer(),
        grant_site(),
        grant_all_sites(),
        grant_worker()
    ])
}

/// A grant held by an installed flow version: no agent id, no expiry.
fn grant_flow_method() -> Value {
    json!({
        "id": "grt_06",
        "holder": {"kind": "flow", "flow_id": "flow_web_index", "flow_version": 3,
                   "flow_install_id": "flowinst_9c41d2"},
        "module": "claustrum", "subject_label": "Search API",
        "scope": "api_key",
        "scope_label": "the search API key, for reads only",
        "methods": ["GET"], "granted_at_ms": NOW_MS, "card_ref": "el_7"
    })
}

fn grant_flow_site() -> Value {
    json!({
        "id": "grt_07",
        "holder": {"kind": "flow", "flow_id": "flow_web_index", "flow_version": 4,
                   "flow_install_id": "flowinst_1b77e0"},
        "module": "claustrum", "subject_label": "news.example",
        "scope": "site_login", "scope_label": "your news.example login",
        "granted_at_ms": NOW_MS, "card_ref": "el_8"
    })
}

/// A holder kind this contract does not define, which a reader must keep.
fn grant_unknown_holder() -> Value {
    json!({
        "id": "grt_08",
        "holder": {"kind": "schedule", "schedule_ref": "sched_nightly"},
        "module": "claustrum", "subject_label": "Archive",
        "scope": "api_key", "scope_label": "the archive API key",
        "granted_at_ms": NOW_MS
    })
}

fn provider_rows() -> Value {
    json!([
        grant_app(),
        grant_whole_computer(),
        grant_site(),
        grant_all_sites(),
        grant_worker(),
        grant_flow_method(),
        grant_flow_site(),
        grant_unknown_holder()
    ])
}

fn cingulate_route() -> Value {
    json!({"principal": "reserved:cingulate"})
}

fn with_now(mut value: Value, now_ms: i64) -> Value {
    value["now_ms"] = json!(now_ms);
    value
}

/// The aggregator's `version` is the lowercase hex SHA-256 of the RFC 8785
/// bytes of the reply's `modules` array.
fn list_all(modules: Value) -> Value {
    json!({"version": sha256(jcs(&modules).as_bytes()), "modules": modules})
}

fn cerebellum_ok() -> Value {
    json!({"module": "cerebellum", "status": "ok", "grants": live_grants()})
}

fn permissions_cases() -> Vec<(&'static str, Value)> {
    let listed_at = NOW_MS + 60_000;
    let expired_at = NOW_MS + HALF_HOUR_MS;
    let mut upgraded = grant_app();
    upgraded.as_object_mut().unwrap().remove("expires_at_ms");
    upgraded["granted_at_ms"] = json!(NOW_MS + 120_000);
    upgraded["card_ref"] = json!("el_6");
    let mut upgraded_grants = live_grants();
    upgraded_grants[0] = upgraded;

    let ok = list_all(json!([cerebellum_ok()]));
    let ok_version = ok["version"].clone();
    let operator_required = || {
        error(
            "permissions_operator_required",
            "permissions operations need the operator: reserved:callosum on an operator surface, or direct",
        )
    };
    let provider_operator_required = || {
        error(
            "permissions_operator_required",
            "permissions operations need reserved:cingulate or the local operator's direct route",
        )
    };

    vec![
        // Provider side: what a permissions/v1 provider (Cerebellum) serves.
        (
            "provider-list",
            with_now(
                case(
                    PERMISSIONS,
                    "permissions.list",
                    cingulate_route(),
                    json!({}),
                    reply(json!({"grants": provider_rows()})),
                ),
                listed_at,
            ),
        ),
        (
            "provider-list-after-upgrade",
            with_now(
                case(
                    PERMISSIONS,
                    "permissions.list",
                    cingulate_route(),
                    json!({"agent_id": "agent-alf"}),
                    reply(json!({"grants": upgraded_grants})),
                ),
                NOW_MS + 120_000,
            ),
        ),
        (
            "provider-list-after-expiry",
            with_now(
                case(
                    PERMISSIONS,
                    "permissions.list",
                    cingulate_route(),
                    json!({}),
                    reply(json!({"grants": [
                        grant_whole_computer(),
                        grant_site(),
                        grant_flow_method(),
                        grant_flow_site(),
                        grant_unknown_holder()
                    ]})),
                ),
                expired_at,
            ),
        ),
        (
            "provider-revoke-revoked",
            with_now(
                case(
                    PERMISSIONS,
                    "permissions.revoke",
                    cingulate_route(),
                    json!({"id": "grt_03"}),
                    reply(json!({"outcome": "revoked"})),
                ),
                listed_at,
            ),
        ),
        (
            "provider-revoke-stopping",
            with_now(
                case(
                    PERMISSIONS,
                    "permissions.revoke",
                    cingulate_route(),
                    json!({"id": "grt_02"}),
                    reply(json!({"outcome": "revoked", "stopping": true})),
                ),
                listed_at,
            ),
        ),
        (
            "provider-revoke-not-found",
            with_now(
                case(
                    PERMISSIONS,
                    "permissions.revoke",
                    cingulate_route(),
                    json!({"id": "grt_99"}),
                    reply(json!({"outcome": "not_found"})),
                ),
                listed_at,
            ),
        ),
        (
            "provider-revoke-expired",
            with_now(
                case(
                    PERMISSIONS,
                    "permissions.revoke",
                    cingulate_route(),
                    json!({"id": "grt_01"}),
                    reply(json!({"outcome": "not_found"})),
                ),
                expired_at,
            ),
        ),
        (
            "provider-revoke-refused-agent",
            with_now(
                case(
                    PERMISSIONS,
                    "permissions.revoke",
                    json!({"principal": "reserved:plexus", "carried_agent_id": "agent-alf"}),
                    json!({"id": "grt_01"}),
                    provider_operator_required(),
                ),
                listed_at,
            ),
        ),
        // Aggregator side: what cingulate serves to the apps.
        (
            "list-all-ok",
            with_now(
                case(
                    PERMISSIONS,
                    "permissions.list_all",
                    phone_route(),
                    json!({}),
                    reply(ok),
                ),
                listed_at,
            ),
        ),
        (
            "list-all-unreachable",
            with_now(
                case(
                    PERMISSIONS,
                    "permissions.list_all",
                    phone_route(),
                    json!({}),
                    reply(list_all(json!([
                        cerebellum_ok(),
                        {"module": "plexus", "status": "unreachable"}
                    ]))),
                ),
                listed_at,
            ),
        ),
        (
            "list-all-unsupported",
            with_now(
                case(
                    PERMISSIONS,
                    "permissions.list_all",
                    phone_route(),
                    json!({}),
                    reply(list_all(json!([
                        {"module": "aft", "status": "unsupported"},
                        cerebellum_ok()
                    ]))),
                ),
                listed_at,
            ),
        ),
        (
            "list-all-unchanged",
            with_now(
                case(
                    PERMISSIONS,
                    "permissions.list_all",
                    phone_route(),
                    json!({"since_version": ok_version}),
                    reply(json!({"version": ok_version, "unchanged": true})),
                ),
                listed_at,
            ),
        ),
        (
            "list-all-refused",
            with_now(
                case(
                    PERMISSIONS,
                    "permissions.list_all",
                    json!({"principal": "reserved:plexus", "carried_agent_id": "agent-alf"}),
                    json!({}),
                    operator_required(),
                ),
                listed_at,
            ),
        ),
        (
            "list-all-refused-no-operator-surface",
            with_now(
                case(
                    PERMISSIONS,
                    "permissions.list_all",
                    json!({"principal": "reserved:callosum", "operator_surface": false}),
                    json!({}),
                    operator_required(),
                ),
                listed_at,
            ),
        ),
        (
            "revoke-revoked",
            with_now(
                case(
                    PERMISSIONS,
                    "permissions.revoke",
                    phone_route(),
                    json!({"module": "cerebellum", "id": "grt_03"}),
                    reply(json!({"outcome": "revoked"})),
                ),
                listed_at,
            ),
        ),
        (
            "revoke-stopping",
            with_now(
                case(
                    PERMISSIONS,
                    "permissions.revoke",
                    phone_route(),
                    json!({"module": "cerebellum", "id": "grt_02"}),
                    reply(json!({"outcome": "stopping"})),
                ),
                listed_at,
            ),
        ),
        (
            "revoke-already-gone",
            with_now(
                case(
                    PERMISSIONS,
                    "permissions.revoke",
                    phone_route(),
                    json!({"module": "cerebellum", "id": "grt_01"}),
                    reply(json!({"outcome": "already_gone"})),
                ),
                expired_at,
            ),
        ),
        (
            "revoke-module-unreachable",
            with_now(
                case(
                    PERMISSIONS,
                    "permissions.revoke",
                    phone_route(),
                    json!({"module": "plexus", "id": "grt_77"}),
                    reply(json!({"outcome": "module_unreachable"})),
                ),
                listed_at,
            ),
        ),
        (
            "revoke-refused-not-provider",
            with_now(
                case(
                    PERMISSIONS,
                    "permissions.revoke",
                    phone_route(),
                    json!({"module": "broca", "id": "grt_01"}),
                    reply(json!({"outcome": "refused", "code": "not_a_provider"})),
                ),
                listed_at,
            ),
        ),
        (
            "revoke-direct",
            with_now(
                case(
                    PERMISSIONS,
                    "permissions.revoke",
                    json!({"principal": "direct"}),
                    json!({"module": "cerebellum", "id": "grt_03"}),
                    reply(json!({"outcome": "revoked"})),
                ),
                listed_at,
            ),
        ),
        (
            "revoke-refused-not-operator",
            with_now(
                case(
                    PERMISSIONS,
                    "permissions.revoke",
                    json!({"principal": "reserved:plexus", "carried_agent_id": "agent-alf"}),
                    json!({"module": "cerebellum", "id": "grt_03"}),
                    operator_required(),
                ),
                listed_at,
            ),
        ),
        (
            "revoke-refused-no-operator-surface",
            with_now(
                case(
                    PERMISSIONS,
                    "permissions.revoke",
                    json!({"principal": "reserved:callosum", "operator_surface": false}),
                    json!({"module": "cerebellum", "id": "grt_03"}),
                    operator_required(),
                ),
                listed_at,
            ),
        ),
    ]
}

// ---------------------------------------------------------------------------

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test-vectors")
}

/// Every file the generator owns in one corpus, keyed by file name.
fn corpus_files(dir: &Path, cases: Vec<(&'static str, Value)>) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    for (name, value) in cases {
        let canonical = jcs(&value);
        let pretty = serde_json::to_string_pretty(&value).expect("pretty") + "\n";
        let digest = sha256(canonical.as_bytes());
        assert!(
            files
                .insert(format!("{name}.json"), pretty.into_bytes())
                .is_none(),
            "duplicate case {name}"
        );
        files.insert(format!("{name}.jcs"), canonical.into_bytes());
        files.insert(format!("{name}.sha256"), digest.into_bytes());
    }
    // README.md is hand-written; it is hashed from disk, never generated.
    let readme = fs::read(dir.join("README.md")).expect("corpus README.md");
    let mut sums = String::new();
    let mut listed: Vec<(&String, &Vec<u8>)> = files.iter().collect();
    let readme_name = "README.md".to_owned();
    listed.push((&readme_name, &readme));
    listed.sort_by(|a, b| a.0.cmp(b.0));
    for (name, bytes) in listed {
        sums.push_str(&format!("{}  {name}\n", sha256(bytes)));
    }
    files.insert("SHA256SUMS".into(), sums.into_bytes());
    files
}

#[tokio::main]
async fn main() -> ExitCode {
    let check = match std::env::args().nth(1).as_deref() {
        None => false,
        Some("--check") => true,
        Some(other) => {
            eprintln!("unknown argument {other}; use --check or no argument");
            return ExitCode::from(2);
        }
    };
    let mut drift = Vec::new();
    let mut total = 0;
    let mut permissions = permissions_cases();
    permissions.extend(served_permissions::served_cases().await);
    for (corpus, cases) in [(DANGER, danger_cases()), (PERMISSIONS, permissions)] {
        let dir = root().join(corpus);
        let count = cases.len();
        let files = corpus_files(&dir, cases);
        total += files.len();
        if check {
            for (name, bytes) in &files {
                if fs::read(dir.join(name)).ok().as_ref() != Some(bytes) {
                    drift.push(format!("{corpus}/{name}: differs or is missing"));
                }
            }
            let expected: BTreeSet<&str> = files
                .keys()
                .map(String::as_str)
                .chain(["README.md"])
                .collect();
            for entry in fs::read_dir(&dir).expect("corpus directory") {
                let name = entry.expect("entry").file_name().into_string().unwrap();
                if !expected.contains(name.as_str()) {
                    drift.push(format!("{corpus}/{name}: not written by the generator"));
                }
            }
        } else {
            for (name, bytes) in &files {
                fs::write(dir.join(name), bytes).expect("write vector file");
            }
        }
        println!("{corpus}: {count} cases, {} generated files", files.len());
    }
    if !drift.is_empty() {
        for line in &drift {
            eprintln!("{line}");
        }
        eprintln!("{} files out of date; rerun without --check", drift.len());
        return ExitCode::FAILURE;
    }
    println!(
        "{} {total} generated files",
        if check { "checked" } else { "wrote" }
    );
    ExitCode::SUCCESS
}
