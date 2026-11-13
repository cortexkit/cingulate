#![allow(dead_code)]

use std::collections::BTreeMap;

use ck_cingulate::Stamp;
use serde_json::{json, Value};

use crate::{secret_use, support};

pub const CORPUS: &str = "manifest-binding-v1";
pub const NOW: i64 = 1_794_355_300_123;
pub const EXPIRY: i64 = 1_794_441_700_123;
pub const AGENT: &str = "agent_4444444444444444";

pub fn sample(name: &str) -> Value {
    let source: Value = serde_json::from_slice(include_bytes!(
        "../../../../test-vectors/manifest-binding-v1/vault-samples.json"
    ))
    .unwrap();
    source["samples"][name].clone()
}

pub fn body() -> Value {
    json!({"kind":"manifest_binding", "manifest_binding":{"statement_b64url":sample("statement")["statement_b64url"]}})
}

pub fn filer() -> Stamp {
    support::stamp("reserved:plexus")
}

pub fn phone() -> Stamp {
    secret_use::phone(secret_use::DEVICE)
}

pub fn answer(id: &str, choice: &str) -> Value {
    let mut params = json!({"elicitationId":id,"choiceId":choice,"clientFeatures":["manifest-binding-approval"]});
    if choice == "allow" {
        params["envelope_b64url"] = sample("envelope")["envelope_b64url"].clone();
    }
    params
}

pub async fn fixture() -> support::Fixture {
    let f = secret_use::fixture().await;
    f.clock.set(NOW);
    f
}

async fn capture(f: &support::Fixture, stamp: &Stamp, method: &str, params: Value) -> Value {
    let reply = match f.call(stamp, method, params.clone()).await {
        Ok(reply) => json!({"result":reply}),
        Err(error) => json!({"error":error}),
    };
    json!({"method":method,"params":params,"reply":reply})
}

pub async fn generate() -> BTreeMap<String, Vec<u8>> {
    let mut cases = BTreeMap::new();
    let f = fixture().await;
    cases.insert(
        "filer-request.json",
        capture(&f, &filer(), "consent.request", body()).await,
    );
    let id = cases["filer-request.json"]["reply"]["result"]["elicitation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    cases.insert(
        "filer-request-repeat.json",
        capture(&f, &filer(), "consent.request", body()).await,
    );
    let get = json!({"elicitationId":id,"clientFeatures":["manifest-binding-approval"]});
    let card = capture(&f, &phone(), "consent.get", get).await;
    cases.insert("phone-get.json", card.clone());
    let mut mismatch = card;
    mismatch["reply"]["result"]["manifestBinding"]["repo"] = json!("Other/Repository");
    mismatch["phone_must_refuse"] =
        json!("decoded repo differs from the committed statement; not served by Cingulate");
    cases.insert("phone-refuse-mismatch.json", mismatch);
    cases.insert(
        "phone-answer-allow.json",
        capture(&f, &phone(), "consent.answer", answer(&id, "allow")).await,
    );
    cases.insert(
        "filer-await-allowed.json",
        capture(
            &f,
            &filer(),
            "consent.await",
            json!({"elicitation_id":id,"timeout_ms":0}),
        )
        .await,
    );
    cases.insert(
        "filer-get-allowed.json",
        capture(&f, &filer(), "consent.get", json!({"elicitation_id":id})).await,
    );
    for (choice, name) in [("decline", "declined"), ("allow", "expired")] {
        let f = fixture().await;
        let id = f.request(&filer(), body()).await;
        if name == "expired" {
            f.clock.set(EXPIRY);
        }
        cases.insert(
            if name == "expired" {
                "phone-answer-expired.json"
            } else {
                "phone-answer-decline.json"
            },
            capture(&f, &phone(), "consent.answer", answer(&id, choice)).await,
        );
        cases.insert(
            if name == "expired" {
                "filer-await-expired.json"
            } else {
                "filer-await-declined.json"
            },
            capture(
                &f,
                &filer(),
                "consent.await",
                json!({"elicitation_id":id,"timeout_ms":0}),
            )
            .await,
        );
        cases.insert(
            if name == "expired" {
                "filer-get-expired.json"
            } else {
                "filer-get-declined.json"
            },
            capture(&f, &filer(), "consent.get", json!({"elicitation_id":id})).await,
        );
    }
    let f = fixture().await;
    let id = f.request(&filer(), body()).await;
    let withdraw = json!({"elicitation_id":id});
    cases.insert(
        "other-withdraw-refused.json",
        capture(
            &f,
            &support::stamp("reserved:cerebellum"),
            "consent.withdraw",
            withdraw.clone(),
        )
        .await,
    );
    cases.insert(
        "filer-withdraw.json",
        capture(&f, &filer(), "consent.withdraw", withdraw.clone()).await,
    );
    cases.insert(
        "phone-get-withdrawn.json",
        capture(
            &f,
            &phone(),
            "consent.get",
            json!({"elicitationId":id,"clientFeatures":["manifest-binding-approval"]}),
        )
        .await,
    );
    cases.insert(
        "filer-withdraw-repeat.json",
        capture(&f, &filer(), "consent.withdraw", withdraw).await,
    );
    cases
        .into_iter()
        .map(|(name, value)| {
            let mut bytes = serde_json::to_vec_pretty(&value).unwrap();
            bytes.push(b'\n');
            (name.into(), bytes)
        })
        .collect()
}
