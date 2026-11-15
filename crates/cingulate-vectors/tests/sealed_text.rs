#[path = "support/phone.rs"]
#[allow(dead_code)]
mod phone;
#[path = "support/sealed_text.rs"]
mod sealed_text;
mod support;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use cingulate_decode::ConsentRequest;
use sealed_text::{answer, desktop, filer, request, CORPUS};
use serde_json::{json, Value};
use support::{stamp, Fixture, NOW};

fn key() -> String {
    URL_SAFE_NO_PAD.encode([41u8; 32])
}
fn input() -> Value {
    json!({"id":"password","enc":URL_SAFE_NO_PAD.encode([42u8;32]),"ciphertext":URL_SAFE_NO_PAD.encode([43u8;37])})
}
async fn filed() -> Fixture {
    let fixture = Fixture::open(None).await;
    assert_eq!(fixture.request(&filer(), request(&key())).await, "el_1");
    fixture
}
async fn approve(fixture: &Fixture) -> Value {
    fixture
        .ok(&phone::phone_stamp(), "consent.answer", answer(input()))
        .await
}
async fn filer_read(fixture: &Fixture) -> Value {
    fixture
        .ok(&filer(), "consent.get", json!({"elicitation_id":"el_1"}))
        .await
}
fn sealed_payload(fixture: &Fixture) -> Option<String> {
    fixture
        .sql()
        .query_row(
            "SELECT sealed_inputs_json FROM elicitation_answers WHERE elicitation_id='el_1'",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

#[tokio::test]
async fn sealed_text_request_validation_is_closed_and_precedes_creation() {
    let fixture = Fixture::open(None).await;
    let base = request(&key());
    let mut bad = Vec::new();
    for value in [
        json!(0),
        json!(4097),
        json!(-1),
        json!(1.5),
        json!("64"),
        Value::Null,
    ] {
        let mut body = base.clone();
        body["inputs"][0]["max_bytes"] = value;
        bad.push(body);
    }
    for (field, value) in [
        ("kind", json!("text")),
        ("id", json!(" ")),
        ("label", json!("")),
    ] {
        let mut body = base.clone();
        body["inputs"][0][field] = value;
        bad.push(body);
    }
    for public_key in [
        "AQ".to_owned(),
        format!("{}=", key()),
        "!".repeat(43),
        URL_SAFE_NO_PAD.encode([1u8; 33]),
    ] {
        let mut body = base.clone();
        body["inputs"][0]["hpke"]["public_key"] = json!(public_key);
        bad.push(body);
    }
    let mut suite = base.clone();
    suite["inputs"][0]["hpke"]["suite"] = json!("other");
    bad.push(suite);
    let mut two = base.clone();
    two["inputs"] = json!([base["inputs"][0], base["inputs"][0]]);
    bad.push(two);
    for value in [Value::Null, json!({}), json!([{"kind":"sealed_text"}])] {
        let mut body = base.clone();
        body["inputs"] = value;
        bad.push(body);
    }
    let mut unknown = base.clone();
    unknown["inputs"][0]["extra"] = json!(true);
    bad.push(unknown);
    for body in bad {
        assert_eq!(
            fixture
                .call(&filer(), "consent.request", body)
                .await
                .unwrap_err()["code"],
            "elicitation_invalid_request"
        );
    }
    assert!(fixture.rows()[0].is_empty());
    for max in [1, 4096] {
        let mut body = base.clone();
        body["inputs"][0]["max_bytes"] = json!(max);
        ConsentRequest::decode(body).unwrap();
    }
}

#[tokio::test]
async fn sealed_text_is_refused_on_every_nonordinary_card_including_tool_permission() {
    let fixture = Fixture::open(None).await;
    for kind in [
        "secret_use",
        "manifest_binding",
        "config_plan",
        "flow_install",
        "package",
        "flow_decision",
        "tool_permission",
        "macos_permission",
    ] {
        let mut body = request(&key());
        body["kind"] = json!(kind);
        assert_eq!(
            fixture
                .call(&filer(), "consent.request", body.clone())
                .await
                .unwrap_err()["code"],
            "elicitation_invalid_request",
            "{kind}"
        );
        assert_eq!(
            ConsentRequest::decode(body).unwrap_err().code,
            "elicitation_invalid_request"
        );
    }
    assert!(fixture.rows()[0].is_empty());
}

#[tokio::test]
async fn sealed_text_readers_preserve_filed_definitions_and_redact_answers() {
    let fixture = filed().await;
    for who in [phone::phone_stamp(), desktop()] {
        let card = fixture
            .ok(&who, "consent.get", json!({"elicitationId":"el_1"}))
            .await;
        assert_eq!(card["inputs"], request(&key())["inputs"]);
        assert!(card.get("round").is_none());
    }
    let list = fixture
        .ok(&desktop(), "consent.list_pending", json!({}))
        .await;
    assert_eq!(list["records"][0]["inputs"], request(&key())["inputs"]);
    assert_eq!(
        filer_read(&fixture).await["inputs"],
        request(&key())["inputs"]
    );
    let response = approve(&fixture).await;
    assert_eq!(response["elicitation"]["input_supplied"], true);
    for who in [phone::phone_stamp(), desktop()] {
        let card = fixture
            .ok(&who, "consent.get", json!({"elicitationId":"el_1"}))
            .await;
        assert_eq!(card["input_supplied"], true);
        assert!(card.get("answer_inputs").is_none());
    }
    assert_eq!(
        filer_read(&fixture).await["answer_inputs"],
        json!([input()])
    );
    let awaited = fixture
        .ok(
            &filer(),
            "consent.await",
            json!({"elicitation_id":"el_1","timeout_ms":0}),
        )
        .await;
    assert_eq!(awaited["answer_inputs"], json!([input()]));
    let feed = fixture
        .ok(&filer(), "consent.answers", json!({"since":0}))
        .await;
    assert_eq!(feed["records"][0]["input_supplied"], true);
    assert!(!feed
        .to_string()
        .contains(input()["ciphertext"].as_str().unwrap()));
    let outcomes = fixture
        .ok(
            &stamp("reserved:prefrontal-core"),
            "consent.outcomes",
            json!({"since":0}),
        )
        .await;
    assert!(!outcomes
        .to_string()
        .contains(input()["ciphertext"].as_str().unwrap()));
    assert_eq!(
        fixture
            .call(
                &stamp("direct"),
                "consent.get",
                json!({"elicitation_id":"el_1"})
            )
            .await
            .unwrap_err()["code"],
        "elicitation_not_owner"
    );
}

#[tokio::test]
async fn sealed_text_answer_shape_refusals_keep_card_open() {
    let fixture = filed().await;
    let mut bad_inputs = vec![Value::Null, json!({}), json!([]), json!([input(), input()])];
    // Each malformed envelope is submitted to the real handler, not only the decoder.
    for (field, value) in [
        ("id", json!("other")),
        ("enc", json!("AQ")),
        (
            "enc",
            json!(format!("{}=", input()["enc"].as_str().unwrap())),
        ),
        ("ciphertext", json!(URL_SAFE_NO_PAD.encode([1u8; 16]))),
        ("ciphertext", json!(URL_SAFE_NO_PAD.encode([1u8; 81]))),
        ("ciphertext", json!("not+base64")),
        ("extra", json!(true)),
    ] {
        let mut entry = input();
        entry[field] = value;
        bad_inputs.push(json!([entry]));
    }
    for inputs in bad_inputs {
        let mut params = answer(input());
        params["inputs"] = inputs;
        assert_eq!(
            fixture
                .call(&phone::phone_stamp(), "consent.answer", params)
                .await
                .unwrap_err()["code"],
            "elicitation_invalid_answer"
        );
        assert_eq!(filer_read(&fixture).await["state"], "pending");
    }
    assert_eq!(
        fixture
            .call(
                &phone::phone_stamp(),
                "consent.answer",
                json!({"elicitationId":"el_1","choiceId":"allow_once"})
            )
            .await
            .unwrap_err()["code"],
        "elicitation_invalid_answer"
    );
    assert!(fixture.rows()[1].is_empty());
}

#[tokio::test]
async fn sealed_text_decline_has_no_input_and_approval_bounds_count_ciphertext_bytes() {
    let fixture = filed().await;
    let mut decline = answer(input());
    decline["choiceId"] = json!("deny");
    assert_eq!(
        fixture
            .call(&desktop(), "consent.answer", decline)
            .await
            .unwrap_err()["code"],
        "elicitation_invalid_answer"
    );
    let declined = fixture
        .ok(
            &desktop(),
            "consent.answer",
            json!({"elicitationId":"el_1","choiceId":"deny","inputs":[]}),
        )
        .await;
    assert_eq!(declined["ok"], true);
    assert_eq!(declined["elicitation"]["answeredSurface"], "operator_app");
    assert!(sealed_payload(&fixture).is_none());
    assert!(filer_read(&fixture).await.get("input_supplied").is_none());
    for len in [17, 80] {
        let fixture = filed().await;
        let mut supplied = input();
        supplied["ciphertext"] = json!(URL_SAFE_NO_PAD.encode(vec![99u8; len]));
        let response = fixture
            .ok(&desktop(), "consent.answer", answer(supplied))
            .await;
        assert_eq!(response["ok"], true);
        assert_eq!(response["elicitation"]["answeredSurface"], "operator_app");
    }
    let fixture = Fixture::open(None).await;
    fixture.request(&filer(), support::app_body()).await;
    assert_eq!(
        fixture
            .call(&desktop(), "consent.answer", answer(input()))
            .await
            .unwrap_err()["code"],
        "elicitation_invalid_answer"
    );
}

#[tokio::test]
async fn sealed_text_retries_are_exact_and_do_not_write_even_after_release() {
    let fixture = filed().await;
    assert_eq!(approve(&fixture).await["alreadyAnswered"], false);
    let before = fixture.rows();
    let pointers = fixture.payloads();
    assert_eq!(approve(&fixture).await["alreadyAnswered"], true);
    assert_eq!(before, fixture.rows());
    assert_eq!(pointers, fixture.payloads());
    for field in ["enc", "ciphertext"] {
        let mut changed = input();
        changed[field] =
            json!(URL_SAFE_NO_PAD.encode(vec![90u8; if field == "enc" { 32 } else { 37 }]));
        assert_eq!(
            fixture
                .ok(&desktop(), "consent.answer", answer(changed))
                .await["code"],
            "conflict"
        );
    }
    fixture
        .ok(
            &filer(),
            "consent.release_inputs",
            json!({"elicitation_id":"el_1"}),
        )
        .await;
    let before = fixture.rows();
    assert_eq!(approve(&fixture).await["alreadyAnswered"], true);
    assert_eq!(before, fixture.rows());
    assert_eq!(
        fixture
            .ok(
                &desktop(),
                "consent.answer",
                json!({"elicitationId":"el_1","choiceId":"deny"})
            )
            .await["code"],
        "conflict"
    );
}

#[tokio::test]
async fn sealed_text_release_is_owner_only_durable_and_idempotent() {
    let mut fixture = filed().await;
    approve(&fixture).await;
    assert!(sealed_payload(&fixture).is_some());
    for who in [stamp("direct"), phone::phone_stamp(), desktop()] {
        assert!(fixture
            .call(
                &who,
                "consent.release_inputs",
                json!({"elicitation_id":"el_1"})
            )
            .await
            .is_err());
        assert!(sealed_payload(&fixture).is_some());
    }
    for _ in 0..2 {
        assert_eq!(
            fixture
                .ok(
                    &filer(),
                    "consent.release_inputs",
                    json!({"elicitation_id":"el_1"})
                )
                .await,
            json!({"released":true})
        );
        assert!(
            sealed_payload(&fixture).is_none(),
            "release must erase answer bytes"
        );
        let card = filer_read(&fixture).await;
        assert_eq!(card["input_released"], true);
        assert!(card.get("answer_inputs").is_none());
        assert!(card.get("input_supplied").is_none());
    }
    reopen(&mut fixture).await;
    assert_eq!(filer_read(&fixture).await["input_released"], true);
    assert_eq!(approve(&fixture).await["alreadyAnswered"], true);
}

async fn reopen(fixture: &mut Fixture) {
    use ck_cingulate::{Handler, Options};
    use std::path::Path;
    use subc_protocol::ModuleHelloAckBody;
    let path = fixture.sql().path().unwrap().to_owned();
    fixture.handler = Handler::new(Options {
        clock: fixture.clock.clone(),
        sweep_timer: false,
        data_home: Path::new(&path).parent().unwrap().to_str().unwrap().into(),
        ..Options::default()
    });
    for _ in 0..4 {
        tokio::task::yield_now().await;
    }
    fixture
        .handler
        .hello_ack(&ModuleHelloAckBody {
            negotiated_ver: 1,
            subc_ops: vec![],
            subc_capabilities: vec![],
            storage: Some(serde_json::to_value(support::descriptor(Path::new(&path))).unwrap()),
            machine_id: None,
        })
        .await;
    assert_eq!(
        fixture.handler.health_report().status,
        subc_protocol::session::HealthStatus::Ok
    );
}

#[tokio::test]
async fn sealed_text_sweep_releases_on_expiry_withdrawal_and_ten_minute_restart_backstop() {
    for mode in ["expiry", "withdrawal", "ten_minutes", "restart"] {
        let mut fixture = Fixture::open(None).await;
        let mut body = request(&key());
        body.as_object_mut().unwrap().remove("expires_in_ms");
        if mode == "expiry" {
            body["expires_in_ms"] = json!(1000);
        }
        fixture.request(&filer(), body).await;
        approve(&fixture).await;
        assert!(sealed_payload(&fixture).is_some());
        match mode {
            "expiry" => {
                fixture.clock.set(NOW + 1000);
                fixture.handler.timer_tick().await.unwrap();
            }
            "withdrawal" => {
                fixture
                    .ok(
                        &filer(),
                        "consent.withdraw",
                        json!({"elicitation_id":"el_1"}),
                    )
                    .await;
            }
            _ => {
                fixture.clock.set(NOW + 599_999);
                fixture.handler.timer_tick().await.unwrap();
                assert!(sealed_payload(&fixture).is_some());
                fixture.clock.set(NOW + 600_000);
                if mode == "restart" {
                    reopen(&mut fixture).await;
                } else {
                    fixture.handler.timer_tick().await.unwrap();
                }
            }
        }
        assert!(sealed_payload(&fixture).is_none(), "{mode}");
        assert_eq!(filer_read(&fixture).await["input_released"], true, "{mode}");
    }
    let fixture = filed().await;
    fixture.clock.set(NOW + 60_000);
    let expired = approve(&fixture).await;
    assert_eq!(expired["code"], "conflict");
    assert_eq!(filer_read(&fixture).await["state"], "expired");
    assert!(sealed_payload(&fixture).is_none());
}

#[derive(Clone)]
struct LogBuffer(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
impl std::io::Write for LogBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct EmptyPermissions;
#[async_trait::async_trait]
impl ck_cingulate::PermissionsRuntime for EmptyPermissions {
    async fn catalog(&self) -> Result<subc_client_rs::consumer::CatalogList, String> {
        Ok(serde_json::from_value(json!({"generation":1,"modules":[]})).unwrap())
    }
    async fn call(
        &self,
        _: &str,
        _: &str,
        _: Value,
    ) -> Result<Value, ck_cingulate::PermissionsCallError> {
        panic!("empty catalog cannot call a provider")
    }
}

#[tokio::test]
async fn sealed_text_bytes_never_appear_in_notices_logs_lists_or_other_storage() {
    let logs = LogBuffer(Default::default());
    let writer = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .without_time()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || writer.clone())
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("one tracing capture per test binary");
    let fixture = filed().await;
    approve(&fixture).await;
    let mut malformed = answer(input());
    malformed["inputs"][0]["extra"] = json!(true);
    assert!(fixture
        .call(&desktop(), "consent.answer", malformed)
        .await
        .is_err());
    let mut malformed_request = request(&key());
    malformed_request["inputs"][0]["hpke"]["suite"] = json!("bad");
    assert!(fixture
        .call(&filer(), "consent.request", malformed_request)
        .await
        .is_err());
    let pending = fixture
        .ok(&phone::phone_stamp(), "consent.list_pending", json!({}))
        .await;
    fixture
        .handler
        .set_permissions_runtime(std::sync::Arc::new(EmptyPermissions));
    let permissions = fixture
        .ok(&phone::phone_stamp(), "permissions.list_all", json!({}))
        .await;
    let sql = fixture.sql();
    let notices: Vec<String> = sql
        .prepare("SELECT body_json FROM access_notices ORDER BY rowid")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(!notices.is_empty());
    let notice_text = format!("{:?} {:?}", fixture.payloads(), notices);
    assert!(
        !notice_text.contains(&key()),
        "notices must not contain the public key"
    );
    let tables: Vec<String> = sql
        .prepare(
            "SELECT name FROM sqlite_master WHERE type='table' AND name!='elicitation_answers'",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let mut storage = String::new();
    for table in tables {
        let mut query = sql.prepare(&format!("SELECT * FROM \"{table}\"")).unwrap();
        let count = query.column_count();
        let rows = query
            .query_map([], |row| {
                (0..count)
                    .map(|i| row.get::<_, rusqlite::types::Value>(i))
                    .collect::<Result<Vec<_>, _>>()
            })
            .unwrap()
            .map(Result::unwrap)
            .collect::<Vec<_>>();
        storage.push_str(&format!("{rows:?}"));
    }
    let logs = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(
        logs.contains("request refused"),
        "capture must exercise refusal tracing"
    );
    for field in ["enc", "ciphertext"] {
        let input = input();
        let secret = input[field].as_str().unwrap();
        for (name, text) in [
            ("notices", notice_text.clone()),
            ("logs", logs.clone()),
            ("pending list", pending.to_string()),
            ("permissions", permissions.to_string()),
            ("non-answer storage", storage.clone()),
        ] {
            assert!(!text.contains(secret), "sealed bytes leaked into {name}");
        }
    }
}

#[tokio::test]
async fn sealed_text_tagged_vectors_are_actual_handler_bytes() {
    let root = support::corpus(CORPUS);
    let filing: Value =
        serde_json::from_slice(&std::fs::read(root.join("filing-request.json")).unwrap()).unwrap();
    let approve: Value =
        serde_json::from_slice(&std::fs::read(root.join("approve-request.json")).unwrap()).unwrap();
    let files = sealed_text::generate(
        filing["params"]["inputs"][0]["hpke"]["public_key"]
            .as_str()
            .unwrap(),
        approve["params"]["inputs"][0].clone(),
    )
    .await;
    for (name, bytes) in files {
        assert_eq!(std::fs::read(root.join(&name)).unwrap(), bytes, "{name}");
    }
    for line in std::fs::read_to_string(root.join("SHA256SUMS"))
        .unwrap()
        .lines()
    {
        use sha2::{Digest, Sha256};
        let (expected, name) = line.split_once("  ").unwrap();
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(std::fs::read(root.join(name)).unwrap())
            ),
            expected,
            "{name}"
        );
    }
}

#[path = "support/watch_pending.rs"]
#[allow(dead_code)]
mod watch_pending;

#[tokio::test]
async fn sealed_text_watches_preserve_definitions_and_never_stream_sealed_answers() {
    let fixture = Fixture::open(None).await;
    let mut watch = watch_pending::Watch::open(&fixture.handler, &desktop(), json!({}));
    assert_eq!(watch.next_value().await["type"], "reread");
    fixture.request(&filer(), request(&key())).await;
    let item = watch.next_value().await;
    assert_eq!(item["card"]["inputs"], request(&key())["inputs"]);
    approve(&fixture).await;
    let item = watch.next_value().await;
    assert_eq!(item["card"]["input_supplied"], true);
    assert!(!item
        .to_string()
        .contains(input()["ciphertext"].as_str().unwrap()));
    watch.unsubscribe();
    assert!(matches!(
        watch.outcome().await,
        subc_client_rs::HandlerOutcome::Streamed
    ));
}
