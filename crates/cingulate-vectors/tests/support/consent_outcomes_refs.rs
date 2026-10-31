use std::{collections::BTreeMap, time::Duration};

use ck_cingulate::Stamp;
use serde_json::{json, Value};
use subc_client_rs::{HandlerOutcome, RouteBindRequest, RouteHandle};
use subc_protocol::{
    scope::{ScopeAttributes, ScopeKind, ScopeStamp},
    BindIdentity, Principal, RouteTarget,
};

use crate::{
    phone,
    support::{app_body, stamp, Fixture, ANSWERED},
};

pub const CORPUS: &str = "consent-outcomes-refs-v1";

fn scoped_requester() -> Stamp {
    Stamp::from_bind(
        &RouteBindRequest::new(
            RouteHandle::detached(1, 1),
            RouteTarget::ManagementSurface {
                module_id: "cingulate".into(),
            },
            BindIdentity::new("test-harness", "untrusted-session"),
        )
        .with_principal(Principal::Reserved {
            module_id: "cerebellum".into(),
        })
        .with_scope(
            ScopeStamp::new(
                Principal::Reserved {
                    module_id: "prefrontal-core".into(),
                },
                "sc_head_alf",
                7,
                ScopeKind::Head,
            )
            .with_attributes(ScopeAttributes::new().with_agent_id(Some("agent-alf".into())))
            .with_owner_authorized(true),
        ),
    )
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
    .expect("consent outcomes vector operation exceeded 1 s");
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
    let envelope = serde_json::from_slice::<Value>(&response).unwrap();
    let result = envelope["result"].clone();
    files.insert(format!("{name}-request.json"), request);
    files.insert(format!("{name}-reply.json"), response);
    result
}

async fn settle_attached(
    files: &mut BTreeMap<String, Vec<u8>>,
    fixture: &Fixture,
    requester: &Stamp,
    name: &str,
    body: Value,
) -> Value {
    let result = record(files, fixture, requester, name, "consent.request", body).await;
    let id = result["elicitation_id"].as_str().unwrap();
    fixture.clock.set(ANSWERED);
    fixture
        .ok(
            &phone::phone_stamp(),
            "consent.answer",
            json!({"elicitationId":id,"choiceId":"allow_once"}),
        )
        .await;
    let outcomes = record(
        files,
        fixture,
        &stamp("reserved:prefrontal-core"),
        &format!("{name}-outcomes"),
        "consent.outcomes",
        json!({"since":0}),
    )
    .await;
    outcomes["records"][0].clone()
}

pub async fn generate() -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let requester = Stamp {
        carried_agent_id: "agent-alf".into(),
        ..stamp("reserved:cerebellum")
    };

    let fixture = Fixture::open(None).await;
    let mut body = app_body();
    body["requester_ref"] = json!("call-single-use");
    body["single_use"] = json!(true);
    let result = record(
        &mut files,
        &fixture,
        &requester,
        "single-use",
        "consent.request",
        body,
    )
    .await;
    let id = result["elicitation_id"].as_str().unwrap();
    let requester_read = record(
        &mut files,
        &fixture,
        &requester,
        "single-use-get",
        "consent.get",
        json!({"elicitation_id":id}),
    )
    .await;
    assert_eq!(requester_read["single_use"], true);
    let phone_get = phone::served(&fixture, "consent.get", json!({"elicitationId":id})).await;
    let phone_value: Value = serde_json::from_slice(&phone_get).unwrap();
    assert!(phone_value["result"].get("singleUse").is_none());
    assert!(phone_value["result"].get("single_use").is_none());
    files.insert("single-use-phone-get-reply.json".into(), phone_get);
    fixture.clock.set(ANSWERED);
    fixture
        .ok(
            &phone::phone_stamp(),
            "consent.answer",
            json!({"elicitationId":id,"choiceId":"allow_once"}),
        )
        .await;
    let outcome = record(
        &mut files,
        &fixture,
        &stamp("reserved:prefrontal-core"),
        "single-use-outcomes",
        "consent.outcomes",
        json!({"since":0}),
    )
    .await;
    let outcome = &outcome["records"][0];
    assert_eq!(outcome["requester_ref"], "call-single-use");
    assert_eq!(outcome["single_use"], true);
    assert!(outcome.get("late").is_none());

    let fixture = Fixture::open(None).await;
    let mut body = app_body();
    body["requester_ref"] = json!("call-attached");
    let outcome = settle_attached(&mut files, &fixture, &requester, "attached-ref", body).await;
    assert_eq!(outcome["requester_ref"], "call-attached");
    assert!(outcome.get("single_use").is_none());
    assert!(outcome.get("late").is_none());

    let fixture = Fixture::open(None).await;
    let outcome = settle_attached(
        &mut files,
        &fixture,
        &requester,
        "neither-field",
        app_body(),
    )
    .await;
    assert!(outcome.get("requester_ref").is_none());
    assert!(outcome.get("single_use").is_none());
    assert!(outcome.get("late").is_none());

    let fixture = Fixture::open(None).await;
    let scoped = scoped_requester();
    let mut body = app_body();
    body["requester_ref"] = json!("call-late");
    let created = record(
        &mut files,
        &fixture,
        &scoped,
        "late",
        "consent.request",
        body,
    )
    .await;
    let id = created["elicitation_id"].as_str().unwrap();
    record(
        &mut files,
        &fixture,
        &scoped,
        "late-detach",
        "consent.detach",
        json!({"elicitation_id":id}),
    )
    .await;
    fixture.clock.set(ANSWERED);
    fixture
        .ok(
            &phone::phone_stamp(),
            "consent.answer",
            json!({"elicitationId":id,"choiceId":"allow_once"}),
        )
        .await;
    let outcome = record(
        &mut files,
        &fixture,
        &stamp("reserved:prefrontal-core"),
        "late-outcomes",
        "consent.outcomes",
        json!({"since":0}),
    )
    .await;
    assert_eq!(outcome["records"][0]["requester_ref"], "call-late");
    assert_eq!(outcome["records"][0]["late"], true);

    let fixture = Fixture::open(None).await;
    let scoped = scoped_requester();
    let mut body = app_body();
    body["requester_ref"] = json!("call-reraised");
    body["dedup_key"] = json!("same-pending-card");
    let created = record(
        &mut files,
        &fixture,
        &scoped,
        "reraised",
        "consent.request",
        body.clone(),
    )
    .await;
    let id = created["elicitation_id"].as_str().unwrap();
    record(
        &mut files,
        &fixture,
        &scoped,
        "reraised-detach",
        "consent.detach",
        json!({"elicitation_id":id}),
    )
    .await;
    let refreshed = record(
        &mut files,
        &fixture,
        &scoped,
        "reraised-again",
        "consent.request",
        body,
    )
    .await;
    assert_eq!(refreshed["elicitation_id"], id);
    fixture.clock.set(ANSWERED);
    fixture
        .ok(
            &phone::phone_stamp(),
            "consent.answer",
            json!({"elicitationId":id,"choiceId":"allow_once"}),
        )
        .await;
    let outcome = record(
        &mut files,
        &fixture,
        &stamp("reserved:prefrontal-core"),
        "reraised-outcomes",
        "consent.outcomes",
        json!({"since":0}),
    )
    .await;
    assert_eq!(outcome["records"][0]["requester_ref"], "call-reraised");
    assert!(outcome["records"][0].get("late").is_none());

    for (name, value) in [
        (
            "single-use-non-consent",
            json!({"kind":"package","single_use":true}),
        ),
        (
            "single-use-non-boolean",
            json!({"kind":"consent","single_use":"yes"}),
        ),
    ] {
        let fixture = Fixture::open(None).await;
        let mut params = app_body();
        for (key, value) in value.as_object().unwrap() {
            params[key] = value.clone();
        }
        let request =
            serde_json::to_vec(&json!({"method":"consent.request","params":params.clone()}))
                .unwrap();
        let refusal = fixture
            .call(&requester, "consent.request", params)
            .await
            .unwrap_err();
        assert_eq!(refusal["code"], "elicitation_invalid_request");
        assert!(refusal["message"].as_str().unwrap().contains("single_use"));
        files.insert(format!("{name}-request.json"), request);
        files.insert(
            format!("{name}-refusal.json"),
            serde_json::to_vec(&refusal).unwrap(),
        );
    }

    files
}
