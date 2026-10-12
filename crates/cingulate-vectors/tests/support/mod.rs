#![allow(dead_code)]

pub mod worker;

use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use async_trait::async_trait;
use cingulate_store::{Isolation, StorageBackend, StorageDescriptor};
use ck_cingulate::{Clock, Handler, Notifier, Options, ResolvedAgent, Resolver, Stamp};
use rusqlite::{types::Value as SqlValue, Connection};
use serde_json::{json, Value};
use subc_client_rs::HandlerOutcome;
use subc_protocol::{session::HealthStatus, ModuleHelloAckBody};
use tempfile::TempDir;

pub const NOW: i64 = 10_000;
pub const ANSWERED: i64 = 11_000;
pub const REQUESTER_ORDER: [&str; 17] = [
    "request.json",
    "get.json",
    "await.json",
    "elicitation_requester_refused.json",
    "elicitation_invalid_request.json",
    "elicitation_unknown_kind.json",
    "elicitation_fail_closed_required.json",
    "elicitation_unavailable.json",
    "elicitation_not_owner.json",
    "elicitation_invalid_execution.json",
    "elicitation_store_error.json",
    "elicitation_operator_required.json",
    "report_execution.json",
    "answers.json",
    "ack.json",
    "elicitation_not_found.json",
    "withdraw.json",
];

pub fn corpus(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../test-vectors")
        .join(name)
}

pub fn vector(name: &str, file: &str) -> Value {
    let value: Value = serde_json::from_slice(&fs::read(corpus(name).join(file)).unwrap()).unwrap();
    assert_eq!(value["tag"], name, "{file}");
    value
}

pub fn request_body() -> Value {
    vector("elicitation-requester-v1", "request.json")["request"].clone()
}

pub fn app_body() -> Value {
    serde_json::from_slice(
        &fs::read(corpus("elicitation-request-v1").join("valid/app-grant-once.json")).unwrap(),
    )
    .unwrap()
}

pub fn stamp(principal: &str) -> Stamp {
    Stamp {
        principal: Some(principal.into()),
        bind_session: "provider-session".into(),
        ..Stamp::default()
    }
}

pub fn owner() -> Stamp {
    Stamp {
        carried_agent_id: "agent-alf".into(),
        ..stamp("reserved:plexus")
    }
}

pub fn operator() -> Stamp {
    Stamp {
        operator_surface: true,
        ..stamp("reserved:callosum")
    }
}

pub fn route_stamp(request: &Value) -> Stamp {
    match request.get("route") {
        None => owner(),
        Some(Value::Null) => Stamp::default(),
        Some(route) => {
            let principal = route["principal"].as_str().map(str::to_owned);
            Stamp {
                carried_agent_id: if principal.as_deref() == Some("reserved:plexus") {
                    "agent-alf".into()
                } else {
                    String::new()
                },
                principal,
                bind_session: route["session"].as_str().unwrap_or_default().into(),
                operator_surface: route["operator"].as_bool().unwrap_or(false),
                console_machine_id: None,
                ..Default::default()
            }
        }
    }
}

pub fn adapted_method(method: &str) -> String {
    if method == "elicitation.list_pending_for_user" {
        "consent.list_pending".into()
    } else {
        format!(
            "consent.{}",
            method.strip_prefix("elicitation.").expect("elicitation op")
        )
    }
}

pub struct Substitution<'a> {
    pub id: &'a str,
    pub now: i64,
    pub expires: i64,
    pub answered: i64,
}

impl Substitution<'_> {
    pub fn apply(&self, value: &Value) -> Value {
        match value {
            Value::String(text) => match text.as_str() {
                "@ELICITATION_ID@" => json!(self.id),
                "@HEAD_AGENT_ID@" => json!("agent-alf"),
                "@NOW_MS@" => json!(self.now),
                "@EXPIRES_AT_MS@" => json!(self.expires),
                "@ANSWERED_AT_MS@" => json!(self.answered),
                _ => {
                    assert!(!text.starts_with('@'), "unknown placeholder {text}");
                    value.clone()
                }
            },
            Value::Array(values) => Value::Array(values.iter().map(|v| self.apply(v)).collect()),
            Value::Object(object) => Value::Object(
                object
                    .iter()
                    .map(|(k, v)| (k.clone(), self.apply(v)))
                    .collect(),
            ),
            _ => value.clone(),
        }
    }
}

pub fn without_requester_label(mut value: Value) -> Value {
    if let Some(records) = value.get_mut("records") {
        for record in records.as_array_mut().unwrap() {
            record.as_object_mut().unwrap().remove("requester_label");
        }
    } else {
        value.as_object_mut().unwrap().remove("requester_label");
    }
    value
}

pub struct TestClock(AtomicI64);
impl TestClock {
    pub fn set(&self, ms: i64) {
        self.0.store(ms, Ordering::SeqCst);
    }
}
#[async_trait]
impl Clock for TestClock {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
    async fn sleep_until(&self, _: i64) {
        panic!("vector scenarios only use zero-timeout awaits and a stopped sweep timer");
    }
}

struct AlfResolver;
#[async_trait]
impl Resolver for AlfResolver {
    async fn resolve(&self, agent: &str) -> Result<Option<ResolvedAgent>, String> {
        assert_eq!(agent, "agent-alf");
        Ok(Some(ResolvedAgent {
            name: Some("ALF".into()),
            retired: false,
        }))
    }
}

#[derive(Default)]
pub struct Recorder(Mutex<Vec<Value>>);
#[async_trait]
impl Notifier for Recorder {
    async fn notify(&self, pointer: Value) -> Result<(), String> {
        self.0.lock().unwrap().push(pointer);
        Ok(())
    }
}

pub struct Fixture {
    pub handler: Handler,
    pub clock: Arc<TestClock>,
    recorder: Arc<Recorder>,
    path: PathBuf,
    _dir: TempDir,
}

pub fn descriptor(path: &std::path::Path) -> StorageDescriptor {
    StorageDescriptor {
        module_id: "cingulate".into(),
        storage_namespace: "main".into(),
        isolation: Isolation::Module,
        backend: StorageBackend::Sqlite {
            path: path.to_str().unwrap().into(),
        },
    }
}

impl Fixture {
    pub async fn open(machine: Option<&str>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cards.sqlite");
        let clock = Arc::new(TestClock(AtomicI64::new(NOW)));
        let recorder = Arc::new(Recorder::default());
        let handler = Handler::new(Options {
            clock: clock.clone(),
            resolver: Arc::new(AlfResolver),
            notifier: recorder.clone(),
            await_hook: None,
            sweep_timer: false,
            data_home: dir.path().to_str().unwrap().into(),
        });
        handler
            .hello_ack(&ModuleHelloAckBody {
                negotiated_ver: 1,
                subc_ops: vec![],
                subc_capabilities: vec![],
                storage: Some(serde_json::to_value(descriptor(&path)).unwrap()),
                machine_id: machine.map(str::to_owned),
            })
            .await;
        assert_eq!(handler.health_report().status, HealthStatus::Ok);
        Self {
            handler,
            clock,
            recorder,
            path,
            _dir: dir,
        }
    }

    pub fn payloads(&self) -> Vec<Value> {
        self.recorder.0.lock().unwrap().clone()
    }

    pub fn rows(&self) -> Vec<Vec<Vec<SqlValue>>> {
        let db = Connection::open(&self.path).unwrap();
        ["elicitation_records", "elicitation_answers"]
            .into_iter()
            .map(|table| {
                let mut query = db
                    .prepare(&format!("SELECT * FROM {table} ORDER BY elicitation_id"))
                    .unwrap();
                let columns = query.column_count();
                query
                    .query_map([], |row| {
                        (0..columns)
                            .map(|i| row.get(i))
                            .collect::<Result<Vec<SqlValue>, _>>()
                    })
                    .unwrap()
                    .map(Result::unwrap)
                    .collect()
            })
            .collect()
    }

    pub fn stored_record(&self, id: &str) -> cingulate_store::ElicitationRecord {
        let record: String = Connection::open(&self.path)
            .unwrap()
            .query_row(
                "SELECT record_json FROM elicitation_records WHERE elicitation_id=?1",
                [id],
                |row| row.get(0),
            )
            .unwrap();
        serde_json::from_str(&record).unwrap()
    }

    pub async fn call(&self, stamp: &Stamp, method: &str, params: Value) -> Result<Value, Value> {
        let bytes = serde_json::to_vec(&json!({"method":method, "params":params})).unwrap();
        match tokio::time::timeout(
            Duration::from_secs(1),
            self.handler.handle_stamped(stamp, &bytes),
        )
        .await
        .expect("op exceeded 1 s")
        {
            HandlerOutcome::Response(bytes) => {
                let envelope: Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(envelope.as_object().unwrap().len(), 1);
                Ok(envelope.get("result").expect("wrapped result").clone())
            }
            HandlerOutcome::Error { code, message } => Err(json!({"code":code,"message":message})),
            other => panic!("{method}: {other:?}"),
        }
    }

    pub async fn ok(&self, stamp: &Stamp, method: &str, params: Value) -> Value {
        self.call(stamp, method, params)
            .await
            .unwrap_or_else(|error| panic!("{method}: {error}"))
    }

    pub async fn request(&self, stamp: &Stamp, body: Value) -> String {
        let reply = self.ok(stamp, "consent.request", body).await;
        let id = reply["elicitation_id"].as_str().unwrap();
        assert!(id.starts_with("el_"));
        id.into()
    }
}
