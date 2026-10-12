use std::{collections::BTreeMap, time::Duration};

use ck_cingulate::Stamp;
use serde_json::{json, Value};
use subc_client_rs::HandlerOutcome;

use crate::{
    phone,
    support::{stamp, Fixture, ANSWERED, NOW},
};

pub const CORPUS: &str = "consent-requester-v1";
pub const DECISIONS: [(&str, &[u8]); 3] = [
    (
        "reconcile",
        include_bytes!("../fixtures/flow-decision-card-v2/reconcile-request.json"),
    ),
    (
        "reenable",
        include_bytes!("../fixtures/flow-decision-card-v2/reenable-request.json"),
    ),
    (
        "grant_lost",
        include_bytes!("../fixtures/flow-decision-card-v2/grant_lost-request.json"),
    ),
];

pub fn decision_body(name: &str) -> Value {
    serde_json::from_slice(
        DECISIONS
            .iter()
            .find(|(decision, _)| *decision == name)
            .unwrap()
            .1,
    )
    .unwrap()
}

async fn capture(
    fixture: &Fixture,
    requester: &Stamp,
    method: &str,
    params: Value,
) -> (Vec<u8>, Vec<u8>) {
    let request = serde_json::to_vec(&json!({"method":method,"params":params})).unwrap();
    let response = tokio::time::timeout(
        Duration::from_secs(1),
        fixture.handler.handle_stamped(requester, &request),
    )
    .await
    .expect("requester op exceeded 1 s");
    match response {
        HandlerOutcome::Response(bytes) => (request, bytes),
        other => panic!("{method}: {other:?}"),
    }
}

async fn record(
    files: &mut BTreeMap<String, Vec<u8>>,
    fixture: &Fixture,
    requester: &Stamp,
    name: &str,
    method: &str,
    params: Value,
) -> Value {
    let (request, response) = capture(fixture, requester, method, params).await;
    let result = serde_json::from_slice::<Value>(&response).unwrap()["result"].clone();
    files.insert(format!("{name}-request.json"), request);
    files.insert(format!("{name}-reply.json"), response);
    result
}

pub async fn generate() -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let basal = stamp("reserved:basal");
    for (decision, input) in DECISIONS {
        let fixture = Fixture::open(None).await;
        // These raw bodies are copied byte-for-byte from the upstream corpus;
        // the request envelope below is the actual input sent to the handler.
        files.insert(format!("{decision}-body.json"), input.to_vec());
        let reply = record(
            &mut files,
            &fixture,
            &basal,
            decision,
            "consent.request",
            decision_body(decision),
        )
        .await;
        let id = reply["elicitation_id"].as_str().unwrap();
        assert_eq!(id, "el_1");
        record(
            &mut files,
            &fixture,
            &basal,
            &format!("{decision}-await-pending"),
            "consent.await",
            json!({"elicitation_id":id,"timeout_ms":0}),
        )
        .await;
        fixture.clock.set(ANSWERED);
        fixture.ok(&phone::phone_stamp(), "consent.answer", json!({"elicitationId":id,"choiceId":match decision {
            "reconcile" => "send_again", "reenable" => "reenable", "grant_lost" => "check_now", _ => unreachable!()
        }})).await;
        record(
            &mut files,
            &fixture,
            &basal,
            &format!("{decision}-answers"),
            "consent.answers",
            json!({"since":0}),
        )
        .await;
    }

    let fixture = Fixture::open(None).await;
    let mut body = decision_body("grant_lost");
    body["dedup_key"] = json!("grant-lost:flow-x:github");
    body["binding"] = json!({"flow_id":"flow-x","local_attempt":7});
    let id = fixture.request(&basal, body.clone()).await;
    let old_list = phone::served(&fixture, "consent.list_pending", json!({})).await;
    files.insert("dedup-before-phone-list.json".into(), old_list);
    fixture.clock.set(ANSWERED);
    record(
        &mut files,
        &fixture,
        &basal,
        "dedup-refresh",
        "consent.request",
        body.clone(),
    )
    .await;
    files.insert(
        "dedup-refresh-phone-list.json".into(),
        phone::served(&fixture, "consent.list_pending", json!({})).await,
    );
    body["flow_decision"]["grant_label"] = json!("Updated provider display text");
    let replacement = record(
        &mut files,
        &fixture,
        &basal,
        "dedup-replace",
        "consent.request",
        body,
    )
    .await;
    assert_eq!(replacement["elicitation_id"], "el_2");
    record(
        &mut files,
        &fixture,
        &basal,
        "dedup-old-get",
        "consent.get",
        json!({"elicitation_id":id}),
    )
    .await;
    files.insert(
        "dedup-replace-phone-list.json".into(),
        phone::served(&fixture, "consent.list_pending", json!({})).await,
    );
    record(
        &mut files,
        &fixture,
        &basal,
        "withdraw",
        "consent.withdraw",
        json!({"elicitation_id":"el_2"}),
    )
    .await;
    record(
        &mut files,
        &fixture,
        &basal,
        "answers-after-withdraw",
        "consent.answers",
        json!({"since":0}),
    )
    .await;
    record(
        &mut files,
        &fixture,
        &basal,
        "ack-withdrawals",
        "consent.ack",
        json!({"cursor":2}),
    )
    .await;

    let fixture = Fixture::open(None).await;
    fixture.request(&basal, decision_body("grant_lost")).await;
    fixture.clock.set(NOW + 60_000);
    let expired = record(
        &mut files,
        &fixture,
        &basal,
        "grant-lost-expired-answers",
        "consent.answers",
        json!({"since":0}),
    )
    .await;
    record(
        &mut files,
        &fixture,
        &basal,
        "ack-expired",
        "consent.ack",
        json!({"cursor":expired["cursor"]}),
    )
    .await;
    record(
        &mut files,
        &fixture,
        &basal,
        "answers-after-ack",
        "consent.answers",
        json!({"since":0}),
    )
    .await;

    let fixture = Fixture::open(None).await;
    let core = stamp("reserved:prefrontal-core");
    let id = fixture.request(&core, phone::package_body()).await;
    fixture.clock.set(ANSWERED);
    fixture
        .ok(
            &phone::phone_stamp(),
            "consent.answer",
            json!({"elicitationId":id,"choiceId":"allow"}),
        )
        .await;
    record(
        &mut files,
        &fixture,
        &core,
        "report-execution",
        "consent.report_execution",
        json!({"elicitation_id":id,"execution":"executed"}),
    )
    .await;
    files
}

/// Generic-consent additions use a separate corpus so v1 stays byte-for-byte stable.
pub async fn generate_v2() -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let requester = Stamp {
        carried_agent_id: "agent-alf".into(),
        ..stamp("reserved:cerebellum")
    };
    for (name, reference) in [
        ("cerebellum", None),
        ("cerebellum-ref", Some("tool-call-42")),
    ] {
        let fixture = Fixture::open(None).await;
        let mut body = crate::support::app_body();
        if let Some(reference) = reference {
            body["requester_ref"] = json!(reference);
        }
        let reply = record(
            &mut files,
            &fixture,
            &requester,
            name,
            "consent.request",
            body,
        )
        .await;
        let id = reply["elicitation_id"].as_str().unwrap();
        record(
            &mut files,
            &fixture,
            &requester,
            &format!("{name}-get"),
            "consent.get",
            json!({"elicitation_id":id}),
        )
        .await;
        record(
            &mut files,
            &fixture,
            &requester,
            &format!("{name}-await"),
            "consent.await",
            json!({"elicitation_id":id,"timeout_ms":0}),
        )
        .await;
        files.insert(
            format!("{name}-phone-list-reply.json"),
            phone::served(&fixture, "consent.list_pending", json!({})).await,
        );
        files.insert(
            format!("{name}-phone-get-reply.json"),
            phone::served(&fixture, "consent.get", json!({"elicitationId":id})).await,
        );
        fixture.clock.set(ANSWERED);
        fixture
            .ok(
                &phone::phone_stamp(),
                "consent.answer",
                json!({"elicitationId":id,"choiceId":"allow_once"}),
            )
            .await;
        record(
            &mut files,
            &fixture,
            &requester,
            &format!("{name}-answers"),
            "consent.answers",
            json!({"since":0}),
        )
        .await;
        record(
            &mut files,
            &fixture,
            &stamp("reserved:prefrontal-core"),
            &format!("{name}-outcomes"),
            "consent.outcomes",
            json!({"since":0}),
        )
        .await;
    }
    let fixture = Fixture::open(None).await;
    let params = decision_body("grant_lost");
    files.insert(
        "cerebellum-non-consent-request.json".into(),
        serde_json::to_vec(&json!({"method":"consent.request","params":params})).unwrap(),
    );
    let refusal = fixture
        .call(&requester, "consent.request", params)
        .await
        .unwrap_err();
    assert_eq!(refusal["code"], "elicitation_requester_refused");
    files.insert(
        "cerebellum-non-consent-refusal.json".into(),
        serde_json::to_vec(&refusal).unwrap(),
    );

    // consent.detach on a pending card, a repeat of it, a card that settled
    // before any detach, and a card raised by another principal.
    let fixture = Fixture::open(None).await;
    let id = fixture
        .request(&requester, crate::support::app_body())
        .await;
    for name in ["cerebellum-detach-pending", "cerebellum-detach-repeat"] {
        record(
            &mut files,
            &fixture,
            &requester,
            name,
            "consent.detach",
            json!({"elicitation_id":id}),
        )
        .await;
    }
    let other = Stamp {
        carried_agent_id: "agent-alf".into(),
        ..stamp("reserved:plexus")
    };
    let params = json!({"elicitation_id":id});
    files.insert(
        "cerebellum-detach-wrong-owner-request.json".into(),
        serde_json::to_vec(&json!({"method":"consent.detach","params":params})).unwrap(),
    );
    let refusal = fixture
        .call(&other, "consent.detach", params)
        .await
        .unwrap_err();
    assert_eq!(refusal["code"], "elicitation_not_owner");
    files.insert(
        "cerebellum-detach-wrong-owner-refusal.json".into(),
        serde_json::to_vec(&refusal).unwrap(),
    );
    let settled = fixture
        .request(&requester, crate::support::app_body())
        .await;
    fixture.clock.set(ANSWERED);
    fixture
        .ok(
            &phone::phone_stamp(),
            "consent.answer",
            json!({"elicitationId":settled,"choiceId":"allow_once"}),
        )
        .await;
    record(
        &mut files,
        &fixture,
        &requester,
        "cerebellum-detach-settled",
        "consent.detach",
        json!({"elicitation_id":settled}),
    )
    .await;
    files
}
