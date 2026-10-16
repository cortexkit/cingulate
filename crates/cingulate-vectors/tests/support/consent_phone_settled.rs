use std::{collections::BTreeMap, time::Duration};

use ck_cingulate::Stamp;
use serde_json::{json, Value};
use subc_client_rs::HandlerOutcome;

use crate::{
    phone,
    support::{stamp, Fixture, ANSWERED, NOW},
};

pub const CORPUS: &str = "consent-phone-settled-v1";

const REQUEST: &[u8] =
    include_bytes!("../../../../test-vectors/consent-package-card-v1/request.json");

fn request_body() -> Value {
    serde_json::from_slice::<Value>(REQUEST).unwrap()["params"].clone()
}

async fn invoke(fixture: &Fixture, route: &Stamp, method: &str, params: Value) -> HandlerOutcome {
    let request = serde_json::to_vec(&json!({"method":method,"params":params})).unwrap();
    tokio::time::timeout(
        Duration::from_secs(1),
        fixture.handler.handle_stamped(route, &request),
    )
    .await
    .expect("settled-phone handler operation exceeded 1 s")
}

fn success(outcome: HandlerOutcome, method: &str) -> Vec<u8> {
    match outcome {
        HandlerOutcome::Response(bytes) => bytes,
        other => panic!("{method}: expected response, got {other:?}"),
    }
}

fn refusal(outcome: HandlerOutcome, method: &str) -> Vec<u8> {
    match outcome {
        HandlerOutcome::Error { code, message } => {
            serde_json::to_vec(&json!({"code":code,"message":message})).unwrap()
        }
        other => panic!("{method}: expected refusal, got {other:?}"),
    }
}

async fn request_card(fixture: &Fixture, core: &Stamp) -> (String, Vec<u8>) {
    let reply = success(
        invoke(fixture, core, "consent.request", request_body()).await,
        "consent.request",
    );
    let id = serde_json::from_slice::<Value>(&reply).unwrap()["result"]["elicitation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(id, "el_1");
    (id, reply)
}

async fn answer(fixture: &Fixture, phone_route: &Stamp, id: &str) {
    let reply = invoke(
        fixture,
        phone_route,
        "consent.answer",
        json!({"elicitationId":id,"choiceId":"allow"}),
    )
    .await;
    assert_eq!(
        serde_json::from_slice::<Value>(&success(reply, "consent.answer")).unwrap()["result"]["ok"],
        true
    );
}

async fn ack(fixture: &Fixture, core: &Stamp) {
    let feed = fixture
        .ok(core, "consent.answers", json!({"since":0}))
        .await;
    assert!(feed["cursor"].as_i64().unwrap() > 0);
    fixture
        .ok(core, "consent.ack", json!({"cursor":feed["cursor"]}))
        .await;
}

async fn phone_get(fixture: &Fixture, route: &Stamp, id: &str) -> Vec<u8> {
    success(
        invoke(fixture, route, "consent.get", json!({"elicitationId":id})).await,
        "consent.get",
    )
}

pub async fn generate() -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let core = stamp("reserved:prefrontal-core");
    let phone_route = phone::phone_stamp();
    files.insert("request.json".into(), REQUEST.to_vec());

    let fixture = Fixture::open(None).await;
    let (id, request_reply) = request_card(&fixture, &core).await;
    files.insert("request-reply.json".into(), request_reply);
    fixture.clock.set(ANSWERED);
    answer(&fixture, &phone_route, &id).await;
    ack(&fixture, &core).await;
    files.insert(
        "answered-after-ack-pending-execution-phone-get.json".into(),
        phone_get(&fixture, &phone_route, &id).await,
    );
    files.insert(
        "answer-acknowledged-conflict.json".into(),
        success(
            invoke(
                &fixture,
                &phone_route,
                "consent.answer",
                json!({"elicitationId":id,"choiceId":"deny"}),
            )
            .await,
            "consent.answer",
        ),
    );

    let fixture = Fixture::open(None).await;
    let (id, _) = request_card(&fixture, &core).await;
    fixture.clock.set(ANSWERED);
    answer(&fixture, &phone_route, &id).await;
    fixture
        .ok(
            &core,
            "consent.report_execution",
            json!({"elicitation_id":id,"execution":"executed"}),
        )
        .await;
    ack(&fixture, &core).await;
    files.insert(
        "answered-after-ack-executed-phone-get.json".into(),
        phone_get(&fixture, &phone_route, &id).await,
    );

    let fixture = Fixture::open(None).await;
    let (id, _) = request_card(&fixture, &core).await;
    fixture.clock.set(ANSWERED);
    answer(&fixture, &phone_route, &id).await;
    fixture
        .ok(
            &core,
            "consent.report_execution",
            json!({"elicitation_id":id,"execution":"approved_not_executed"}),
        )
        .await;
    ack(&fixture, &core).await;
    files.insert(
        "answered-after-ack-not-run-phone-get.json".into(),
        phone_get(&fixture, &phone_route, &id).await,
    );

    let fixture = Fixture::open(None).await;
    let (id, _) = request_card(&fixture, &core).await;
    let expires_at = NOW + request_body()["expires_in_ms"].as_i64().unwrap();
    fixture.clock.set(expires_at);
    ack(&fixture, &core).await;
    files.insert(
        "expired-after-ack-phone-get.json".into(),
        phone_get(&fixture, &phone_route, &id).await,
    );

    let fixture = Fixture::open(None).await;
    let (id, _) = request_card(&fixture, &core).await;
    fixture.clock.set(ANSWERED);
    fixture
        .ok(&core, "consent.withdraw", json!({"elicitation_id":id}))
        .await;
    ack(&fixture, &core).await;
    files.insert(
        "withdrawn-after-ack-phone-get.json".into(),
        phone_get(&fixture, &phone_route, &id).await,
    );

    let fixture = Fixture::open(None).await;
    let (id, _) = request_card(&fixture, &core).await;
    fixture.clock.set(ANSWERED);
    answer(&fixture, &phone_route, &id).await;
    ack(&fixture, &core).await;
    fixture.clock.set(ANSWERED + cingulate_store::RETENTION_MS);
    files.insert(
        "pruned-after-ack-phone-get.json".into(),
        refusal(
            invoke(
                &fixture,
                &phone_route,
                "consent.get",
                json!({"elicitationId":id}),
            )
            .await,
            "consent.get",
        ),
    );
    files
}
