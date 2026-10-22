use crate::support::{stamp, Fixture};
use ck_cingulate::Stamp;
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const DEVICE: &str = "300c9c9603b92a4b39ed3958bf924011";
pub const OTHER: &str = "00000000000000000000000000000000";
pub const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

pub fn phone(device: &str) -> Stamp {
    Stamp {
        operator_surface: true,
        harness: format!("fed-phone:{device}"),
        ..stamp("reserved:callosum")
    }
}

pub fn body() -> Value {
    json!({
        "kind":"rich_ui_approval", "title":"Approve provider interface", "prompt":"Review on this phone",
        "rich_ui_approval": {"approvalDigest": DIGEST, "manifestDigest": DIGEST, "capabilityDigest": DIGEST, "provider":"weather-tools", "device":DEVICE},
        "options":[{"id":"approve","label":"Approve","effect":"grant"},{"id":"deny","label":"Decline","effect":"decline"}],
        "default":"deny", "on_expiry":"deny", "urgency":"normal", "material_damage":false,
        "late_execution":"notify_only", "scope":{"provider":"weather-tools"},
        "target":{"kind":"provider","label":"Weather tools"}
    })
}

pub fn answer(id: &str, choice: &str) -> Value {
    json!({"elicitationId":id,"choiceId":choice,"bindDigest":DIGEST,"clientFeatures":["rich-ui-approval"]})
}

pub async fn get(f: &Fixture, id: &str) -> Value {
    f.ok(
        &phone(DEVICE),
        "consent.get",
        json!({"elicitationId":id,"clientFeatures":["rich-ui-approval"]}),
    )
    .await
}

pub async fn samples() -> BTreeMap<String, Value> {
    let f = Fixture::open(None).await;
    let owner = stamp("reserved:occipital");
    let mut files = BTreeMap::new();
    let id = f.request(&owner, body()).await;
    files.insert("pending.json".into(), get(&f, &id).await);
    files.insert("feature-with.json".into(), get(&f, &id).await);
    files.insert(
        "feature-without.json".into(),
        f.call(&phone(DEVICE), "consent.get", json!({"elicitationId":id}))
            .await
            .unwrap_err(),
    );
    let matching = answer(&id, "approve");
    let mut different = matching.clone();
    different["bindDigest"] = json!(format!("sha256:{}", "b".repeat(64)));
    for (file, route, params) in [
        ("answer-different.json", phone(DEVICE), different),
        ("answer-wrong-device.json", phone(OTHER), matching.clone()),
        ("answer-matching.json", phone(DEVICE), matching),
    ] {
        let reply = match f.call(&route, "consent.answer", params.clone()).await {
            Ok(result) => json!({"result":result}),
            Err(error) => json!({"error":error}),
        };
        files.insert(file.into(), json!({"params":params,"reply":reply}));
    }
    files.insert("granted.json".into(), get(&f, &id).await);
    let params = json!({"elicitationId":id});
    let error = f
        .call(&stamp("reserved:basal"), "consent.revoke", params.clone())
        .await
        .unwrap_err();
    files.insert(
        "revoke-non-requester.json".into(),
        json!({"params":params,"error":error}),
    );
    f.ok(&owner, "consent.revoke", params).await;
    files.insert("revoked.json".into(), get(&f, &id).await);
    let id = f.request(&owner, body()).await;
    f.ok(&phone(DEVICE), "consent.answer", answer(&id, "deny"))
        .await;
    files.insert("declined.json".into(), get(&f, &id).await);
    let id = f.request(&owner, body()).await;
    f.ok(&owner, "consent.withdraw", json!({"elicitation_id":id}))
        .await;
    files.insert("withdrawn.json".into(), get(&f, &id).await);
    files
}
