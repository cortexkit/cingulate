use std::{collections::BTreeMap, path::PathBuf};

use cingulate_store::{storage_descriptor, ConsentStore, RequestIdentity, Subject};
use serde_json::json;

pub const ACCOUNT: &str = "box_example";

pub fn corpus() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test-vectors/card-settled-v1.1")
}

pub fn generate() -> BTreeMap<String, Vec<u8>> {
    let dir = tempfile::tempdir().unwrap();
    let descriptor = storage_descriptor(None, dir.path().to_str().unwrap());
    let (store, _) = ConsentStore::open(&descriptor, 100).unwrap();
    let mut files = BTreeMap::new();
    for filer in ["aft", "plexus", "basal", "broca"] {
        let principal = format!("reserved:{filer}");
        let identity = RequestIdentity {
            owner: json!([principal, "daemon-stamped-session"]).to_string(),
            requester: principal,
            subject: Subject {
                agent_id: None,
                head_agent_id: String::new(),
                label: "Private card label".into(),
                via_worker: false,
                federated: None,
            },
            resolved_name: None,
            asker: None,
            head: None,
        };
        let mut request = cingulate_decode::ConsentRequest::decode(
            serde_json::from_str(include_str!(
                "../../../../test-vectors/elicitation-request-v1/valid/app-grant-once.json"
            ))
            .unwrap(),
        )
        .unwrap();
        request.requester_ref = Some("private correlation".into());
        let id = store
            .create(&identity, request, 100)
            .unwrap()
            .record
            .elicitation_id;
        store.answer(&id, "allow_once", None, 101).unwrap();
        for row in store
            .card_settled_pending(64)
            .unwrap()
            .into_iter()
            .filter(|row| row.elicitation_id == id)
        {
            let name = format!("{filer}.{}", row.reader);
            files.insert(
                format!("{name}.subject.txt"),
                ck_cingulate::card_settled::subject(ACCOUNT, &row).into_bytes(),
            );
            files.insert(
                format!("{name}.headers.json"),
                serde_json::to_vec(&json!({"Nats-Msg-Id": row.msg_id()})).unwrap(),
            );
            files.insert(format!("{name}.body.json"), row.body());
        }
    }
    files
}
