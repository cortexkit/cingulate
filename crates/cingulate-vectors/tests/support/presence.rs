use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use ck_cingulate::{Clock, Handler, Options, Stamp, WatchSink};
use cortexkit_presence::{route, Summary};
use serde_json::{json, Value};
use subc_client_rs::{HandlerOutcome, RouteBindRequest, RouteHandle};
use subc_protocol::{
    peer::{OperatorApp, RouteBindPeer},
    BindIdentity, Principal, RouteTarget,
};
use tokio::sync::{mpsc, watch};

const NOW: i64 = 1_700_000_000_000;
pub fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("presence-v1")
}

struct FixedClock(watch::Sender<i64>);
#[async_trait]
impl Clock for FixedClock {
    fn now_ms(&self) -> i64 {
        *self.0.borrow()
    }
    async fn sleep_until(&self, at: i64) {
        let mut tick = self.0.subscribe();
        let _ = tick.wait_for(|now| *now >= at).await;
    }
}

fn stamp(principal: Principal, harness: &str, app: Option<&str>) -> Stamp {
    Stamp::from_bind(
        &RouteBindRequest::new(
            RouteHandle::detached(1, 1),
            RouteTarget::ManagementSurface {
                module_id: "cingulate".into(),
            },
            BindIdentity::new(harness, "presence-vectors"),
        )
        .with_principal(principal)
        .with_peer(
            RouteBindPeer::new().with_operator_app(app.map(|app| OperatorApp::new(app, 123))),
        ),
    )
}
fn reserved(id: &str) -> Principal {
    Principal::Reserved {
        module_id: id.into(),
    }
}

async fn call(h: &Handler, stamp: &Stamp, request: &Value) -> Value {
    match h
        .handle_stamped(stamp, &serde_json::to_vec(request).unwrap())
        .await
    {
        HandlerOutcome::Response(body) => serde_json::from_slice(&body).unwrap(),
        HandlerOutcome::Error { code, message } => json!({"error":{"code":code,"message":message}}),
        other => panic!("unexpected {other:?}"),
    }
}
fn bytes(value: &Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(value).unwrap();
    bytes.push(b'\n');
    bytes
}
fn save(files: &mut BTreeMap<String, Vec<u8>>, name: &str, value: Value) {
    files.insert(format!("{name}.json"), bytes(&value));
}
fn summary(reply: &Value) -> Summary {
    serde_json::from_value(
        json!({"where":reply["result"]["where"],"watching":reply["result"]["watching"]}),
    )
    .unwrap()
}

struct Sink(mpsc::UnboundedSender<Value>);
#[async_trait]
impl WatchSink for Sink {
    async fn emit(&self, item: Vec<u8>) -> Result<(), String> {
        self.0
            .send(serde_json::from_slice(&item).unwrap())
            .map_err(|e| e.to_string())
    }
    async fn ended(&self) {
        self.0.closed().await;
    }
}

pub async fn generate() -> BTreeMap<String, Vec<u8>> {
    let clock = Arc::new(FixedClock(watch::channel(NOW).0));
    let h = Handler::new(Options {
        clock: clock.clone(),
        ..Options::default()
    });
    let core = stamp(reserved("prefrontal-core"), "core", None);
    let phone = stamp(
        reserved("callosum"),
        "fed-phone:0123456789abcdef0123456789abcdef",
        None,
    );
    let desktop = stamp(Principal::Direct, "desktop", Some("alfonso-desktop"));
    // The terminal sample models an explicitly enabled development build.
    let tui = Stamp {
        local_operator: true,
        operator_surface: true,
        ..stamp(Principal::Direct, "terminal", None)
    };
    let mut files = BTreeMap::new();
    let get = json!({"method":"presence.get", "params":{}});
    let unknown = call(&h, &core, &get).await;
    save(&mut files, "where-unknown", unknown.clone());
    let (send, mut frames) = mpsc::unbounded_channel();
    let watcher = h.clone();
    let reader = core.clone();
    let task = tokio::spawn(async move {
        watcher
            .handle_presence_watch_stamped(
                &reader,
                br#"{"method":"presence.watch","params":{}}"#,
                &Sink(send),
            )
            .await
    });
    let opening = frames.recv().await.unwrap();
    for (name, reporter, params) in [
        (
            "report-desktop",
            &desktop,
            json!({"app":"body-cannot-override","device_kind":"phone","state":"active","focus":["agent-a","agent-b"],"shows_inline":["ask","card"],"ttl_ms":1_000}),
        ),
        (
            "report-cktui",
            &tui,
            json!({"app":"cktui","state":"active","focus":["agent-b","agent-c"],"shows_inline":["mention","future_kind"],"ttl_ms":1_000}),
        ),
        (
            "report-phone",
            &phone,
            json!({"device_kind":"computer","app":42,"state":"active","focus":["agent-a"],"shows_inline":[],"ttl_ms":1_000}),
        ),
    ] {
        let request = json!({"method":"presence.report","params":params});
        let response = call(&h, reporter, &request).await;
        save(
            &mut files,
            name,
            json!({"route":{"principal":reporter.principal,"harness":reporter.harness,"operator_app":reporter.operator_app},"request":request,"response":response}),
        );
        if name == "report-cktui" {
            save(&mut files, "where-computer", call(&h, &core, &get).await);
        }
    }
    let merged = call(&h, &core, &get).await;
    save(&mut files, "get-merged", merged.clone());
    save(&mut files, "where-phone", merged.clone());
    let summary = summary(&merged);
    let mut routes = vec![];
    for (kind, agent_id, now_ms) in [
        ("ask", "agent-a", NOW),
        ("mention", "agent-a", NOW),
        ("ask", "agent-a", NOW + 60_001),
    ] {
        routes.push(json!({"summary":summary,"kind":kind,"agent_id":agent_id,"now_ms":now_ms,"deliver":route(&summary,kind,agent_id,now_ms)}));
    }
    let inactive = json!({"method":"presence.report","params":{"state":"locked","ttl_ms":1_000}});
    call(&h, &phone, &inactive).await;
    let result = call(&h, &core, &get).await;
    let computer_summary = self::summary(&result);
    routes.push(json!({"summary":computer_summary,"kind":"other","agent_id":"agent-a","now_ms":NOW,"deliver":route(&computer_summary,"other","agent-a",NOW)}));
    // Restore the phone before observing the coalesced change.
    call(&h, &phone, &json!({"method":"presence.report","params":{"state":"active","focus":["agent-a"],"shows_inline":[],"ttl_ms":1_000}})).await;
    tokio::task::yield_now().await;
    clock.0.send_replace(NOW + 250);
    let changed = frames.recv().await.unwrap();
    clock.0.send_replace(NOW + 1_000);
    tokio::task::yield_now().await;
    clock.0.send_replace(NOW + 1_250);
    let expired = frames.recv().await.unwrap();
    save(
        &mut files,
        "watch-sequence",
        json!({"request":{"method":"presence.watch","params":{}},"frames":[{"at_ms":NOW,"frame":opening},{"at_ms":NOW+250,"frame":changed},{"at_ms":NOW+1_250,"frame":expired}]}),
    );
    h.stop_watches();
    assert_eq!(task.await.unwrap(), HandlerOutcome::Streamed);
    call(&h, &phone, &inactive).await;
    let away = call(&h, &core, &get).await;
    save(&mut files, "where-away", away.clone());
    let away_summary = self::summary(&away);
    routes.push(json!({"summary":away_summary,"kind":"ask","agent_id":"agent-a","now_ms":NOW+1_250,"deliver":route(&away_summary,"ask","agent-a",NOW+1_250)}));
    let unknown_summary = self::summary(&unknown);
    routes.push(json!({"summary":unknown_summary,"kind":"ask","agent_id":"agent-a","now_ms":NOW,"deliver":route(&unknown_summary,"ask","agent-a",NOW)}));
    save(&mut files, "route", json!(routes));
    for (name, field, value) in [
        ("refuse-state", "state", json!("away")),
        ("refuse-focus", "focus", json!(["agent a"])),
        ("refuse-ttl-ms", "ttl_ms", json!(120_001)),
        ("refuse-shows-inline", "shows_inline", json!(["ask-card"])),
    ] {
        let mut params = json!({"app":"cktui","state":"active","ttl_ms":1_000});
        params[field] = value;
        let request = json!({"method":"presence.report","params":params});
        save(
            &mut files,
            name,
            json!({"request":request,"response":call(&h,&tui,&request).await}),
        );
    }
    for (name, reporter, method) in [
        (
            "refuse-device-simulator",
            stamp(
                reserved("callosum"),
                "fed-sim:0123456789abcdef0123456789abcdef",
                None,
            ),
            "presence.report",
        ),
        (
            "refuse-device-computer",
            stamp(
                reserved("callosum"),
                "fed-computer:0123456789abcdef0123456789abcdef",
                None,
            ),
            "presence.report",
        ),
        ("refuse-device-caller", core.clone(), "presence.report"),
        ("refuse-read", phone, "presence.get"),
    ] {
        let request =
            json!({"method":method,"params":{"app":"cktui","state":"active","ttl_ms":1_000}});
        save(
            &mut files,
            name,
            json!({"route":{"principal":reporter.principal,"harness":reporter.harness},"request":request,"response":call(&h,&reporter,&request).await}),
        );
    }
    files
}
