#[path = "support/explanations.rs"]
mod explanations;
mod support;

use std::{collections::BTreeSet, fs, path::Path};

use ck_cingulate::{Handler, Options};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use subc_protocol::ModuleHelloAckBody;
use support::{corpus, descriptor, operator, stamp, Fixture};

const FILES: [&str; 9] = [
    "README.md",
    "flow-install-app-get.json",
    "flow-install-app-list-pending.json",
    "flow-install-request.json",
    "flow-install-requester-get.json",
    "package-app-get.json",
    "package-app-list-pending.json",
    "package-request.json",
    "package-requester-get.json",
];

#[tokio::test]
async fn explanation_vectors_regenerate_byte_for_byte() {
    let generated = explanations::generate().await;
    assert_eq!(generated.len(), FILES.len() - 1);
    for (name, bytes) in generated {
        assert_eq!(
            fs::read(corpus(explanations::CORPUS).join(name)).unwrap(),
            bytes
        );
    }
}

#[test]
fn explanation_samples_pin_unchanged_text_on_both_app_readbacks() {
    let sample = |file: &str| -> Value {
        serde_json::from_slice(&fs::read(corpus(explanations::CORPUS).join(file)).unwrap()).unwrap()
    };
    for prefix in ["flow-install", "package"] {
        let request = sample(&format!("{prefix}-request.json"))["params"].clone();
        let get = sample(&format!("{prefix}-app-get.json"))["result"].clone();
        let list =
            sample(&format!("{prefix}-app-list-pending.json"))["result"]["records"][0].clone();
        let requester = sample(&format!("{prefix}-requester-get.json"))["result"].clone();
        for field in ["explanation", "changes"] {
            assert!(request.get(field).is_some());
            for card in [&get, &list, &requester] {
                assert_eq!(card[field], request[field], "{prefix} {field}");
            }
        }
    }
}

#[test]
fn explanation_checksums_cover_exactly_the_served_corpus() {
    let root = corpus(explanations::CORPUS);
    let sums = fs::read(root.join("SHA256SUMS")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&sums)),
        "b28da78845d77564e7256d65b0a1e36dc3bc4e78cc600eb501cbafe0c1bcef73"
    );
    let mut seen = BTreeSet::new();
    for line in std::str::from_utf8(&sums).unwrap().lines() {
        let (hash, name) = line.split_once("  ").unwrap();
        assert!(seen.insert(name.to_owned()));
        assert_eq!(
            format!("{:x}", Sha256::digest(fs::read(root.join(name)).unwrap())),
            hash
        );
    }
    assert_eq!(seen, FILES.iter().map(|name| name.to_string()).collect());
    seen.insert("SHA256SUMS".into());
    assert_eq!(
        fs::read_dir(root)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect::<BTreeSet<_>>(),
        seen
    );
}

async fn reopen(fixture: &mut Fixture) {
    let path = fixture.sql().path().unwrap().to_owned();
    fixture.handler = Handler::new(Options {
        clock: fixture.clock.clone(),
        sweep_timer: false,
        data_home: Path::new(&path).parent().unwrap().to_str().unwrap().into(),
        ..Options::default()
    });
    // Finish the old handler's notice task so its clone releases the store
    // lease, as terminating the old process would do on a real restart.
    for _ in 0..4 {
        tokio::task::yield_now().await;
    }
    fixture
        .handler
        .hello_ack(&ModuleHelloAckBody {
            negotiated_ver: 1,
            subc_ops: vec![],
            subc_capabilities: vec![],
            storage: Some(serde_json::to_value(descriptor(Path::new(&path))).unwrap()),
            machine_id: None,
        })
        .await;
    assert_eq!(
        fixture.handler.health_report().status,
        subc_protocol::session::HealthStatus::Ok
    );
}

#[tokio::test]
async fn author_text_survives_restart_and_old_records_omit_absent_fields() {
    for kind in ["flow_install", "package"] {
        for with_text in [false, true] {
            let mut fixture = Fixture::open(None).await;
            let core = stamp("reserved:prefrontal-core");
            let mut body = explanations::body(kind, with_text);
            if with_text {
                body["explanation"]["text"] = json!("é".repeat(3000));
                body["changes"]["text"] = json!("🌿".repeat(2048));
                body["changes"]["label"] = json!("🌿".repeat(64));
            }
            let id = fixture.request(&core, body.clone()).await;
            let stored: Value = serde_json::to_value(fixture.stored_record(&id)).unwrap();
            for field in ["explanation", "changes"] {
                assert_eq!(stored["request"].get(field), body.get(field));
            }
            reopen(&mut fixture).await;
            let requester = fixture
                .ok(&core, "consent.get", json!({"elicitation_id":id}))
                .await;
            let app = fixture
                .ok(&operator(), "consent.get", json!({"elicitationId":id}))
                .await;
            let list = fixture
                .ok(&operator(), "consent.list_pending", json!({}))
                .await;
            assert_eq!(list["records"].as_array().unwrap().len(), 1);
            for field in ["explanation", "changes"] {
                for card in [&requester, &app, &list["records"][0]] {
                    assert_eq!(card.get(field), body.get(field), "{kind} {field}");
                }
            }
        }
    }
}

fn assert_no_author_text(value: &Value) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                assert!(!matches!(key.as_str(), "explanation" | "changes"));
                assert_no_author_text(value);
            }
        }
        Value::Array(array) => array.iter().for_each(assert_no_author_text),
        Value::String(text) => {
            assert!(!text.contains("What the author says"));
            assert!(!text.contains("Meadow Digest reads"));
            assert!(!text.contains("Version 2 groups"));
        }
        _ => {}
    }
}

#[tokio::test]
async fn author_text_never_enters_outbox_notices_or_push_pointers() {
    for kind in ["flow_install", "package"] {
        let fixture = Fixture::open(None).await;
        let core = stamp("reserved:prefrontal-core");
        let id = fixture.request(&core, explanations::body(kind, true)).await;
        fixture
            .ok(
                &operator(),
                "consent.answer",
                json!({"elicitationId":id,"choiceId":"deny"}),
            )
            .await;
        let id = fixture.request(&core, explanations::body(kind, true)).await;
        fixture
            .ok(&core, "consent.withdraw", json!({"elicitation_id":id}))
            .await;
        let db = fixture.sql();
        let mut query = db
            .prepare("SELECT body_json FROM access_notices ORDER BY rowid")
            .unwrap();
        let notices: Vec<Value> = query
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .map(|row| serde_json::from_str(&row.unwrap()).unwrap())
            .collect();
        assert_eq!(notices.len(), 4);
        let purposes: Vec<_> = notices
            .iter()
            .map(|notice| notice["purpose"].as_str().unwrap())
            .collect();
        assert_eq!(purposes, ["escalated", "settled", "escalated", "withdrawn"]);
        for notice in &notices {
            assert_no_author_text(notice);
            let keys: BTreeSet<_> = notice
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            let expected: BTreeSet<_> = if notice["purpose"] == "withdrawn" {
                ["elicitation_id", "purpose"].into_iter().collect()
            } else {
                ["elicitation_id", "purpose", "fields"]
                    .into_iter()
                    .collect()
            };
            assert_eq!(keys, expected);
        }
        let pointers = fixture.payloads();
        assert!(!pointers.is_empty());
        for pointer in &pointers {
            assert_no_author_text(pointer);
            assert_eq!(
                pointer
                    .as_object()
                    .unwrap()
                    .keys()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>(),
                ["elicitationId", "kind", "eventAtMs"].into_iter().collect()
            );
        }
    }
}
