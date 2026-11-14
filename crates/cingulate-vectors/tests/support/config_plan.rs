#![allow(dead_code)]

use std::collections::BTreeMap;

use ck_cingulate::Stamp;
use serde_json::{json, Value};

use crate::support;

pub const CORPUS: &str = "config-plan-v1";
// Deliberately not JSON and not a real signature: Cingulate is a byte carrier.
pub const STATEMENT: &str = "_wABeCBub3QtSlNPTgo";
pub const SIGNATURE: &str = "AQIDBA";

pub fn body() -> Value {
    json!({"kind":"config_plan","expires_in_ms":300000,"binding":{"correlation":"fake-correlation"},"config_plan":{
        "plan_digest":"fake-plan-digest","base_revision":"42","changes":[{
            "module_id":"example-module","project_id":"example-project","path":"settings.enabled",
            "old":false,"new":true,"old_source":"default","new_source":"project","apply":"live"
        }],"other_sections":{"policy_changes":{"example":true}}
    }})
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
        "second-answer.json",
        capture(&f, &phone(), "consent.answer", answer(&id, "approve")).await,
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
        .map(|(name, value)| {
            let mut bytes = serde_json::to_vec_pretty(&value).unwrap();
            bytes.push(b'\n');
            (name.into(), bytes)
        })
        .collect()
}
