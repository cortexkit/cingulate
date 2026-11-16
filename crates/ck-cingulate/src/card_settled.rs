//! Durable feed wake-ups. The bus never participates in admitting or settling a card.

use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use async_trait::async_trait;
use cingulate_store::{CardSettled, ConsentStore, CARD_SETTLED_READERS};
use cortexkit_bus_client::{BusClient, BusClientOptions};
use cortexkit_bus_naming::AccountNames;
use serde_json::{json, Value};

use crate::{DaemonAccessRuntime, Handler};

pub const EVENT: &str = "card_settled";
pub const VERSION: u32 = 1;
const TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Default)]
struct Retry {
    attempts: u32,
    outage: bool,
}

impl Retry {
    fn failed(&mut self) -> (Duration, bool) {
        let log = !self.outage;
        self.outage = true;
        let delay = cingulate_store::maintenance_backoff_ms(self.attempts);
        self.attempts = self.attempts.saturating_add(1);
        (Duration::from_millis(delay as u64), log)
    }
}

/// The same naming constructor and arguments used by BusClient::publish_event.
/// Exposed for checksum-pinned wire samples; it never accepts card content.
pub fn subject(account: &str, row: &CardSettled) -> String {
    AccountNames::derive(account)
        .expect("valid account")
        .event_subject_with_reader(crate::MODULE_ID, EVENT, VERSION, &row.reader)
        .expect("valid reader")
}

#[derive(Default)]
struct Counts {
    pending: AtomicU64,
    published: AtomicU64,
    dropped: AtomicU64,
}

#[derive(Default)]
pub(crate) struct Metrics {
    connected: AtomicBool,
    readers: [Counts; 5],
}

impl Metrics {
    pub(crate) fn refresh(&self, store: &ConsentStore) {
        if let Ok(counts) = store.card_settled_counts() {
            for (target, count) in self.readers.iter().zip(counts) {
                target.pending.store(count.pending, Ordering::Relaxed);
                target.published.store(count.published, Ordering::Relaxed);
                target.dropped.store(count.dropped, Ordering::Relaxed);
            }
        }
    }

    pub(crate) fn json(&self) -> Value {
        let mut metrics = serde_json::Map::new();
        metrics.insert(
            "bus_connected".into(),
            json!(u8::from(self.connected.load(Ordering::Relaxed))),
        );
        for (reader, count) in CARD_SETTLED_READERS.iter().zip(&self.readers) {
            for (kind, value) in [
                ("pending", &count.pending),
                ("published", &count.published),
                ("dropped", &count.dropped),
            ] {
                metrics.insert(
                    format!("bus_{reader}_{kind}"),
                    json!(value.load(Ordering::Relaxed)),
                );
            }
        }
        Value::Object(metrics)
    }
}

/// Success means the server acknowledged storage, not just a socket write.
#[async_trait]
trait Publisher: Send + Sync {
    async fn publish(&self, row: &CardSettled) -> Result<(), ()>;
    fn connected(&self) -> bool;
}

#[async_trait]
impl Publisher for BusClient {
    async fn publish(&self, row: &CardSettled) -> Result<(), ()> {
        let receipt = self
            .publish_event(EVENT, VERSION, Some(&row.reader), &row.msg_id(), row.body())
            .await
            .map_err(|_| ())?;
        // A duplicate is also an acknowledgement: a crash may have happened
        // after server storage but before our local outbox deletion committed.
        let _duplicate = receipt.duplicate();
        Ok(())
    }

    fn connected(&self) -> bool {
        matches!(
            self.connection().client().connection_state(),
            async_nats::connection::State::Connected
        ) && self.connection_changes().has_changed().is_ok()
    }
}

#[async_trait]
trait Connector: Send + Sync {
    async fn connect(&self) -> Result<Box<dyn Publisher>, ()>;
}

#[async_trait]
impl Connector for DaemonAccessRuntime {
    async fn connect(&self) -> Result<Box<dyn Publisher>, ()> {
        let consumer = self.consumer().await.map_err(|_| ())?;
        BusClient::connect(
            consumer,
            BusClientOptions::new(crate::MODULE_ID).map_err(|_| ())?,
        )
        .await
        .map(|client| Box::new(client) as Box<dyn Publisher>)
        .map_err(|_| ())
    }
}

impl Handler {
    /// Starts independently of store startup and all request handling. Credentials
    /// are obtained by BusClient through `ckbus.credential` and `ckbus.nonce_sign`
    /// calls on the shared daemon consumer, never from local credential files.
    pub fn start_bus_publisher(&self, runtime: Arc<DaemonAccessRuntime>) {
        self.start_publisher(runtime);
    }

    fn start_publisher(&self, connector: Arc<dyn Connector>) {
        let mut task = self.inner.bus_task.lock().expect("bus task lock");
        if task.is_some() {
            return;
        }
        let weak = Arc::downgrade(&self.inner);
        let mut stopping = self.inner.stopping.subscribe();
        let mut changes = self.inner.changes.subscribe();
        let handle = tokio::spawn(async move {
            let mut publisher: Option<Box<dyn Publisher>> = None;
            let mut retry = Retry::default();
            loop {
                // Consume only writes preceding this pass. A settlement made
                // during the pass must still wake the next read of the outbox.
                changes.borrow_and_update();
                let work = async {
                    let Some(inner) = weak.upgrade() else {
                        return Err(());
                    };
                    let handler = Handler { inner };
                    // Refresh even when connecting fails, so outage backlog is visible.
                    handler.refresh_bus_metrics().await;
                    if publisher.is_none() {
                        publisher = Some(
                            tokio::time::timeout(TIMEOUT, connector.connect())
                                .await
                                .map_err(|_| ())??,
                        );
                    }
                    let bus = publisher.as_ref().expect("connected publisher");
                    if !bus.connected() {
                        return Err(());
                    }
                    handler
                        .inner
                        .bus_metrics
                        .connected
                        .store(true, Ordering::Relaxed);
                    handler.publish_pending(bus.as_ref()).await
                };
                let result = tokio::select! {
                    _ = stopping.changed() => return,
                    result = work => result,
                };
                let (delay, wait_for_changes) = match result {
                    Ok(full_page) => {
                        retry = Retry::default();
                        if full_page {
                            continue;
                        }
                        // Store writes normally wake an idle publisher. The
                        // backstop also covers a write path missing its tick.
                        (Duration::from_secs(30), true)
                    }
                    Err(()) => {
                        if let Some(inner) = weak.upgrade() {
                            inner.bus_metrics.connected.store(false, Ordering::Relaxed);
                        } else {
                            return;
                        }
                        publisher = None;
                        let (delay, log) = retry.failed();
                        if log {
                            tracing::warn!(
                                "card-settled bus unavailable; durable pointers will retry"
                            );
                        }
                        (delay, false)
                    }
                };
                tokio::select! {
                    _ = stopping.changed() => return,
                    result = changes.changed(), if wait_for_changes => {
                        if result.is_err() { return; }
                    },
                    _ = tokio::time::sleep(delay) => {},
                }
            }
        });
        *task = Some(handle.abort_handle());
    }

    async fn refresh_bus_metrics(&self) {
        let guard = self.inner.store.lock().await;
        if let Some(store) = guard.as_ref() {
            self.inner.bus_metrics.refresh(store);
        }
    }

    async fn publish_pending(&self, publisher: &dyn Publisher) -> Result<bool, ()> {
        let rows = {
            let guard = self.inner.store.lock().await;
            let Some(store) = guard.as_ref() else {
                return Ok(false);
            };
            store.card_settled_pending(64).map_err(|_| ())?
        };
        let full_page = rows.len() == 64;
        for row in rows {
            tokio::time::timeout(TIMEOUT, publisher.publish(&row))
                .await
                .map_err(|_| ())??;
            // The store lock is never held across bus I/O. A failed publish or
            // timeout leaves the row intact; ack loss retries with the same id.
            self.with_store(|store| store.card_settled_published(&row))
                .await
                .map_err(|_| ())?;
        }
        self.refresh_bus_metrics().await;
        Ok(full_page)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Options, Subject};
    use cingulate_store::RequestIdentity;
    use tokio::sync::{Notify, Semaphore};

    async fn setup() -> (tempfile::TempDir, Handler) {
        let dir = tempfile::tempdir().unwrap();
        let handler = Handler::new(Options {
            sweep_timer: false,
            data_home: dir.path().to_str().unwrap().into(),
            ..Options::default()
        });
        let descriptor = cingulate_store::storage_descriptor(None, dir.path().to_str().unwrap());
        handler.try_open(&descriptor).await.unwrap();
        (dir, handler)
    }

    async fn settle(handler: &Handler) -> String {
        handler.with_store(settle_store).await.unwrap()
    }

    fn settle_store(store: &ConsentStore) -> Result<String, cingulate_store::Error> {
        let request = cingulate_decode::ConsentRequest::decode(
            serde_json::from_str(include_str!(
                "../../../test-vectors/elicitation-request-v1/valid/app-grant-once.json"
            ))
            .unwrap(),
        )
        .unwrap();
        let id = store
            .create(
                &RequestIdentity {
                    owner: r#"["reserved:aft","stamped"]"#.into(),
                    requester: "reserved:aft".into(),
                    subject: Subject {
                        agent_id: None,
                        head_agent_id: String::new(),
                        label: "private".into(),
                        via_worker: false,
                        federated: None,
                    },
                    resolved_name: None,
                    asker: None,
                    head: None,
                },
                request,
                100,
            )?
            .record
            .elicitation_id;
        store.answer(&id, "allow_once", None, 101)?;
        Ok(id)
    }

    struct FakePublisher {
        calls: std::sync::Mutex<Vec<CardSettled>>,
        fail: AtomicBool,
        entered: Notify,
        ack: Semaphore,
    }

    #[async_trait]
    impl Publisher for FakePublisher {
        async fn publish(&self, row: &CardSettled) -> Result<(), ()> {
            self.calls.lock().unwrap().push(row.clone());
            self.entered.notify_one();
            if self.fail.load(Ordering::Relaxed) {
                return Err(());
            }
            self.ack.acquire().await.unwrap().forget();
            Ok(())
        }
        fn connected(&self) -> bool {
            true
        }
    }

    fn fake(fail: bool) -> Arc<FakePublisher> {
        Arc::new(FakePublisher {
            calls: Default::default(),
            fail: AtomicBool::new(fail),
            entered: Notify::new(),
            ack: Semaphore::new(0),
        })
    }

    #[tokio::test]
    async fn card_settled_publish_marks_only_after_ack_and_retries_same_id() {
        let (_dir, handler) = setup().await;
        settle(&handler).await;
        let bus = fake(true);
        assert!(handler.publish_pending(bus.as_ref()).await.is_err());
        bus.entered.notified().await;
        assert_eq!(
            handler.health_report().metrics.unwrap()["bus_aft_pending"],
            1
        );
        assert_eq!(
            handler
                .with_store(|s| s.card_settled_pending(64))
                .await
                .unwrap()
                .len(),
            2
        );
        bus.fail.store(false, Ordering::Relaxed);
        let task = tokio::spawn({
            let handler = handler.clone();
            let bus = bus.clone();
            async move { handler.publish_pending(bus.as_ref()).await }
        });
        bus.entered.notified().await;
        assert_eq!(
            handler
                .with_store(|s| s.card_settled_pending(64))
                .await
                .unwrap()
                .len(),
            2,
            "unacknowledged rows stay pending"
        );
        assert_eq!(
            handler.health_report().metrics.unwrap()["bus_aft_published"],
            0
        );
        bus.ack.add_permits(2);
        task.await.unwrap().unwrap();
        let calls = bus.calls.lock().unwrap();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0], calls[1], "retry preserves id and bytes");
        let metrics = handler.health_report().metrics.unwrap();
        assert_eq!(metrics["bus_aft_pending"], 0);
        assert_eq!(metrics["bus_aft_published"], 1);
        assert_eq!(metrics["bus_prefrontal-core_published"], 1);
    }

    struct AbsentBus {
        attempts: AtomicU64,
    }

    struct SharedPublisher(Arc<FakePublisher>);
    #[async_trait]
    impl Publisher for SharedPublisher {
        async fn publish(&self, row: &CardSettled) -> Result<(), ()> {
            self.0.publish(row).await
        }
        fn connected(&self) -> bool {
            true
        }
    }

    struct FakeConnector(Arc<FakePublisher>);
    #[async_trait]
    impl Connector for FakeConnector {
        async fn connect(&self) -> Result<Box<dyn Publisher>, ()> {
            Ok(Box::new(SharedPublisher(self.0.clone())))
        }
    }

    #[tokio::test(start_paused = true)]
    async fn card_settled_task_retries_failed_publish_and_recovers_metrics() {
        let (_dir, handler) = setup().await;
        settle(&handler).await;
        let bus = fake(true);
        handler.start_publisher(Arc::new(FakeConnector(bus.clone())));
        bus.entered.notified().await;
        tokio::task::yield_now().await;
        assert_eq!(handler.health_report().metrics.unwrap()["bus_connected"], 0);
        bus.fail.store(false, Ordering::Relaxed);
        bus.ack.add_permits(2);
        tokio::time::advance(Duration::from_millis(250)).await;
        tokio::task::yield_now().await;
        let metrics = handler.health_report().metrics.unwrap();
        assert_eq!(metrics["bus_connected"], 1);
        assert_eq!(metrics["bus_aft_pending"], 0);
        assert_eq!(metrics["bus_aft_published"], 1);
        assert_eq!(bus.calls.lock().unwrap().len(), 3);
        handler.inner.stopping.send_replace(true);
        tokio::task::yield_now().await;
    }

    #[tokio::test(start_paused = true)]
    async fn card_settled_idle_wakes_on_store_write_without_backstop() {
        let (_dir, handler) = setup().await;
        let bus = fake(false);
        bus.ack.add_permits(2);
        handler.start_publisher(Arc::new(FakeConnector(bus.clone())));
        tokio::task::yield_now().await;
        assert_eq!(handler.health_report().metrics.unwrap()["bus_connected"], 1);
        assert!(bus.calls.lock().unwrap().is_empty());

        let before = tokio::time::Instant::now();
        settle(&handler).await;
        tokio::time::timeout(Duration::from_millis(1), bus.entered.notified())
            .await
            .expect("the store-write notification must wake the idle publisher");
        tokio::task::yield_now().await;
        assert_eq!(tokio::time::Instant::now(), before, "no timer elapsed");
        assert_eq!(bus.calls.lock().unwrap().len(), 2);
        assert_eq!(
            handler.health_report().metrics.unwrap()["bus_aft_pending"],
            0
        );
        handler.inner.stopping.send_replace(true);
        tokio::task::yield_now().await;
    }

    #[tokio::test(start_paused = true)]
    async fn card_settled_write_during_publish_wakes_the_next_pass() {
        let (_dir, handler) = setup().await;
        settle(&handler).await;
        let bus = fake(false);
        handler.start_publisher(Arc::new(FakeConnector(bus.clone())));
        bus.entered.notified().await;

        // The first page has already been read, but its first ack is withheld.
        let before = tokio::time::Instant::now();
        settle(&handler).await;
        bus.ack.add_permits(4);
        tokio::time::timeout(Duration::from_millis(1), async {
            while bus.calls.lock().unwrap().len() < 4 {
                bus.entered.notified().await;
            }
        })
        .await
        .expect("a write during a publish must not lose its wake-up");
        assert_eq!(tokio::time::Instant::now(), before);
        assert_eq!(
            handler.health_report().metrics.unwrap()["bus_aft_pending"],
            0
        );
        handler.inner.stopping.send_replace(true);
        tokio::task::yield_now().await;
    }

    #[tokio::test(start_paused = true)]
    async fn card_settled_idle_backstop_finds_a_write_without_a_tick() {
        let (_dir, handler) = setup().await;
        let bus = fake(false);
        bus.ack.add_permits(2);
        handler.start_publisher(Arc::new(FakeConnector(bus.clone())));
        tokio::task::yield_now().await;
        assert_eq!(handler.health_report().metrics.unwrap()["bus_connected"], 1);

        // Bypass with_store and its notification to model a missing write tick.
        {
            let guard = handler.inner.store.lock().await;
            settle_store(guard.as_ref().unwrap()).unwrap();
        }
        tokio::time::advance(Duration::from_secs(29)).await;
        tokio::task::yield_now().await;
        assert!(bus.calls.lock().unwrap().is_empty(), "no idle polling");
        tokio::time::advance(Duration::from_secs(1)).await;
        tokio::task::yield_now().await;
        assert_eq!(bus.calls.lock().unwrap().len(), 2);
        handler.inner.stopping.send_replace(true);
        tokio::task::yield_now().await;
    }

    #[tokio::test(start_paused = true)]
    async fn card_settled_full_page_reports_more_work_and_drains_immediately() {
        let (_dir, handler) = setup().await;
        for _ in 0..33 {
            settle(&handler).await;
        }
        let bus = fake(false);
        bus.ack.add_permits(66);
        assert!(handler.publish_pending(bus.as_ref()).await.unwrap());
        assert_eq!(bus.calls.lock().unwrap().len(), 64);
        assert!(!handler.publish_pending(bus.as_ref()).await.unwrap());
        assert_eq!(bus.calls.lock().unwrap().len(), 66);

        for _ in 0..33 {
            settle(&handler).await;
        }
        bus.calls.lock().unwrap().clear();
        bus.ack.add_permits(66);
        let before = tokio::time::Instant::now();
        handler.start_publisher(Arc::new(FakeConnector(bus.clone())));
        tokio::time::timeout(Duration::from_millis(1), async {
            while bus.calls.lock().unwrap().len() < 66 {
                bus.entered.notified().await;
            }
        })
        .await
        .expect("full pages must drain without an idle wait");
        assert_eq!(bus.calls.lock().unwrap().len(), 66);
        assert_eq!(tokio::time::Instant::now(), before);
        handler.inner.stopping.send_replace(true);
        tokio::task::yield_now().await;
    }

    #[tokio::test(start_paused = true)]
    async fn card_settled_publish_timeout_keeps_row_pending() {
        let (_dir, handler) = setup().await;
        settle(&handler).await;
        let bus = fake(false);
        assert!(handler.publish_pending(bus.as_ref()).await.is_err());
        assert_eq!(
            handler
                .with_store(|s| s.card_settled_pending(64))
                .await
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            handler.health_report().metrics.unwrap()["bus_aft_published"],
            0
        );
    }

    #[test]
    fn card_settled_logs_once_per_outage_not_per_retry() {
        let mut retry = Retry::default();
        assert_eq!(retry.failed(), (Duration::from_millis(250), true));
        assert_eq!(retry.failed(), (Duration::from_millis(500), false));
        for _ in 0..30 {
            let (delay, log) = retry.failed();
            assert!(!log, "only the first failure logs");
            assert!(delay <= Duration::from_secs(60));
        }
        assert_eq!(retry.failed(), (Duration::from_secs(60), false));
        // A successful drain resets the outage and retry delay together.
        retry = Retry::default();
        assert_eq!(retry.failed(), (Duration::from_millis(250), true));
    }
    #[async_trait]
    impl Connector for AbsentBus {
        async fn connect(&self) -> Result<Box<dyn Publisher>, ()> {
            self.attempts.fetch_add(1, Ordering::Relaxed);
            Err(())
        }
    }

    #[tokio::test(start_paused = true)]
    async fn card_settled_absent_bus_never_blocks_startup_or_settle_and_backs_off() {
        let (_dir, handler) = setup().await;
        let bus = Arc::new(AbsentBus {
            attempts: AtomicU64::new(0),
        });
        handler.start_publisher(bus.clone());
        tokio::task::yield_now().await;
        assert_eq!(bus.attempts.load(Ordering::Relaxed), 1);
        tokio::time::advance(Duration::from_millis(249)).await;
        assert_eq!(bus.attempts.load(Ordering::Relaxed), 1);
        tokio::time::advance(Duration::from_millis(1)).await;
        tokio::task::yield_now().await;
        assert_eq!(bus.attempts.load(Ordering::Relaxed), 2);
        tokio::time::advance(Duration::from_millis(499)).await;
        assert_eq!(bus.attempts.load(Ordering::Relaxed), 2);
        // Both operations succeed with the connector repeatedly refusing.
        let id = tokio::time::timeout(Duration::from_millis(1), settle(&handler))
            .await
            .unwrap();
        assert!(!id.is_empty());
        assert_eq!(
            handler.health_report().status,
            subc_protocol::session::HealthStatus::Ok
        );
        let metrics = handler.health_report().metrics.unwrap();
        assert_eq!(metrics["bus_connected"], 0);
        assert_eq!(metrics["bus_aft_pending"], 1);
        assert_eq!(metrics["bus_aft_dropped"], 0);
        handler.inner.stopping.send_replace(true);
        tokio::task::yield_now().await;
    }
}
