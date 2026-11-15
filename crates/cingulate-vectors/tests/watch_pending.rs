#[path = "support/phone.rs"]
#[allow(dead_code)]
mod phone;
mod support;
#[path = "support/watch_pending.rs"]
mod watch_pending;

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    time::SystemTime,
};

use ck_cingulate::{Stamp, MAX_BACKLOG, MAX_WATCHES};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use subc_client_rs::{ConnectionEnd, HandlerOutcome, ModuleHandler, RouteCloseReason};
use support::{app_body, stamp, Fixture, ANSWERED};
use watch_pending::{mac_stamp, open_fixture, package_body_with_script, Watch, INSTANCE};

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn version(item: &Value) -> i64 {
    item["cursor"]["version"].as_i64().expect("item version")
}

fn card_id(item: &Value) -> String {
    match item["type"].as_str() {
        Some("changed") => item["card"]["elicitationId"].as_str().unwrap().into(),
        Some("removed") => item["elicitation_id"].as_str().unwrap().into(),
        other => panic!("not a card item: {other:?}"),
    }
}

fn is_reread(item: &Value) -> bool {
    *item == json!({"type":"reread"})
}

async fn answer(fixture: &Fixture, id: &str) {
    fixture
        .ok(
            &phone::phone_stamp(),
            "consent.answer",
            json!({"elicitationId":id,"choiceId":"allow_once"}),
        )
        .await;
}

/// Open a watch without a cursor and consume its leading reread.
async fn live(fixture: &Fixture) -> Watch {
    let mut watch = Watch::open(&fixture.handler, &phone::phone_stamp(), json!({}));
    assert!(is_reread(&watch.next_value().await));
    watch
}

async fn close(watch: Watch) {
    watch.unsubscribe();
    assert!(matches!(watch.outcome().await, HandlerOutcome::Streamed));
}

#[tokio::test]
async fn watch_pending_vectors_regenerate_byte_for_byte() {
    let root = support::corpus(watch_pending::CORPUS);
    let mut generated = watch_pending::generate().await;
    generated.insert(
        "README.md".into(),
        fs::read(root.join("README.md")).unwrap(),
    );
    let sums: String = generated
        .iter()
        .map(|(name, bytes)| format!("{}  {name}\n", sha256(bytes)))
        .collect();
    generated.insert("SHA256SUMS".into(), sums.into_bytes());
    let actual: BTreeSet<_> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(
        actual,
        generated.keys().cloned().collect(),
        "corpus file coverage"
    );
    for (name, bytes) in generated {
        assert_eq!(
            fs::read(root.join(&name)).unwrap(),
            bytes,
            "served vector differs: {name}"
        );
    }
}

/// The phone pins this corpus. Its manifest hash is fixed here, and every file
/// except the manifest itself must be listed with its exact digest.
#[test]
fn watch_pending_manifest_is_pinned_and_covers_exact_bytes() {
    let root = support::corpus(watch_pending::CORPUS);
    let manifest = fs::read(root.join("SHA256SUMS")).unwrap();
    assert_eq!(
        sha256(&manifest),
        "034af23b7f1787a528e12ee10ea7b87f71b59f35b1322704fff65bdb5019e6ee",
        "consent-watch-pending-v1 manifest pin"
    );
    let mut listed = BTreeSet::new();
    for line in std::str::from_utf8(&manifest).unwrap().lines() {
        let (digest, path) = line.split_once("  ").unwrap();
        assert!(!path.contains('/') && path != "SHA256SUMS");
        assert!(listed.insert(path.to_owned()), "duplicate path {path}");
        assert_eq!(
            sha256(&fs::read(root.join(path)).unwrap()),
            digest,
            "{path}"
        );
    }
    let actual: BTreeSet<String> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|path| path != "SHA256SUMS")
        .collect();
    assert_eq!(
        actual, listed,
        "manifest must cover every file except itself"
    );
}

#[test]
fn manifest_declares_card_and_presence_watches_with_watch_delivery() {
    let manifest = ck_cingulate::manifest();
    assert!(manifest.is_watch_operation("consent.watch_pending"));
    for (name, _) in ck_cingulate::OPERATIONS {
        if name == "presence.watch" {
            assert!(manifest.is_watch_operation(name));
        } else if name != "consent.watch_pending" {
            assert!(!manifest.is_watch_operation(name), "{name}");
        }
    }
}

#[tokio::test]
async fn missing_or_unusable_cursors_start_with_a_reread() {
    let fixture = open_fixture(cingulate_store::CHANGE_LOG_ROWS).await;
    let id = fixture.request(&stamp("direct"), app_body()).await;
    let mut watch = live(&fixture).await;
    answer(&fixture, &id).await;
    let head = version(&watch.next_value().await);
    close(watch).await;
    for params in [
        json!({}),
        json!({"cursor":null}),
        json!({"cursor":"opaque"}),
        json!({"cursor":head}),
        json!({"cursor":{"version":head}}),
        json!({"cursor":{"instance":INSTANCE}}),
        json!({"cursor":{"instance":INSTANCE,"version":-1}}),
        json!({"cursor":{"instance":INSTANCE,"version":head,"extra":true}}),
        // Another store instance, and a version this store never reached.
        json!({"cursor":{"instance":"another-store","version":head}}),
        json!({"cursor":{"instance":INSTANCE,"version":head + 1}}),
    ] {
        let mut watch = Watch::open(&fixture.handler, &phone::phone_stamp(), params.clone());
        assert!(is_reread(&watch.next_value().await), "{params}");
        // Nothing older than the watch is replayed after the reread.
        watch.quiet().await;
        close(watch).await;
    }
}

#[tokio::test]
async fn valid_cursor_replays_only_changes_after_it_in_order() {
    let fixture = open_fixture(cingulate_store::CHANGE_LOG_ROWS).await;
    let mut watch = live(&fixture).await;
    let mut items = Vec::new();
    for _ in 0..4 {
        fixture.request(&stamp("direct"), app_body()).await;
        items.push(watch.next().await);
    }
    let parsed: Vec<Value> = items
        .iter()
        .map(|item| serde_json::from_slice(item).unwrap())
        .collect();
    close(watch).await;
    let mut replay = Watch::open(
        &fixture.handler,
        &phone::phone_stamp(),
        json!({"cursor":parsed[1]["cursor"]}),
    );
    // Exactly the two changes after the cursor, in their original order and
    // with their original bytes.
    assert_eq!(replay.next().await, items[2]);
    assert_eq!(replay.next().await, items[3]);
    replay.quiet().await;
    // Then the watch continues live.
    answer(&fixture, &card_id(&parsed[0])).await;
    let next = replay.next_value().await;
    assert_eq!(card_id(&next), card_id(&parsed[0]));
    assert_eq!(next["card"]["state"], "answered");
    assert!(version(&next) > version(&parsed[3]));
    close(replay).await;
}

#[tokio::test]
async fn replay_folds_repeated_changes_to_one_item_per_card_at_its_latest_version() {
    let fixture = open_fixture(cingulate_store::CHANGE_LOG_ROWS).await;
    let requester = stamp("direct");
    let mut watch = live(&fixture).await;
    let first = fixture.request(&requester, app_body()).await;
    let cursor = watch.next_value().await["cursor"].clone();
    let second = fixture.request(&requester, app_body()).await;
    let second_created = watch.next_value().await;
    answer(&fixture, &first).await;
    let first_answered = watch.next_value().await;
    fixture
        .ok(
            &requester,
            "consent.withdraw",
            json!({"elicitation_id":second}),
        )
        .await;
    let second_withdrawn = watch.next_value().await;
    close(watch).await;
    assert!(version(&second_created) < version(&first_answered));
    let mut replay = Watch::open(
        &fixture.handler,
        &phone::phone_stamp(),
        json!({"cursor":cursor}),
    );
    assert_eq!(replay.next_value().await, first_answered);
    assert_eq!(replay.next_value().await, second_withdrawn);
    replay.quiet().await;
    close(replay).await;
}

#[tokio::test]
async fn applying_an_item_twice_is_harmless() {
    // The phone keeps, per card, the item with the highest version it has
    // seen. Duplicated and replayed items must leave that state unchanged.
    fn apply(model: &mut BTreeMap<String, (i64, Value)>, item: &Value) {
        let (id, version) = (card_id(item), version(item));
        if model.get(&id).is_none_or(|(seen, _)| *seen < version) {
            model.insert(id, (version, item.clone()));
        }
    }
    let fixture = open_fixture(cingulate_store::CHANGE_LOG_ROWS).await;
    let requester = stamp("direct");
    let mut watch = live(&fixture).await;
    let mut items = Vec::new();
    let first = fixture.request(&requester, app_body()).await;
    items.push(watch.next_value().await);
    let second = fixture.request(&requester, app_body()).await;
    items.push(watch.next_value().await);
    answer(&fixture, &first).await;
    items.push(watch.next_value().await);
    fixture
        .ok(
            &requester,
            "consent.withdraw",
            json!({"elicitation_id":second}),
        )
        .await;
    items.push(watch.next_value().await);
    close(watch).await;
    let versions: Vec<_> = items.iter().map(version).collect();
    assert!(
        versions.windows(2).all(|pair| pair[0] < pair[1]),
        "{versions:?}"
    );
    let mut once = BTreeMap::new();
    for item in &items {
        apply(&mut once, item);
    }
    // Replaying from the first cursor delivers items the phone already
    // applied; replaying the same cursor twice yields identical bytes.
    let cursor = json!({"cursor":items[0]["cursor"]});
    let mut replays = Vec::new();
    for _ in 0..2 {
        let mut replay = Watch::open(&fixture.handler, &phone::phone_stamp(), cursor.clone());
        let replayed = vec![replay.next().await, replay.next().await];
        replay.quiet().await;
        close(replay).await;
        replays.push(replayed);
    }
    assert_eq!(replays[0], replays[1]);
    let mut twice = once.clone();
    for item in items.iter().chain(&items) {
        apply(&mut twice, item);
    }
    for item in &replays[0] {
        apply(&mut twice, &serde_json::from_slice(item).unwrap());
    }
    assert_eq!(twice, once);
    // Resuming from the newest item has nothing to replay.
    let mut resumed = Watch::open(
        &fixture.handler,
        &phone::phone_stamp(),
        json!({"cursor":items[3]["cursor"]}),
    );
    resumed.quiet().await;
    close(resumed).await;
}

#[tokio::test]
async fn every_card_lifecycle_change_streams_one_item() {
    let fixture = open_fixture(cingulate_store::CHANGE_LOG_ROWS).await;
    let requester = stamp("direct");
    let mut watch = live(&fixture).await;

    let answered = fixture.request(&requester, app_body()).await;
    let item = watch.next_value().await;
    assert_eq!(
        (item["type"].clone(), item["card"]["state"].clone()),
        (json!("changed"), json!("pending"))
    );
    fixture.clock.set(ANSWERED);
    answer(&fixture, &answered).await;
    let item = watch.next_value().await;
    assert_eq!(item["card"]["state"], "answered");
    assert_eq!(item["card"]["answeredChoiceId"], "allow_once");
    fixture
        .ok(
            &requester,
            "consent.report_execution",
            json!({"elicitation_id":answered,"execution":"approved_not_executed","reason":"user_cancelled"}),
        )
        .await;
    let item = watch.next_value().await;
    assert_eq!(item["card"]["execution"], "approved_not_executed");
    assert_eq!(item["card"]["executionReason"], "user_cancelled");

    let withdrawn = fixture.request(&requester, app_body()).await;
    assert_eq!(card_id(&watch.next_value().await), withdrawn);
    fixture
        .ok(
            &requester,
            "consent.withdraw",
            json!({"elicitation_id":withdrawn}),
        )
        .await;
    let item = watch.next_value().await;
    assert_eq!(
        (card_id(&item), item["card"]["state"].clone()),
        (withdrawn.clone(), json!("withdrawn"))
    );

    let expired = fixture.request(&requester, app_body()).await;
    assert_eq!(card_id(&watch.next_value().await), expired);
    let expiry = ANSWERED + app_body()["expires_in_ms"].as_i64().unwrap();
    fixture.clock.set(expiry);
    fixture
        .ok(&phone::phone_stamp(), "consent.list_pending", json!({}))
        .await;
    let item = watch.next_value().await;
    assert_eq!(
        (card_id(&item), item["card"]["state"].clone()),
        (expired.clone(), json!("expired"))
    );

    // Retention prunes all three settled cards from the phone's view.
    fixture.clock.set(expiry + cingulate_store::RETENTION_MS);
    fixture
        .ok(&phone::phone_stamp(), "consent.list_pending", json!({}))
        .await;
    let mut pruned = BTreeSet::new();
    for _ in 0..3 {
        let item = watch.next_value().await;
        assert_eq!(item["type"], "removed", "{item}");
        assert_eq!(item.as_object().unwrap().len(), 3, "{item}");
        pruned.insert(card_id(&item));
    }
    assert_eq!(pruned, BTreeSet::from([answered, withdrawn, expired]));
    watch.quiet().await;
    close(watch).await;
}

#[tokio::test]
async fn changed_card_equals_the_phone_get_projection_byte_for_byte() {
    let fixture = open_fixture(cingulate_store::CHANGE_LOG_ROWS).await;
    let requester = stamp("reserved:prefrontal-core");
    for params in [json!({}), json!({"clientFeatures":["option-group"]})] {
        let mut watch = Watch::open(&fixture.handler, &phone::phone_stamp(), params.clone());
        assert!(is_reread(&watch.next_value().await));
        // A package card carries its bodies on get; the watch must too.
        let id = fixture.request(&requester, phone::package_body()).await;
        let pending = watch.next().await;
        let mut get = json!({"elicitationId":id});
        if let Some(features) = params.get("clientFeatures") {
            get["clientFeatures"] = features.clone();
        }
        let expect = |reply: Vec<u8>, item: &[u8]| {
            let card = serde_json::to_vec(&serde_json::from_slice::<Value>(item).unwrap()["card"])
                .unwrap();
            assert_eq!(
                String::from_utf8(reply).unwrap(),
                format!("{{\"result\":{}}}", String::from_utf8(card).unwrap())
            );
        };
        expect(
            phone::served(&fixture, "consent.get", get.clone()).await,
            &pending,
        );
        fixture
            .ok(
                &phone::phone_stamp(),
                "consent.answer",
                json!({"elicitationId":id,"choiceId":"allow","clientFeatures":["option-group"]}),
            )
            .await;
        let answered = watch.next().await;
        expect(phone::served(&fixture, "consent.get", get).await, &answered);
        close(watch).await;
    }
}

#[tokio::test]
async fn oversized_card_is_sent_as_a_reread_and_the_watch_continues() {
    let fixture = open_fixture(cingulate_store::CHANGE_LOG_ROWS).await;
    let mut watch = live(&fixture).await;
    let large = fixture
        .request(
            &stamp("reserved:prefrontal-core"),
            package_body_with_script(ck_cingulate::ITEM_LIMIT_BYTES),
        )
        .await;
    assert!(is_reread(&watch.next_value().await));
    // A card just under the limit is still sent whole.
    let small = fixture
        .request(&stamp("reserved:prefrontal-core"), {
            let mut body = package_body_with_script(1);
            body["dedup_key"] = json!("package:small");
            body
        })
        .await;
    let item = watch.next().await;
    assert!(item.len() <= ck_cingulate::ITEM_LIMIT_BYTES);
    let item: Value = serde_json::from_slice(&item).unwrap();
    assert_eq!(card_id(&item), small);
    assert_ne!(small, large);
    close(watch).await;
}

#[tokio::test]
async fn cursor_older_than_the_compacted_log_gets_a_reread() {
    let fixture = open_fixture(2).await;
    let mut watch = live(&fixture).await;
    fixture.request(&stamp("direct"), app_body()).await;
    let oldest = watch.next_value().await;
    close(watch).await;
    for _ in 0..3 {
        fixture.request(&stamp("direct"), app_body()).await;
    }
    // The cursor still replays until a sweep compacts the log past it.
    let mut replay = Watch::open(
        &fixture.handler,
        &phone::phone_stamp(),
        json!({"cursor":oldest["cursor"]}),
    );
    assert_eq!(replay.next_value().await["type"], "changed");
    close(replay).await;
    fixture
        .ok(&phone::phone_stamp(), "consent.list_pending", json!({}))
        .await;
    let mut stale = Watch::open(
        &fixture.handler,
        &phone::phone_stamp(),
        json!({"cursor":oldest["cursor"]}),
    );
    assert!(is_reread(&stale.next_value().await));
    stale.quiet().await;
    close(stale).await;
}

#[tokio::test]
async fn callers_list_pending_refuses_are_refused_the_watch_the_same_way() {
    let fixture = open_fixture(cingulate_store::CHANGE_LOG_ROWS).await;
    let sim = {
        let mut sim = mac_stamp();
        sim.operator_surface = false;
        sim.sim_surface = true;
        sim
    };
    for caller in [
        mac_stamp(),
        sim,
        stamp("direct"),
        stamp("reserved:plexus"),
        stamp("reserved:prefrontal-core"),
        Stamp::default(),
    ] {
        let list = fixture
            .call(&caller, "consent.list_pending", json!({}))
            .await
            .unwrap_err();
        let watch = Watch::open(&fixture.handler, &caller, json!({}))
            .outcome()
            .await;
        assert_eq!(
            serde_json::from_slice::<Value>(&watch_pending::refusal(watch)).unwrap(),
            list
        );
        assert_eq!(list["code"], "elicitation_operator_required");
        assert_eq!(
            fixture
                .call(&caller, "consent.watch_pending", json!({}))
                .await
                .unwrap_err(),
            list
        );
    }
    // The phone may open the watch as a stream, but a plain request/response
    // call to consent.watch_pending is refused as invalid.
    assert_eq!(
        fixture
            .call(&phone::phone_stamp(), "consent.watch_pending", json!({}))
            .await
            .unwrap_err()["code"],
        "elicitation_invalid_request"
    );
    assert_eq!(fixture.handler.open_watches(), 0);
}

#[tokio::test]
async fn unsubscribing_ends_the_stream_and_frees_its_slot() {
    let fixture = open_fixture(cingulate_store::CHANGE_LOG_ROWS).await;
    let watch = live(&fixture).await;
    assert_eq!(fixture.handler.open_watches(), 1);
    close(watch).await;
    assert_eq!(fixture.handler.open_watches(), 0);
}

#[tokio::test]
async fn watches_beyond_the_cap_are_refused_with_watch_capacity() {
    let fixture = open_fixture(cingulate_store::CHANGE_LOG_ROWS).await;
    let mut open = Vec::new();
    for _ in 0..MAX_WATCHES {
        open.push(live(&fixture).await);
    }
    assert_eq!(fixture.handler.open_watches(), MAX_WATCHES);
    let refused = Watch::open(&fixture.handler, &phone::phone_stamp(), json!({}))
        .outcome()
        .await;
    assert_eq!(
        refused,
        HandlerOutcome::Error {
            code: "watch_capacity".into(),
            message: "at most 16 watches may be open".into()
        }
    );
    close(open.pop().unwrap()).await;
    open.push(live(&fixture).await);
    for watch in open {
        close(watch).await;
    }
}

#[tokio::test]
async fn module_drain_ends_open_watches_and_refuses_new_ones() {
    let fixture = open_fixture(cingulate_store::CHANGE_LOG_ROWS).await;
    let watch = live(&fixture).await;
    fixture
        .handler
        .on_draining(RouteCloseReason::Restart, SystemTime::now())
        .await;
    let (items, outcome) = watch.drain().await;
    assert!(items.is_empty());
    assert_eq!(outcome, HandlerOutcome::Streamed);
    assert_eq!(fixture.handler.open_watches(), 0);
    let refused = Watch::open(&fixture.handler, &phone::phone_stamp(), json!({}))
        .outcome()
        .await;
    assert_eq!(
        refused,
        HandlerOutcome::Error {
            code: "consent_unavailable".into(),
            message: "consent module is stopping".into()
        }
    );
}

#[tokio::test]
async fn connection_end_ends_open_watches() {
    let fixture = open_fixture(cingulate_store::CHANGE_LOG_ROWS).await;
    let watch = live(&fixture).await;
    fixture
        .handler
        .on_connection_end(ConnectionEnd::Goodbye)
        .await;
    assert_eq!(watch.outcome().await, HandlerOutcome::Streamed);
}

#[tokio::test]
async fn stalled_reader_that_falls_too_far_behind_gets_a_reread_and_ends() {
    let fixture = open_fixture(cingulate_store::CHANGE_LOG_ROWS).await;
    // One permit: the leading reread is delivered, then the reader stalls.
    let mut watch = Watch::open_with_permits(&fixture.handler, &phone::phone_stamp(), json!({}), 1);
    assert!(is_reread(&watch.next_value().await));
    // The stalled watch may already hold one read of the log (at most 64
    // changes) when it blocks, so create enough that the rest still exceed
    // the backlog bound once it resumes.
    let cards = usize::try_from(MAX_BACKLOG).unwrap() + 64 + 2;
    for _ in 0..cards {
        fixture.request(&stamp("direct"), app_body()).await;
    }
    watch.sink.release(cards + 1);
    let (items, outcome) = watch.drain().await;
    assert_eq!(outcome, HandlerOutcome::Streamed);
    assert!(items.len() < cards, "{} items", items.len());
    assert!(is_reread(items.last().expect("closing reread")));
    assert!(items[..items.len() - 1]
        .iter()
        .all(|item| item["type"] == "changed"));
    assert_eq!(fixture.handler.open_watches(), 0);
}

#[tokio::test]
async fn store_failure_after_the_first_item_ends_with_a_reread_not_an_error() {
    let fixture = open_fixture(cingulate_store::CHANGE_LOG_ROWS).await;
    let watch = live(&fixture).await;
    // Break the change log's bookkeeping table. Card writes still succeed, but
    // the watch's next read of the log fails.
    fixture
        .sql()
        .execute_batch("DROP TABLE cingulate_change_log")
        .unwrap();
    fixture.request(&stamp("direct"), app_body()).await;
    let (items, outcome) = watch.drain().await;
    assert_eq!(items, [json!({"type":"reread"})]);
    assert_eq!(outcome, HandlerOutcome::Streamed);
    assert_eq!(fixture.handler.open_watches(), 0);
}

#[tokio::test]
async fn card_projection_failure_after_the_first_item_ends_with_a_reread_not_an_error() {
    let fixture = open_fixture(cingulate_store::CHANGE_LOG_ROWS).await;
    let watch = live(&fixture).await;
    // A card whose stored record cannot be decoded makes consent.get fail
    // with a store error rather than not-found.
    fixture
        .sql()
        .execute(
            "INSERT INTO elicitation_records(elicitation_id,owner,record_json,state) \
             VALUES ('el_broken','[]','not json','pending')",
            [],
        )
        .unwrap();
    // A write through the module wakes the watch, which reads the broken card
    // first.
    fixture.request(&stamp("direct"), app_body()).await;
    let (items, outcome) = watch.drain().await;
    assert_eq!(items, [json!({"type":"reread"})]);
    assert_eq!(outcome, HandlerOutcome::Streamed);
    assert_eq!(
        fixture
            .call(
                &phone::phone_stamp(),
                "consent.get",
                json!({"elicitationId":"el_broken"})
            )
            .await
            .unwrap_err()["code"],
        "elicitation_store_error"
    );
}

#[tokio::test]
async fn failure_before_the_first_item_stays_an_error_reply() {
    let fixture = open_fixture(cingulate_store::CHANGE_LOG_ROWS).await;
    fixture
        .sql()
        .execute_batch("DROP TABLE cingulate_change_log")
        .unwrap();
    let outcome = Watch::open(&fixture.handler, &phone::phone_stamp(), json!({}))
        .outcome()
        .await;
    assert!(
        matches!(&outcome, HandlerOutcome::Error { code, .. } if code == "elicitation_store_error"),
        "{outcome:?}"
    );
}
