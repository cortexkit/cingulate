use std::{collections::BTreeMap, time::Duration};

use ck_cingulate::Stamp;
use serde_json::{json, Value};
use subc_client_rs::{HandlerOutcome, RouteBindRequest, RouteHandle};
use subc_protocol::{BindIdentity, Principal, RouteTarget};

use crate::support::{app_body, stamp, Fixture, ANSWERED, NOW};

pub const CORPUS: &str = "consent-phone-v1";

pub fn phone_stamp() -> Stamp {
    Stamp::from_bind(
        &RouteBindRequest::new(
            RouteHandle::detached(1, 1),
            RouteTarget::ManagementSurface {
                module_id: "cingulate".into(),
            },
            BindIdentity::new("fed-phone:vector-device", "ignored-session"),
        )
        .with_principal(Principal::Reserved {
            module_id: "callosum".into(),
        }),
    )
}

pub fn package_body() -> Value {
    let request: Value = serde_json::from_slice(include_bytes!(
        "../../../../test-vectors/consent-package-card-v1/request.json"
    ))
    .unwrap();
    request["params"].clone()
}

pub async fn served(fixture: &Fixture, method: &str, params: Value) -> Vec<u8> {
    let request = serde_json::to_vec(&json!({"method":method,"params":params})).unwrap();
    match tokio::time::timeout(
        Duration::from_secs(1),
        fixture.handler.handle_stamped(&phone_stamp(), &request),
    )
    .await
    .expect("phone op exceeded 1 s")
    {
        HandlerOutcome::Response(bytes) => bytes,
        other => panic!("{method}: {other:?}"),
    }
}

// Store the untouched response bytes: no field normalization, envelope removal,
// pretty-printing or trailing newline can conceal a wire change.
pub async fn generate() -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let core = stamp("reserved:prefrontal-core");
    for state in ["answered", "expired", "withdrawn"] {
        let fixture = Fixture::open(None).await;
        let request = package_body();
        let expiry = NOW + request["expires_in_ms"].as_i64().unwrap();
        let id = fixture.request(&core, request).await;
        assert_eq!(id, "el_1");
        if state == "answered" {
            let list = served(&fixture, "consent.list_pending", json!({})).await;
            let revision: Value =
                serde_json::from_slice::<Value>(&list).unwrap()["result"]["revision"].clone();
            files.insert("package-list-pending.json".into(), list);
            files.insert(
                "list-unchanged.json".into(),
                served(
                    &fixture,
                    "consent.list_pending",
                    json!({"if_revision":revision}),
                )
                .await,
            );
            files.insert(
                "package-get-pending.json".into(),
                served(&fixture, "consent.get", json!({"elicitationId":id})).await,
            );
            fixture.clock.set(ANSWERED);
            files.insert(
                "answer-success.json".into(),
                served(
                    &fixture,
                    "consent.answer",
                    json!({"elicitationId":id,"choiceId":"allow"}),
                )
                .await,
            );
            files.insert(
                "answer-already-answered.json".into(),
                served(
                    &fixture,
                    "consent.answer",
                    json!({"elicitationId":id,"choiceId":"allow"}),
                )
                .await,
            );
            files.insert(
                "answer-different-choice.json".into(),
                served(
                    &fixture,
                    "consent.answer",
                    json!({"elicitationId":id,"choiceId":"deny"}),
                )
                .await,
            );
            files.insert(
                "answer-not-found.json".into(),
                served(
                    &fixture,
                    "consent.answer",
                    json!({"elicitationId":"el_missing","choiceId":"allow"}),
                )
                .await,
            );
        } else {
            fixture
                .clock
                .set(if state == "expired" { expiry } else { ANSWERED });
            if state == "withdrawn" {
                fixture
                    .ok(&core, "consent.withdraw", json!({"elicitation_id":id}))
                    .await;
            }
            files.insert(
                format!("answer-{state}.json"),
                served(
                    &fixture,
                    "consent.answer",
                    json!({"elicitationId":id,"choiceId":"allow"}),
                )
                .await,
            );
        }
        files.insert(
            format!("package-get-{state}.json"),
            served(&fixture, "consent.get", json!({"elicitationId":id})).await,
        );
    }

    let fixture = Fixture::open(None).await;
    let mut install = app_body();
    install["kind"] = json!("flow_install");
    install["title"] = json!("Install morning summary");
    install["prompt"] = json!("Allow this scheduled summary flow?");
    install["flow_install"] = json!({"flow":"morning-summary","manifest":{"schedule":"daily"}});
    let install_id = fixture.request(&core, install).await;
    assert_eq!(install_id, "el_1");
    files.insert(
        "flow-install-get-pending.json".into(),
        served(&fixture, "consent.get", json!({"elicitationId":install_id})).await,
    );

    let mut decision = app_body();
    decision["kind"] = json!("flow_decision");
    decision["title"] = json!("Choose a summary destination");
    decision["prompt"] = json!("Send this summary to the inbox?");
    decision["flow_decision"] = json!({"flow_id":"morning-summary","step":"destination"});
    decision["options"][0]["effect"] = json!("choose");
    let decision_id = fixture.request(&stamp("reserved:basal"), decision).await;
    assert_eq!(decision_id, "el_2");
    files.insert(
        "flow-decision-get-pending.json".into(),
        served(
            &fixture,
            "consent.get",
            json!({"elicitationId":decision_id}),
        )
        .await,
    );

    let browser = Stamp {
        carried_agent_id: "agent-alf".into(),
        ..stamp("reserved:cerebellum")
    };
    let mut consent = app_body();
    consent["title"] = json!("Allow browser access");
    consent["prompt"] = json!("Allow reading this documentation page?");
    consent["target"] = json!({"kind":"browser","label":"docs.example.org"});
    consent["scope"] = json!({"origins":["https://docs.example.org"]});
    consent["facts"] = json!([{"label":"Origin","value":"https://docs.example.org"},{"label":"Action","value":"Read page"}]);
    consent["preview"] = json!({"label":"Page preview","text":"Documentation index"});
    consent["binding"] = json!({"private_route":"not projected"});
    let browser_id = fixture.request(&browser, consent).await;
    assert_eq!(browser_id, "el_3");
    files.insert(
        "browser-get-pending.json".into(),
        served(&fixture, "consent.get", json!({"elicitationId":browser_id})).await,
    );
    files.insert(
        "other-list-pending.json".into(),
        served(&fixture, "consent.list_pending", json!({})).await,
    );
    files
}
