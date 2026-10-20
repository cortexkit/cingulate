//! Drives `consent.watch_pending` in-process: a capturing stream sink, and the
//! scenario that generates the `consent-watch-pending-v1` corpus.

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use ck_cingulate::{Handler, Stamp, WatchSink};
use serde_json::{json, Value};
use subc_client_rs::{HandlerOutcome, RouteBindRequest, RouteHandle};
use subc_protocol::{BindIdentity, Principal, RouteTarget};
use tokio::{
    sync::{mpsc, watch, Semaphore},
    task::JoinHandle,
};

use crate::{
    phone,
    support::{app_body, stamp, Fixture, ANSWERED},
};

pub const CORPUS: &str = "consent-watch-pending-v1";
/// The store instance name every vector cursor carries.
pub const INSTANCE: &str = "0123456789abcdef0123456789abcdef";

/// Captures emitted items. `permits` counts emits that may complete; a test
/// that wants a stalled reader starts it low and adds permits later.
pub struct Sink {
    items: mpsc::UnboundedSender<Vec<u8>>,
    unsubscribed: watch::Sender<bool>,
    permits: Semaphore,
}

#[async_trait]
impl WatchSink for Sink {
    async fn emit(&self, item: Vec<u8>) -> Result<(), String> {
        self.permits
            .acquire()
            .await
            .map_err(|error| error.to_string())?
            .forget();
        self.items.send(item).map_err(|error| error.to_string())
    }
    async fn ended(&self) {
        let _ = self
            .unsubscribed
            .subscribe()
            .wait_for(|unsubscribed| *unsubscribed)
            .await;
    }
}

impl Sink {
    /// Let `emits` more items through a stalled reader.
    pub fn release(&self, emits: usize) {
        self.permits.add_permits(emits);
    }
}

pub struct Watch {
    pub sink: Arc<Sink>,
    items: mpsc::UnboundedReceiver<Vec<u8>>,
    task: JoinHandle<HandlerOutcome>,
}

pub fn request(params: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({"method":"consent.watch_pending","params":params})).unwrap()
}

impl Watch {
    pub fn open(handler: &Handler, stamp: &Stamp, params: Value) -> Self {
        Self::open_with_permits(handler, stamp, params, Semaphore::MAX_PERMITS)
    }

    pub fn open_with_permits(
        handler: &Handler,
        stamp: &Stamp,
        params: Value,
        permits: usize,
    ) -> Self {
        let (items, receiver) = mpsc::unbounded_channel();
        let sink = Arc::new(Sink {
            items,
            unsubscribed: watch::channel(false).0,
            permits: Semaphore::new(permits),
        });
        let (handler, stamp, body, task_sink) = (
            handler.clone(),
            stamp.clone(),
            request(params),
            sink.clone(),
        );
        let task = tokio::spawn(async move {
            handler
                .handle_watch_stamped(&stamp, &body, task_sink.as_ref())
                .await
        });
        Self {
            sink,
            items: receiver,
            task,
        }
    }

    /// The next item's exact bytes.
    pub async fn next(&mut self) -> Vec<u8> {
        tokio::time::timeout(Duration::from_secs(5), self.items.recv())
            .await
            .expect("watch item within 5 s")
            .expect("watch stream open")
    }

    pub async fn next_value(&mut self) -> Value {
        serde_json::from_slice(&self.next().await).unwrap()
    }

    /// Assert that no item arrives while the watch is given time to run.
    pub async fn quiet(&mut self) {
        let item = tokio::time::timeout(Duration::from_millis(100), self.items.recv()).await;
        assert!(
            item.is_err(),
            "unexpected item {:?}",
            item.unwrap().map(String::from_utf8)
        );
    }

    pub fn unsubscribe(&self) {
        self.sink.unsubscribed.send_replace(true);
    }

    /// Wait for the watch to end and return its terminal outcome.
    pub async fn outcome(self) -> HandlerOutcome {
        tokio::time::timeout(Duration::from_secs(5), self.task)
            .await
            .expect("watch ended within 5 s")
            .unwrap()
    }

    /// Every item emitted before the stream ended, then the outcome.
    pub async fn drain(mut self) -> (Vec<Value>, HandlerOutcome) {
        let outcome = tokio::time::timeout(Duration::from_secs(5), &mut self.task)
            .await
            .expect("watch ended within 5 s")
            .unwrap();
        let mut items = Vec::new();
        while let Ok(item) = self.items.try_recv() {
            items.push(serde_json::from_slice(&item).unwrap());
        }
        (items, outcome)
    }
}

/// A Callosum route from a paired Mac: not the phone, so refused like
/// consent.list_pending refuses it.
pub fn mac_stamp() -> Stamp {
    Stamp::from_bind(
        &RouteBindRequest::new(
            RouteHandle::detached(1, 1),
            RouteTarget::ManagementSurface {
                module_id: "cingulate".into(),
            },
            BindIdentity::new("/scratch", "fed-mac:vector-device", "ignored-session"),
        )
        .with_principal(Principal::Reserved {
            module_id: "callosum".into(),
        }),
    )
}

pub fn refusal(outcome: HandlerOutcome) -> Vec<u8> {
    match outcome {
        HandlerOutcome::Error { code, message } => {
            serde_json::to_vec(&json!({"code":code,"message":message})).unwrap()
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}

pub fn package_body_with_script(bytes: usize) -> Value {
    let mut body = phone::package_body();
    body["package"]["script"] = json!("x".repeat(bytes));
    body
}

pub async fn open_fixture(change_log_rows: i64) -> Fixture {
    let fixture = Fixture::open_with_change_log(None, change_log_rows).await;
    fixture.pin_store_instance(INSTANCE);
    fixture
}

// Store the untouched item bytes, as emitted on the stream.
pub async fn generate() -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let phone = phone::phone_stamp();
    let requester = stamp("direct");

    // One live watch through a card's whole life.
    let fixture = open_fixture(cingulate_store::CHANGE_LOG_ROWS).await;
    files.insert("live-request.json".into(), request(json!({})));
    let mut live = Watch::open(&fixture.handler, &phone, json!({}));
    files.insert("live-01-reread.json".into(), live.next().await);
    let first = fixture.request(&requester, app_body()).await;
    files.insert("live-02-created.json".into(), live.next().await);
    fixture.clock.set(ANSWERED);
    fixture
        .ok(
            &phone,
            "consent.answer",
            json!({"elicitationId":first,"choiceId":"allow_once"}),
        )
        .await;
    let answered = live.next().await;
    let cursor = serde_json::from_slice::<Value>(&answered).unwrap()["cursor"].clone();
    files.insert("live-03-answered.json".into(), answered);
    let second = fixture.request(&requester, app_body()).await;
    files.insert("live-04-created.json".into(), live.next().await);
    fixture
        .ok(
            &requester,
            "consent.withdraw",
            json!({"elicitation_id":second}),
        )
        .await;
    files.insert("live-05-withdrawn.json".into(), live.next().await);
    // A day after both settled, the next sweep prunes them from the phone's view.
    fixture.clock.set(ANSWERED + cingulate_store::RETENTION_MS);
    fixture.ok(&phone, "consent.list_pending", json!({})).await;
    files.insert("live-06-pruned.json".into(), live.next().await);
    files.insert("live-07-pruned.json".into(), live.next().await);
    live.quiet().await;
    live.unsubscribe();
    assert!(matches!(live.outcome().await, HandlerOutcome::Streamed));

    // Resuming from the answered item's cursor replays only what followed it,
    // one item per card at its latest change, then continues live.
    let replay_params = json!({"cursor":cursor});
    files.insert("replay-request.json".into(), request(replay_params.clone()));
    let mut replay = Watch::open(&fixture.handler, &phone, replay_params);
    files.insert("replay-01.json".into(), replay.next().await);
    files.insert("replay-02.json".into(), replay.next().await);
    replay.quiet().await;
    replay.unsubscribe();
    replay.outcome().await;

    // A cursor older than the retained change log cannot be replayed.
    let fixture = open_fixture(1).await;
    for _ in 0..3 {
        fixture.request(&requester, app_body()).await;
    }
    fixture.ok(&phone, "consent.list_pending", json!({})).await;
    let stale = json!({"cursor":{"instance":INSTANCE,"version":1}});
    files.insert("stale-cursor-request.json".into(), request(stale.clone()));
    let mut watch = Watch::open(&fixture.handler, &phone, stale);
    files.insert("stale-cursor-reread.json".into(), watch.next().await);
    watch.quiet().await;
    watch.unsubscribe();
    watch.outcome().await;

    // A card whose projection exceeds the item limit is sent as a reread.
    let fixture = open_fixture(cingulate_store::CHANGE_LOG_ROWS).await;
    let mut watch = Watch::open(&fixture.handler, &phone, json!({}));
    watch.next().await;
    fixture
        .request(
            &stamp("reserved:prefrontal-core"),
            package_body_with_script(ck_cingulate::ITEM_LIMIT_BYTES),
        )
        .await;
    files.insert("oversized-reread.json".into(), watch.next().await);
    watch.unsubscribe();
    watch.outcome().await;

    // A caller that consent.list_pending refuses is also refused the watch.
    files.insert(
        "refused-reply.json".into(),
        refusal(
            Watch::open(&fixture.handler, &mac_stamp(), json!({}))
                .outcome()
                .await,
        ),
    );
    files
}
