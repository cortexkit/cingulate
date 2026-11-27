use std::{collections::BTreeMap, fs, path::Path};

use ck_cingulate::{Stamp, DEVELOPMENT_LOCAL_OPERATOR};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use subc_protocol::Principal;

use crate::{
    flow_card_structure::{bind, desktop, phone_stamp, served},
    support::{worker, Fixture},
    watch_pending::INSTANCE,
};

pub const SERVED_V1: &str = "basal-flow-install-cards-served-v1";
pub const ARCHIVED_REQUEST_SUFFIX: &str = "-request.json";
pub const CARDS: [&str; 7] = [
    "agent-new",
    "agent-update-v2",
    "flow-secrets",
    "max-explanation",
    "operator-install",
    "package-register",
    "switch-refile-agent",
];
const FILED_AT_MS: i64 = 1_777_593_540_000;

pub async fn generate(
    input_dir: &Path,
    output_dir: &Path,
    input_suffix: &str,
) -> BTreeMap<String, Vec<u8>> {
    let fixture = Fixture::open(None).await;
    fixture.clock.set(FILED_AT_MS);
    fixture.pin_store_instance(INSTANCE);
    let requester =
        Stamp::from_bind(&bind("sample-flow-runner").with_principal(worker::principal("basal")));
    let phone = phone_stamp();
    // Keep verified desktop gets on release admission even when generating the
    // plain local, development-only list on the same fixture.
    let mut desktop = desktop();
    desktop.local_operator = false;
    let features = json!(["secret-use-approval"]);
    let mut files = BTreeMap::new();
    let mut phone_ids = Vec::new();

    for (index, card) in CARDS.into_iter().enumerate() {
        let input_name = format!("{card}{ARCHIVED_REQUEST_SUFFIX}");
        let input_path = input_dir.join(format!("{card}{input_suffix}"));
        let input = fs::read(&input_path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", input_path.display()));
        let filing: Value = serde_json::from_slice(&input).unwrap();
        assert_eq!(filing["provider"], "cingulate", "{card}");
        assert_eq!(filing["method"], "consent.request", "{card}");
        let reply = fixture
            .call(&requester, "consent.request", filing["params"].clone())
            .await
            .unwrap_or_else(|error| panic!("{card} filing refused: {error}"));
        let id = format!("el_{}", index + 1);
        assert_eq!(reply["elicitation_id"], id, "{card}");
        files.insert(input_name, input);

        // Capture successful replies directly, including the handler's result envelope.
        let params = json!({"elicitationId":id,"clientFeatures":features});
        let phone_get = served(&fixture, &phone, "consent.get", params.clone()).await;
        let record: Value = serde_json::from_slice(&phone_get).unwrap();
        assert_eq!(record["result"]["elicitationId"], id, "{card}");
        phone_ids.push(id);

        if card == "flow-secrets" {
            let error = fixture
                .call(&desktop, "consent.get", params)
                .await
                .unwrap_err();
            assert_eq!(error["code"], "elicitation_not_found");
            // Refusals are typed outcomes, with no result bytes to capture.
            files.insert(
                format!("{card}-desktop-get-refusal.json"),
                serde_json::to_vec(&json!({"error":error})).unwrap(),
            );
        } else {
            let desktop_get = served(&fixture, &desktop, "consent.get", params).await;
            assert_eq!(desktop_get, phone_get, "operator get: {card}");
            files.insert(format!("{card}-desktop-get.json"), desktop_get);
        }
        files.insert(format!("{card}-phone-get.json"), phone_get);
    }

    let phone_list = pending_list(&fixture, &phone, features, &phone_ids, "phone").await;
    files.insert("phone-list-pending.json".into(), phone_list);
    if DEVELOPMENT_LOCAL_OPERATOR {
        let local = Stamp::from_bind(&bind("local-desktop").with_principal(Principal::Direct));
        let bytes = pending_list(
            &fixture,
            &local,
            json!([
                "option-group",
                "secret-use-approval",
                "manifest-binding-approval"
            ]),
            &phone_ids,
            "plain local desktop with dev-local-operator",
        )
        .await;
        let reply: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            reply["result"]["records"][2]["flowInstall"]["secrets"]
                .as_array()
                .map(Vec::len),
            Some(2),
            "development desktop list must serve both flow secret statements"
        );
        files.insert("desktop-list-pending.json".into(), bytes);
    }

    files.insert(
        "README.md".into(),
        fs::read(output_dir.join("README.md")).unwrap(),
    );
    let sums: String = files
        .iter()
        .map(|(name, bytes)| format!("{:x}  {name}\n", Sha256::digest(bytes)))
        .collect();
    files.insert("SHA256SUMS".into(), sums.into_bytes());
    files
}

async fn pending_list(
    fixture: &Fixture,
    stamp: &Stamp,
    features: Value,
    ids: &[String],
    surface: &str,
) -> Vec<u8> {
    let bytes = served(
        fixture,
        stamp,
        "consent.list_pending",
        json!({"clientFeatures":features}),
    )
    .await;
    let list: Value = serde_json::from_slice(&bytes).unwrap();
    let result = &list["result"];
    let listed_ids: Vec<_> = result["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["elicitationId"].as_str().unwrap())
        .collect();
    assert_eq!(listed_ids, ids, "{surface} pending page");
    assert!(
        result.get("continuation").is_none(),
        "{surface} pending page"
    );
    bytes
}
