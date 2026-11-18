use super::*;
use std::collections::BTreeSet;

const PHONE: &str = "fed-phone:11111111111111111111111111111111";
const SECRET_FEATURES: [&str; 2] = ["secret-use-approval", "manifest-binding-approval"];
const CONFIG_FEATURE: &str = "config-plan";

fn phone() -> Stamp {
    bound_surface(
        Principal::Reserved {
            module_id: "callosum".into(),
        },
        PHONE,
    )
}

fn desktop() -> Stamp {
    bound_surface(Principal::Direct, "desktop")
}

fn params(vector: &str) -> Value {
    serde_json::from_str::<Value>(vector).unwrap()["params"].take()
}

fn secret_requests() -> [(Stamp, Value); 3] {
    let scoped = Stamp {
        scope: Some(
            serde_json::from_value(json!({
                "owner":{"kind":"reserved","module_id":"prefrontal-core"},
                "ref":"H", "scope_epoch":7, "kind":"head",
                "attributes":{"agent_id":"agent-A"}, "owner_authorized":true
            }))
            .unwrap(),
        ),
        carried_agent_id: "agent-A".into(),
        ..stamp("reserved:aft")
    };
    [
        (
            scoped.clone(),
            params(include_str!(
                "../../../../test-vectors/secret-use-approval-v1/filer-request.json"
            )),
        ),
        (
            stamp("reserved:plexus"),
            params(include_str!(
                "../../../../test-vectors/manifest-binding-v1/filer-request.json"
            )),
        ),
        (
            Stamp {
                principal: Some("reserved:prefrontal-core".into()),
                ..scoped
            },
            params(include_str!(
                "../../../../test-vectors/flow-install-secrets-v1/filing.json"
            )),
        ),
    ]
}

async fn open_watch(
    f: &Fixture,
    stamp: &Stamp,
    params: &Value,
) -> (
    tokio::task::JoinHandle<HandlerOutcome>,
    mpsc::UnboundedReceiver<Value>,
) {
    let (send, mut items) = mpsc::unbounded_channel();
    let handler = f.handler.clone();
    let stamp = stamp.clone();
    let body =
        serde_json::to_vec(&json!({"method":"consent.watch_pending", "params":params})).unwrap();
    let watch = tokio::spawn(async move {
        handler
            .handle_watch_stamped(&stamp, &body, &OperatorWatch(send))
            .await
    });
    assert_eq!(
        next_operator_item(&mut items).await,
        json!({"type":"reread"})
    );
    (watch, items)
}

async fn listed_ids(f: &Fixture, stamp: &Stamp, params: &Value) -> BTreeSet<String> {
    ok(&f.handler, stamp, "consent.list_pending", params.clone()).await["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|card| card["elicitationId"].as_str().unwrap().into())
        .collect()
}

async fn changes_before(items: &mut mpsc::UnboundedReceiver<Value>, last_id: &str) -> Vec<Value> {
    let mut changes = Vec::new();
    loop {
        let item = next_operator_item(items).await;
        match item["type"].as_str() {
            Some("changed") if item["card"]["elicitationId"] == last_id => return changes,
            Some("changed" | "removed") => changes.push(item),
            _ => panic!("unexpected watch item: {item}"),
        }
    }
}

fn item_id(item: &Value) -> &str {
    if item["type"] == "changed" {
        item["card"]["elicitationId"].as_str().unwrap()
    } else {
        item["elicitation_id"].as_str().unwrap()
    }
}

#[tokio::test]
#[cfg(feature = "dev-local-operator")]
async fn desktop_watch_reports_phone_settlements_for_secret_cards() {
    let f = Fixture::open(None).await;
    f.clock.set(1_700_000_000_000);
    let mut pending = Vec::new();
    for (filer, request) in secret_requests() {
        let kind = request["kind"].as_str().unwrap().to_owned();
        pending.push((kind, f.request(&filer, request).await));
    }

    let local = desktop();
    let phone = phone();
    let features = json!({"clientFeatures":SECRET_FEATURES});
    assert_eq!(
        listed_ids(&f, &local, &features).await,
        pending.iter().map(|(_, id)| id.clone()).collect()
    );
    let (watch, mut items) = open_watch(&f, &local, &features).await;
    for (_, id) in &pending {
        let answer = ok(
            &f.handler,
            &phone,
            "consent.answer",
            json!({"elicitationId":id, "choiceId":"decline", "clientFeatures":SECRET_FEATURES}),
        )
        .await;
        assert_eq!(answer["elicitation"]["state"], "answered");
    }
    assert!(listed_ids(&f, &local, &features).await.is_empty());

    // The ordinary card marks the end of the answer changes, even if the watch hides them.
    let last_id = f.request(&carrier(), body()).await;
    let changes = changes_before(&mut items, &last_id).await;
    f.handler.stop_watches();
    assert_eq!(watch.await.unwrap(), HandlerOutcome::Streamed);
    let mut missing = Vec::new();
    for (kind, id) in &pending {
        match changes.iter().find(|item| item_id(item) == id) {
            Some(item) if item["type"] == "changed" => {
                assert_eq!(item["card"]["state"], "answered", "{kind}");
            }
            Some(_) => {}
            None => missing.push(kind),
        }
    }
    assert!(
        missing.is_empty(),
        "missing settlement changes: {missing:?}"
    );
}

#[tokio::test]
async fn pending_watch_and_list_agree_for_each_card_kind_and_route() {
    let f = Fixture::open(None).await;
    f.clock.set(1_700_000_000_000);

    let mut routes = vec![("phone", phone()), ("plain operator", operator())];
    let desktop = desktop();
    if cfg!(feature = "dev-local-operator") {
        routes.push(("development desktop", desktop));
    } else {
        let params =
            json!({"clientFeatures":[SECRET_FEATURES[0], SECRET_FEATURES[1], CONFIG_FEATURE]});
        error(
            &f.handler,
            &desktop,
            "consent.list_pending",
            params.clone(),
            "elicitation_operator_required",
        )
        .await;
        let (send, _items) = mpsc::unbounded_channel();
        let body = serde_json::to_vec(&json!({"method":"consent.watch_pending", "params":params}))
            .unwrap();
        assert!(matches!(
            bounded(f.handler.handle_watch_stamped(&desktop, &body, &OperatorWatch(send))).await,
            HandlerOutcome::Error { code, .. } if code == "elicitation_operator_required"
        ));
    }

    let mut watches = Vec::new();
    for (route, stamp) in &routes {
        for features in [
            &[][..],
            &[SECRET_FEATURES[0]],
            &[SECRET_FEATURES[1]],
            &[CONFIG_FEATURE],
            &[SECRET_FEATURES[0], SECRET_FEATURES[1], CONFIG_FEATURE],
        ] {
            let params = json!({"clientFeatures":features});
            let (watch, items) = open_watch(&f, stamp, &params).await;
            watches.push((route, stamp, features, params, watch, items));
        }
    }

    let flow = stamp("reserved:prefrontal-core");
    let config = params(include_str!(
        "../../../../test-vectors/config-plan-v4/filing.json"
    ));
    let always_visible = BTreeSet::from([
        f.request(&flow, kind_body("flow_install")).await,
        f.request(&flow, package_body()).await,
        f.request(&stamp("reserved:basal"), kind_body("flow_decision"))
            .await,
    ]);
    let config_id = f.request(&stamp("reserved:callosum"), config).await;
    let mut secret_ids = Vec::new();
    for ((filer, request), feature) in secret_requests().into_iter().zip([
        SECRET_FEATURES[0],
        SECRET_FEATURES[1],
        SECRET_FEATURES[0],
    ]) {
        secret_ids.push((f.request(&filer, request).await, feature));
    }
    let last_id = f.request(&carrier(), body()).await;

    let mut tasks = Vec::new();
    for (&route, stamp, features, params, watch, mut items) in watches {
        let mut watched = BTreeSet::from([last_id.clone()]);
        for item in changes_before(&mut items, &last_id).await {
            assert_eq!(item["type"], "changed", "{route}, {features:?}: {item}");
            assert_eq!(item["card"]["state"], "pending");
            watched.insert(item_id(&item).into());
        }
        let listed = listed_ids(&f, stamp, &params).await;
        assert_eq!(watched, listed, "{route}, {features:?}");

        let mut expected = always_visible.clone();
        expected.insert(last_id.clone());
        if route == "phone" && features.contains(&CONFIG_FEATURE) {
            expected.insert(config_id.clone());
        }
        if route != "plain operator" {
            expected.extend(
                secret_ids
                    .iter()
                    .filter(|(_, feature)| features.contains(feature))
                    .map(|(id, _)| id.clone()),
            );
        }
        assert_eq!(listed, expected, "{route}, {features:?}");
        tasks.push(watch);
    }
    f.handler.stop_watches();
    for watch in tasks {
        assert_eq!(watch.await.unwrap(), HandlerOutcome::Streamed);
    }
}
