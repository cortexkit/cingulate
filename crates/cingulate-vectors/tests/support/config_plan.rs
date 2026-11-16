#![allow(dead_code)]

use std::collections::BTreeMap;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ck_cingulate::Stamp;
use serde_json::{json, Value};

use crate::support;

pub const CORPUS: &str = "config-plan-v4";
// Deliberately not JSON and not a real signature: Cingulate is a byte carrier.
pub const STATEMENT: &str = "_wABeCBub3QtSlNPTgo";
pub const SIGNATURE: &str = "AQIDBA";

pub fn body() -> Value {
    json!({"kind":"config_plan","expires_in_ms":300000,"binding":{"correlation":"fake-correlation"},"config_plan":{
        "plan_digest":"fake-plan-digest","base_revision":"42",
        "apply":{"operation":"change","rollback_to":null,"modules":[],"review":null},"changes":[{
            "module_id":"example-module","project_id":"example-project","path":"settings.enabled",
            "old":false,"new":true,"old_source":"default","new_source":"project","apply":"live"
        }],"other_sections":{"policy_changes":[{"example":true}]}
    }})
}

pub fn all_sections_body() -> Value {
    let mut b = body();
    b["config_plan"]["apply"] = json!({"operation":"change","rollback_to":null,"modules":[{
        "module_id":"example-module","transition":"module_restart","whole_generation":true,
        "waits_for_next_start":false,"waits_for_daemon_restart":false,
        "deferred_live_fields":["settings.enabled"],"explanation":"Restart the module to apply its new settings."
    }],"review":{"supervisor_reconcile":true}});
    b["config_plan"]["affected_projects"] = json!(["example-project"]);
    b["config_plan"]["policy_changes"] = json!([
        {"kind":"declaration","module_id":"example-module","old":null,"new":{"read":true},"reason":"Allow project reads"},
        {"kind":"subc_jsonc","old":{"redacted":true},"new":{"redacted":true}},
        {"kind":"reviewed_program","module_id":"example-module","old":null,"new":"bin/tool"}
    ]);
    b["config_plan"]["launch_manifests"] = json!([{"module_id":"example-module","origin":"project","programs":[{
        "input":"bin/tool","old":null,"new":{"program":"bin/tool","args":["--safe"]}
    }]}]);
    b["config_plan"]
        .as_object_mut()
        .unwrap()
        .remove("other_sections");
    b
}

pub fn layer_file_body() -> Value {
    let mut b = all_sections_body();
    b["config_plan"]["apply"] =
        json!({"operation":"change","rollback_to":null,"modules":[],"review":null});
    b["config_plan"]["changes"] = json!([]);
    b["config_plan"]
        .as_object_mut()
        .unwrap()
        .remove("launch_manifests");
    b["config_plan"]["policy_changes"] = json!([{
        "kind":"layer_file","module_id":"example-module","layer":"user",
        "old":{"sha256":"a".repeat(64),"bytes":42},"new":{"absent":true}
    }]);
    b
}

pub fn rollback_body() -> Value {
    let mut b = all_sections_body();
    b["config_plan"]["apply"]["operation"] = json!("rollback");
    b["config_plan"]["apply"]["rollback_to"] = json!("41");
    b["config_plan"]["apply"]["rollback_programs"] = json!([{
        "module_id":"example-module","input":"bin/tool",
        "approved_then":{"program":"bin/tool","sha256":"a".repeat(64)},
        "on_disk_now":{"program":"bin/tool","sha256":"b".repeat(64)}
    }]);
    let manifest = &mut b["config_plan"]["launch_manifests"][0];
    manifest["program_changed_since_target"] = json!(true);
    manifest["program_changed_since_active"] = json!(true);
    b
}

pub fn flag_only_body() -> Value {
    let mut b = all_sections_body();
    b["config_plan"]["changes"] = json!([]);
    b["config_plan"]
        .as_object_mut()
        .unwrap()
        .remove("policy_changes");
    b["config_plan"]["launch_manifests"] = json!([{
        "module_id":"example-module","programs":[],"program_changed_since_active":true
    }]);
    b
}

pub fn route_rebind_body() -> Value {
    let mut b = all_sections_body();
    let module = &mut b["config_plan"]["apply"]["modules"][0];
    module["transition"] = json!("route_rebind");
    module["whole_generation"] = json!(false);
    module["explanation"] =
        json!("This module's open connections close and reopen under the new settings.");
    b["config_plan"]
        .as_object_mut()
        .unwrap()
        .remove("launch_manifests");
    b
}

pub fn filer() -> Stamp {
    support::stamp("reserved:callosum")
}
pub fn phone() -> Stamp {
    Stamp {
        operator_surface: true,
        harness: format!("fed-phone:{}", "a".repeat(32)),
        ..filer()
    }
}
pub fn answer(id: &str, choice: &str) -> Value {
    let mut p = json!({"elicitationId":id,"choiceId":choice,"clientFeatures":["config-plan"]});
    if choice == "approve" {
        p["statement"] = json!(STATEMENT);
        p["signature_b64url"] = json!(SIGNATURE);
    }
    p
}

pub fn answer_round(id: &str, choice: &str, round: u64) -> Value {
    let mut params = answer(id, choice);
    params["expectedRound"] = json!(round);
    if choice == "approve" && round > 0 {
        params["statement"] =
            json!(URL_SAFE_NO_PAD.encode(format!("fake statement for round {round}")));
    }
    params
}
pub fn get(id: &str) -> Value {
    json!({"elicitationId":id,"clientFeatures":["config-plan"]})
}

async fn capture(f: &support::Fixture, stamp: &Stamp, method: &str, params: Value) -> Value {
    let reply = match f.call(stamp, method, params.clone()).await {
        Ok(v) => json!({"result":v}),
        Err(v) => json!({"error":v}),
    };
    json!({"method":method,"params":params,"reply":reply})
}

pub async fn generate() -> BTreeMap<String, Vec<u8>> {
    let f = support::Fixture::open(None).await;
    let mut cases = BTreeMap::new();
    for (name, mut params) in [
        ("change-all-sections.json", all_sections_body()),
        ("rollback.json", rollback_body()),
        ("flag-only.json", flag_only_body()),
        ("route-rebind.json", route_rebind_body()),
        ("false-program-flag.json", flag_only_body()),
        ("empty-manifest.json", flag_only_body()),
        ("missing-programs.json", flag_only_body()),
        ("rollback-programs-on-change.json", rollback_body()),
        ("program-only.json", all_sections_body()),
        ("layer-file-only.json", layer_file_body()),
        ("unknown-transition.json", all_sections_body()),
        ("over-cap-list.json", all_sections_body()),
        ("missing-apply.json", all_sections_body()),
        ("empty-plan.json", all_sections_body()),
        ("malformed-layer-file-old.json", layer_file_body()),
    ] {
        match name {
            "false-program-flag.json" => {
                params["config_plan"]["launch_manifests"][0]["program_changed_since_active"] =
                    json!(false);
            }
            "empty-manifest.json" => {
                params["config_plan"]["launch_manifests"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("program_changed_since_active");
            }
            "missing-programs.json" => {
                params["config_plan"]["launch_manifests"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("programs");
            }
            "rollback-programs-on-change.json" => {
                params["config_plan"]["apply"]["operation"] = json!("change");
                params["config_plan"]["apply"]["rollback_to"] = Value::Null;
            }
            "program-only.json" => {
                params["config_plan"]["changes"] = json!([]);
                params["config_plan"]
                    .as_object_mut()
                    .unwrap()
                    .remove("policy_changes");
            }
            "unknown-transition.json" => {
                params["config_plan"]["apply"]["modules"][0]["transition"] = json!("future")
            }
            "over-cap-list.json" => {
                params["config_plan"]["affected_projects"] =
                    json!((0..65).map(|i| format!("project-{i}")).collect::<Vec<_>>())
            }
            "missing-apply.json" => {
                params["config_plan"]
                    .as_object_mut()
                    .unwrap()
                    .remove("apply");
            }
            "empty-plan.json" => {
                for field in ["changes", "policy_changes", "launch_manifests"] {
                    params["config_plan"][field] = json!([]);
                }
            }
            "malformed-layer-file-old.json" => {
                params["config_plan"]["policy_changes"][0]["old"] =
                    json!({"sha256":"NOT-A-HASH","bytes":42})
            }
            _ => {}
        }
        let fixture = support::Fixture::open(None).await;
        let mut sample = capture(&fixture, &filer(), "consent.request", params).await;
        sample["tag"] = json!(CORPUS);
        if let Some(id) = sample["reply"]["result"]["elicitation_id"].as_str() {
            sample["phone"] = fixture.ok(&phone(), "consent.get", get(id)).await;
        }
        cases.insert(name, sample);
    }
    cases.insert(
        "filing.json",
        capture(&f, &filer(), "consent.request", body()).await,
    );
    let id = cases["filing.json"]["reply"]["result"]["elicitation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    cases.insert(
        "phone-list.json",
        capture(
            &f,
            &phone(),
            "consent.list_pending",
            json!({"clientFeatures":["config-plan"]}),
        )
        .await,
    );
    cases.insert(
        "phone-get.json",
        capture(&f, &phone(), "consent.get", get(&id)).await,
    );
    let mut missing = answer(&id, "approve");
    missing.as_object_mut().unwrap().remove("statement");
    cases.insert(
        "missing-statement.json",
        capture(&f, &phone(), "consent.answer", missing).await,
    );
    cases.insert(
        "wrong-filer.json",
        capture(
            &f,
            &support::stamp("reserved:plexus"),
            "consent.request",
            body(),
        )
        .await,
    );
    cases.insert(
        "approve.json",
        capture(&f, &phone(), "consent.answer", answer(&id, "approve")).await,
    );
    cases.insert(
        "requester-get.json",
        capture(&f, &filer(), "consent.get", json!({"elicitation_id":id})).await,
    );
    cases.insert(
        "requester-await.json",
        capture(
            &f,
            &filer(),
            "consent.await",
            json!({"elicitation_id":id,"timeout_ms":0}),
        )
        .await,
    );
    cases.insert(
        "report-unverified.json",
        capture(
            &f,
            &filer(),
            "consent.report_unverified",
            json!({"elicitation_id":id,"answer_seq":1,"code":"signature"}),
        )
        .await,
    );
    cases.insert(
        "phone-reopened.json",
        capture(&f, &phone(), "consent.get", get(&id)).await,
    );
    cases.insert(
        "stale-round.json",
        capture(&f, &phone(), "consent.answer", answer(&id, "approve")).await,
    );
    let mut reused = answer(&id, "approve");
    reused["expectedRound"] = json!(1);
    cases.insert(
        "statement-reused.json",
        capture(&f, &phone(), "consent.answer", reused).await,
    );
    cases.insert(
        "second-answer.json",
        capture(
            &f,
            &phone(),
            "consent.answer",
            answer_round(&id, "approve", 1),
        )
        .await,
    );
    let db = f.sql();
    let mut rows = db
        .prepare("SELECT body_json FROM access_notices WHERE elicitation_id=?1 ORDER BY rowid")
        .unwrap();
    let notices: Vec<Value> = rows
        .query_map([&id], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|row| serde_json::from_str(&row.unwrap()).unwrap())
        .collect();
    cases.insert("person-notices-two-rounds.json", json!({"notices":notices}));
    for name in ["decline", "withdraw", "expiry"] {
        let f = support::Fixture::open(None).await;
        let id = f.request(&filer(), body()).await;
        let (method, params) = match name {
            "decline" => ("consent.answer", answer(&id, "decline")),
            "withdraw" => ("consent.withdraw", json!({"elicitation_id":id})),
            _ => {
                f.clock.set(support::NOW + 300000);
                ("consent.get", json!({"elicitation_id":id}))
            }
        };
        let actor = if name == "decline" { phone() } else { filer() };
        cases.insert(
            match name {
                "decline" => "decline.json",
                "withdraw" => "withdraw.json",
                _ => "expiry.json",
            },
            capture(&f, &actor, method, params).await,
        );
    }
    cases
        .into_iter()
        .map(|(name, mut value)| {
            value["tag"] = json!(CORPUS);
            let mut bytes = serde_json::to_vec_pretty(&value).unwrap();
            bytes.push(b'\n');
            (name.into(), bytes)
        })
        .collect()
}
