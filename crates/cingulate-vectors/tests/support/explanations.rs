use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::support::{app_body, operator, stamp, Fixture};

pub const CORPUS: &str = "consent-card-explanations-v1";

pub fn body(kind: &str, with_text: bool) -> Value {
    let mut body = app_body();
    body["kind"] = json!(kind);
    body["title"] = json!("Approve Meadow Digest version 2");
    body["prompt"] = json!("Send a daily summary of the agent's board.");
    body["target"] = json!({"kind":"flow", "label":"Meadow Digest"});
    body["scope"] = json!({"flow_id":"meadow-digest"});
    if kind == "flow_install" {
        body["flow_install"] = json!({"flow":"meadow-digest", "version":2});
    } else {
        body["package"] = json!({
            "package":"meadow-digest", "version":2, "code_hash":"a".repeat(64),
            "script":"export default flow({schedule: 'daily'});",
            "manifest_json":"{\"purpose\":\"Send a daily summary of the agent's board.\"}"
        });
    }
    if with_text {
        body["explanation"] = json!({
            "label":"What the author says this flow does",
            "text":"Meadow Digest reads the agent's board each morning.\nIt sends a short summary to the inbox, including café notes."
        });
        body["changes"] = json!({
            "label":"What the author says changed",
            "text":"Version 2 groups open items by project.\nIt no longer sends a second evening summary."
        });
    }
    body
}

pub async fn generate() -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    for (prefix, kind) in [("flow-install", "flow_install"), ("package", "package")] {
        let fixture = Fixture::open(None).await;
        let core = stamp("reserved:prefrontal-core");
        let params = body(kind, true);
        files.insert(
            format!("{prefix}-request.json"),
            serde_json::to_vec(&json!({"method":"consent.request", "params":params})).unwrap(),
        );
        let id = fixture.request(&core, params).await;
        for (name, reader, method, params) in [
            (
                "app-get",
                operator(),
                "consent.get",
                json!({"elicitationId":id}),
            ),
            (
                "app-list-pending",
                operator(),
                "consent.list_pending",
                json!({}),
            ),
            (
                "requester-get",
                core,
                "consent.get",
                json!({"elicitation_id":id}),
            ),
        ] {
            let result = fixture.ok(&reader, method, params).await;
            files.insert(
                format!("{prefix}-{name}.json"),
                serde_json::to_vec(&json!({"result":result})).unwrap(),
            );
        }
    }
    files
}
