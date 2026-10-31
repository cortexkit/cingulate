use super::*;
use crate::{Clock, Options};
use async_trait::async_trait;
use std::{sync::Arc, time::Duration};
use subc_client_rs::{RouteBindRequest, RouteHandle};
use subc_protocol::{
    peer::{OperatorApp, RouteBindPeer},
    BindIdentity, Principal, RouteTarget,
};
use tokio::{sync::mpsc, time::Instant};

struct TestClock(Instant);
#[async_trait]
impl Clock for TestClock {
    fn now_ms(&self) -> i64 {
        self.0.elapsed().as_millis() as i64
    }
    async fn sleep_until(&self, ms: i64) {
        tokio::time::sleep_until(self.0 + Duration::from_millis(ms as u64)).await;
    }
}

fn handler() -> Handler {
    Handler::new(Options {
        clock: Arc::new(TestClock(Instant::now())),
        ..Options::default()
    })
}

fn bind(principal: Principal, harness: &str) -> RouteBindRequest {
    RouteBindRequest::new(
        RouteHandle::detached(1, 1),
        RouteTarget::ManagementSurface {
            module_id: "cingulate".into(),
        },
        BindIdentity::new(harness, "session"),
    )
    .with_principal(principal)
}

fn local() -> Stamp {
    Stamp::from_bind(&bind(Principal::Direct, "local"))
}
fn core() -> Stamp {
    Stamp::from_principal(Some(&Principal::Reserved {
        module_id: "prefrontal-core".into(),
    }))
}
fn report() -> Value {
    json!({"app":"cktui", "state":"active", "ttl_ms":1_000, "focus":["agent-a"], "shows_inline":["ask"]})
}

#[test]
fn route_identity_and_admission() {
    let h = handler();
    for (harness, allowed) in [
        ("fed-phone:0123456789abcdef0123456789abcdef", true),
        ("fed-phone:bad", false),
        ("fed-sim:0123456789abcdef0123456789abcdef", false),
        ("fed-computer:0123456789abcdef0123456789abcdef", false),
        ("other", false),
    ] {
        let stamp = Stamp::from_bind(&bind(
            Principal::Reserved {
                module_id: "callosum".into(),
            },
            harness,
        ));
        let result = h.presence_report(
            &stamp,
            &json!({"app":42,"device_kind":"computer","state":"active","ttl_ms":1}),
        );
        if allowed {
            assert_eq!(
                result.unwrap()["device_id"],
                "phone:0123456789abcdef0123456789abcdef"
            );
        } else {
            assert_eq!(result.unwrap_err().code, "presence_device_not_allowed");
        }
        assert_eq!(
            stamp.presence_reader().unwrap_err().code,
            "presence_read_not_allowed"
        );
    }
    let req = bind(Principal::Direct, "local").with_peer(
        RouteBindPeer::new().with_operator_app(Some(OperatorApp::new("alfonso-desktop", 5))),
    );
    let verified = Stamp::from_bind_with_local_operator(&req, false);
    assert_eq!(
        h.presence_report(&verified, &report()).unwrap()["device_id"],
        "computer:alfonso-desktop"
    );
    assert!(verified.presence_reader().is_ok());
    assert_eq!(
        h.presence_report(&local(), &report()).unwrap()["device_id"],
        "computer:cktui"
    );
    let disabled = Stamp::from_bind_with_local_operator(&bind(Principal::Direct, "local"), false);
    for stamp in [
        disabled,
        Stamp::default(),
        core(),
        Stamp::from_bind(&bind(Principal::Unverified, "local")),
    ] {
        assert_eq!(
            h.presence_report(&stamp, &report()).unwrap_err().code,
            "presence_device_not_allowed"
        );
    }
    assert!(core().presence_reader().is_ok());
    assert_eq!(
        Stamp::default().presence_reader().unwrap_err().code,
        "presence_read_not_allowed"
    );
}

#[test]
fn validation_rules_and_inactive_drop() {
    let h = handler();
    for (field, invalids) in [
        (
            "state",
            vec![Value::Null, json!("away"), json!("Active"), json!(1)],
        ),
        (
            "app",
            vec![
                Value::Null,
                json!(""),
                json!("A"),
                json!("a_b"),
                json!("a".repeat(33)),
            ],
        ),
        (
            "ttl_ms",
            vec![
                Value::Null,
                json!(0),
                json!(-1),
                json!(120_001),
                json!(1.5),
                json!("1"),
            ],
        ),
        (
            "focus",
            vec![
                Value::Null,
                json!([]),
                json!(["a", "a"]),
                json!([""]),
                json!(["two words"]),
                json!(["é"]),
                json!(["a\n"]),
                json!(["x".repeat(257)]),
                json!([1]),
                json!((0..17).map(|n| format!("a{n}")).collect::<Vec<_>>()),
            ],
        ),
        (
            "shows_inline",
            vec![
                Value::Null,
                json!(["ask", "ask"]),
                json!(["Bad"]),
                json!(["a-b"]),
                json!([""]),
                json!(["x".repeat(33)]),
                json!([1]),
                json!((0..9).map(|n| format!("a{n}")).collect::<Vec<_>>()),
            ],
        ),
    ] {
        for value in invalids {
            let mut params = report();
            params[field] = value;
            let error = h.presence_report(&local(), &params).unwrap_err();
            assert_eq!(error.code, "invalid_params", "{params}");
            assert_eq!(
                serde_json::from_str::<Value>(&error.message).unwrap(),
                json!({"field":field}),
                "{params}"
            );
        }
        let mut missing = report();
        missing.as_object_mut().unwrap().remove(field);
        if matches!(field, "state" | "app" | "ttl_ms") {
            assert!(h.presence_report(&local(), &missing).is_err());
        }
    }
    let mut params = report();
    params["focus"] = json!((0..16).map(|n| format!("a{n}")).collect::<Vec<_>>());
    params["shows_inline"] = json!(["ask", "card", "mention", "future_kind", "a", "b", "c", "d"]);
    params["ttl_ms"] = json!(120_000);
    h.presence_report(&local(), &params).unwrap();
    assert_eq!(h.presence_snapshot().0[0].report.focus.len(), 16);
    for state in ["background", "idle", "locked"] {
        params["state"] = json!(state);
        h.presence_report(&local(), &params).unwrap();
        let devices = h.presence_snapshot().0;
        assert_eq!(devices.len(), 1);
        assert!(devices[0].report.focus.is_empty());
        assert!(devices[0].report.shows_inline.is_empty());
        params["focus"] = json!([""]);
        assert_eq!(
            h.presence_report(&local(), &params).unwrap_err().code,
            "invalid_params"
        );
        params["focus"] = json!(["agent-a"]);
    }
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

async fn advance(ms: u64) {
    tokio::time::advance(Duration::from_millis(ms)).await;
    tokio::task::yield_now().await;
}

#[tokio::test(start_paused = true)]
async fn watch_coalesces_and_emits_on_expiry_without_refresh_spam() {
    let h = handler();
    let (send, mut frames) = mpsc::unbounded_channel();
    let watcher = h.clone();
    let task = tokio::spawn(async move {
        watcher
            .handle_presence_watch_stamped(
                &core(),
                br#"{"method":"presence.watch","params":{}}"#,
                &Sink(send),
            )
            .await
    });
    assert_eq!(
        frames.recv().await.unwrap(),
        json!({"where":"unknown", "watching":[]})
    );
    h.presence_report(&local(), &report()).unwrap();
    tokio::task::yield_now().await;
    advance(100).await;
    let mut params = report();
    params["focus"] = json!(["agent-b"]);
    h.presence_report(&local(), &params).unwrap();
    tokio::task::yield_now().await;
    advance(149).await;
    assert!(frames.try_recv().is_err());
    advance(1).await;
    assert_eq!(
        frames.recv().await.unwrap(),
        json!({"where":"computer","watching":[{"agent_id":"agent-b","shows_inline":["ask"],"seen_at_ms":100}]})
    );
    h.presence_report(&local(), &params).unwrap();
    tokio::task::yield_now().await;
    advance(500).await;
    assert!(
        frames.try_recv().is_err(),
        "timestamp-only refresh must not emit"
    );
    advance(500).await;
    assert!(h.presence_snapshot().0.is_empty());
    advance(249).await;
    assert!(frames.try_recv().is_err());
    advance(1).await;
    assert_eq!(
        frames.recv().await.unwrap(),
        json!({"where":"unknown","watching":[]})
    );
    h.stop_watches();
    assert_eq!(task.await.unwrap(), HandlerOutcome::Streamed);
    assert_eq!(
        handler().presence_snapshot().1.location,
        cingulate_presence::Where::Unknown
    );
}

#[tokio::test]
async fn presence_does_not_wait_for_consent_store() {
    let h = handler();
    let _store = h.inner.store.lock().await;
    h.presence_report(&local(), &report()).unwrap();
    let reply = h
        .dispatch(&core(), "presence.get", json!({}))
        .await
        .unwrap();
    assert_eq!(reply["where"], "computer");
}

#[tokio::test(start_paused = true)]
async fn inline_kind_changes_emit_and_unsubscribe_ends_the_stream() {
    let h = handler();
    h.presence_report(&local(), &report()).unwrap();
    let (send, mut frames) = mpsc::unbounded_channel();
    let watcher = h.clone();
    let task = tokio::spawn(async move {
        watcher
            .handle_presence_watch_stamped(&local(), br#"{"method":"presence.watch"}"#, &Sink(send))
            .await
    });
    assert_eq!(
        frames.recv().await.unwrap()["watching"][0]["shows_inline"],
        json!(["ask"])
    );
    let mut params = report();
    params["shows_inline"] = json!(["card"]);
    h.presence_report(&local(), &params).unwrap();
    tokio::task::yield_now().await;
    advance(250).await;
    let changed = frames.recv().await.unwrap();
    assert_eq!(changed["watching"][0]["agent_id"], "agent-a");
    assert_eq!(changed["watching"][0]["shows_inline"], json!(["card"]));
    drop(frames);
    assert_eq!(task.await.unwrap(), HandlerOutcome::Streamed);
    let (send, _frames) = mpsc::unbounded_channel();
    assert!(
        matches!(h.handle_presence_watch_stamped(&Stamp::default(), br#"{"method":"presence.watch"}"#, &Sink(send)).await,
        HandlerOutcome::Error {code, ..} if code == "presence_read_not_allowed")
    );
}
