//! `consent.watch_pending`: a live stream of phone-visible card changes,
//! replacing the phone's polling of `consent.list_pending`.
//!
//! The stream is pulled from the store's change log (one row per committed
//! change, written by triggers in the same transaction). A watch keeps only its
//! position in that log, so a slow reader never makes the module buffer
//! changes; the log itself is bounded by compaction.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use async_trait::async_trait;
use cingulate_decode::Refusal;
use cingulate_store::ChangePage;
use serde::Deserialize;
use serde_json::{json, Value};
use subc_client_rs::{HandlerOutcome, RequestCtx};
use tokio::sync::Semaphore;

use crate::{feature_declared, invalid, operator, option_group_declared, Handler, Stamp};

/// The watch operation's name, declared with watch delivery in the manifest.
pub const WATCH_PENDING: &str = "consent.watch_pending";

/// Open watches per process; one more is refused with `watch_capacity`.
pub const MAX_WATCHES: usize = 16;

/// Largest encoded stream item. A card whose projection would exceed it is
/// replaced by a reread item, and the phone reads it through
/// `consent.list_pending`, which leaves out package bodies.
pub const ITEM_LIMIT_BYTES: usize = 192 * 1024;

/// Most changed cards a watch will replay. A cursor further behind than this
/// gets a reread instead of a long replay, and a live watch that falls this far
/// behind (its reader stalled) sends a reread and ends.
pub const MAX_BACKLOG: i64 = 256;

/// Change-log rows read per store visit.
const BATCH: usize = 64;

/// Where a watch's items go. The production sink is the request's stream; tests
/// supply their own to capture items and to end the stream.
#[async_trait]
pub trait WatchSink: Send + Sync {
    /// Send one encoded stream item. An error means the stream is gone.
    async fn emit(&self, item: Vec<u8>) -> Result<(), String>;
    /// Completes when the caller unsubscribes or the route closes.
    async fn ended(&self);
}

#[async_trait]
impl WatchSink for RequestCtx {
    async fn emit(&self, item: Vec<u8>) -> Result<(), String> {
        RequestCtx::emit(self, item)
            .await
            .map_err(|error| error.to_string())
    }
    async fn ended(&self) {
        self.cancelled().await;
    }
}

/// The cursor every changed or removed item carries, echoed back unchanged by
/// the phone. `instance` names the store file and `version` is the item's
/// position in that store's change log.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    instance: String,
    version: i64,
}

pub(crate) fn watch_slots() -> Arc<Semaphore> {
    Arc::new(Semaphore::new(MAX_WATCHES))
}

/// An error that stopped a watch's stream, with the card being projected when
/// it happened, if any, for the log.
struct Failure {
    card: Option<String>,
    refusal: Refusal,
}

impl From<Refusal> for Failure {
    fn from(refusal: Refusal) -> Self {
        Self {
            card: None,
            refusal,
        }
    }
}

/// Send one item, recording in `emitted` that the stream has started. Returns
/// false when the stream is gone.
async fn send(sink: &dyn WatchSink, emitted: &AtomicBool, item: Vec<u8>) -> bool {
    let sent = sink.emit(item).await.is_ok();
    if sent {
        emitted.store(true, Ordering::Release);
    }
    sent
}

fn reread() -> Vec<u8> {
    serde_json::to_vec(&json!({"type":"reread"})).expect("reread serialization")
}

impl Handler {
    /// End every open watch and refuse new ones. Called when the daemon starts
    /// draining ck-cingulate and when its connection ends.
    pub fn stop_watches(&self) {
        self.inner.stopping.send_replace(true);
    }

    /// Number of watches currently open.
    pub fn open_watches(&self) -> usize {
        MAX_WATCHES - self.inner.watch_slots.available_permits()
    }

    /// Wake open watches if the store connection wrote anything since the
    /// last check. Called after every use of the store.
    pub(crate) fn note_writes(&self, store: &cingulate_store::ConsentStore) {
        let count = store.write_count();
        if self.inner.writes.swap(count, Ordering::AcqRel) != count {
            self.inner.changes.send_replace(());
        }
    }

    /// Serve one `consent.watch_pending` request to the end of its stream.
    /// Refusals before the first item are returned as an error reply. Once an
    /// item has been sent the stream always ends with `Streamed`: an error
    /// after that point is logged and ends the stream with a final reread.
    pub async fn handle_watch_stamped(
        &self,
        stamp: &Stamp,
        body: &[u8],
        sink: &dyn WatchSink,
    ) -> HandlerOutcome {
        match self.watch(stamp, body, sink).await {
            Ok(()) => HandlerOutcome::Streamed,
            Err(error) => HandlerOutcome::Error {
                code: error.code.into(),
                message: error.message,
            },
        }
    }

    async fn watch(&self, stamp: &Stamp, body: &[u8], sink: &dyn WatchSink) -> Result<(), Refusal> {
        if body.len() > 1_310_720 {
            return Err(Refusal::new(
                "elicitation_request_too_large",
                "consent envelope exceeds 1310720 bytes",
            ));
        }
        #[derive(Deserialize)]
        struct WireRequest {
            method: String,
            #[serde(default)]
            params: Value,
        }
        let request = serde_json::from_slice::<WireRequest>(body)
            .map_err(|error| Refusal::new("invalid_request", error.to_string()))?;
        if request.method != WATCH_PENDING {
            return Err(Refusal::new(
                "unknown_method",
                format!("{} is not a watch operation", request.method),
            ));
        }
        let params = request.params;
        // Admit exactly as consent.list_pending does: an open store, then the
        // phone's operator route, then a well-formed clientFeatures.
        self.available()?;
        operator(stamp)?;
        option_group_declared(&params)?;
        // A missing, malformed or foreign cursor is not an error: the stream
        // starts with a reread, as it does with no cursor at all.
        let cursor = params
            .get("cursor")
            .filter(|cursor| !cursor.is_null())
            .and_then(|cursor| serde_json::from_value::<Cursor>(cursor.clone()).ok())
            .filter(|cursor| cursor.version >= 0);
        let mut stopping = self.inner.stopping.subscribe();
        if *stopping.borrow() {
            return Err(Refusal::new(
                "consent_unavailable",
                "consent module is stopping",
            ));
        }
        let _slot = self
            .inner
            .watch_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                Refusal::new(
                    "watch_capacity",
                    format!("at most {MAX_WATCHES} watches may be open"),
                )
            })?;
        // Set once the first item reaches the sink. From then on the reply is
        // the stream itself, so nothing may end it with an error reply.
        let emitted = AtomicBool::new(false);
        // Unsubscribe and route close end the stream with no final item:
        // nobody is left to read one. Stopping ends it with no final item
        // either: the change log is durable, so the phone's last cursor still
        // replays after the restart and a reread would only cost it a list read.
        let result = tokio::select! {
            biased;
            _ = sink.ended() => Ok(()),
            _ = stopping.wait_for(|stopping| *stopping) => Ok(()),
            result = self.stream(stamp, &params, cursor, sink, &emitted) => result,
        };
        match result {
            Ok(()) => Ok(()),
            Err(failure) if emitted.load(Ordering::Acquire) => {
                // The phone already holds items from this stream. Ending it with
                // a reread tells the phone to read its list again, which covers
                // whatever this watch could not deliver.
                tracing::warn!(
                    card = failure.card.as_deref().unwrap_or("none"),
                    code = failure.refusal.code,
                    error = %failure.refusal.message,
                    "consent.watch_pending ended by an error after streaming; sent reread"
                );
                let _ = sink.emit(reread()).await;
                Ok(())
            }
            Err(failure) => Err(failure.refusal),
        }
    }

    async fn changes_after(&self, version: i64) -> Result<ChangePage, Refusal> {
        self.with_store(|store| store.changes_after(version, BATCH))
            .await
    }

    async fn stream(
        &self,
        stamp: &Stamp,
        params: &Value,
        cursor: Option<Cursor>,
        sink: &dyn WatchSink,
        emitted: &AtomicBool,
    ) -> Result<(), Failure> {
        // Subscribe before the first read, so a change committed between that
        // read and the first wait still wakes this watch.
        let mut wake = self.inner.changes.subscribe();
        wake.borrow_and_update();
        let first = self
            .changes_after(cursor.as_ref().map_or(i64::MAX, |cursor| cursor.version))
            .await?;
        let replayable = cursor.as_ref().is_some_and(|cursor| {
            cursor.instance == first.instance
                && cursor.version >= first.pruned_through
                && cursor.version <= first.head
                && first.backlog <= MAX_BACKLOG
        });
        let instance = first.instance.clone();
        let (mut position, mut page) = match cursor {
            Some(cursor) if replayable => (cursor.version, Some(first)),
            _ => {
                // On a reread the phone fetches consent.list_pending, and it
                // does so only after this item arrives. Every change after the
                // head read above is streamed, so no change can fall between
                // that head and the phone's list.
                if !send(sink, emitted, reread()).await {
                    return Ok(());
                }
                (first.head, None)
            }
        };
        loop {
            let page = match page.take() {
                Some(page) => page,
                None => {
                    let page = self.changes_after(position).await?;
                    if position < page.pruned_through || page.backlog > MAX_BACKLOG {
                        // The reader fell further behind than a watch replays.
                        send(sink, emitted, reread()).await;
                        return Ok(());
                    }
                    page
                }
            };
            let full = page.changes.len() == BATCH;
            for change in page.changes {
                // Advance over hidden changes without emitting even a removal id.
                position = change.version;
                let hidden = change.rich_ui_device.as_deref().is_some_and(|device| {
                    stamp.answering_device() != Some(device)
                        || !feature_declared(params, "rich-ui-approval")
                });
                if hidden {
                    continue;
                }
                let item = self
                    .watch_item(
                        stamp,
                        params,
                        &instance,
                        change.version,
                        &change.elicitation_id,
                    )
                    .await
                    .map_err(|refusal| Failure {
                        card: Some(change.elicitation_id.clone()),
                        refusal,
                    })?;
                if !send(sink, emitted, item).await {
                    return Ok(());
                }
            }
            if full {
                continue;
            }
            if wake.changed().await.is_err() {
                return Ok(());
            }
            wake.borrow_and_update();
        }
    }

    /// One stream item for a card that changed at `version`. The card is
    /// projected by the operator `consent.get` path itself, so a changed
    /// item's card is byte for byte what the phone would read there.
    async fn watch_item(
        &self,
        stamp: &Stamp,
        params: &Value,
        instance: &str,
        version: i64,
        id: &str,
    ) -> Result<Vec<u8>, Refusal> {
        let mut get = json!({"elicitationId": id});
        if let Some(features) = params.get("clientFeatures") {
            get["clientFeatures"] = features.clone();
        }
        let cursor = json!({"instance": instance, "version": version});
        let item = match self.dispatch(stamp, "consent.get", get).await {
            Ok(card) => json!({"type":"changed","cursor":cursor,"card":card}),
            Err(error) if error.code == "elicitation_not_found" => {
                json!({"type":"removed","cursor":cursor,"elicitation_id":id})
            }
            Err(error) => return Err(error),
        };
        let item = serde_json::to_vec(&item).map_err(|error| invalid(error.to_string()))?;
        Ok(if item.len() > ITEM_LIMIT_BYTES {
            reread()
        } else {
            item
        })
    }
}
