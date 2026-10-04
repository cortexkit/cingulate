mod support;

use serde_json::{json, Value};
use support::*;

async fn step(fixture: &Fixture, file: &str, substitution: &Substitution<'_>) -> Value {
    assert_ne!(file, "elicitation_unavailable.json");
    let vector = vector("elicitation-requester-v1", file);
    let request = substitution.apply(&vector["request"]);
    let stamp = route_stamp(&request);
    let params = request.get("params").unwrap_or(&request).clone();
    let method = adapted_method(vector["method"].as_str().unwrap());
    if vector.get("error").is_some() {
        let before = fixture.rows();
        let pointers = fixture.payloads();
        let error = fixture.call(&stamp, &method, params).await.expect_err(file);
        assert_eq!(error["code"], vector["error"]["code"], "{file}: {error}");
        assert_eq!(fixture.rows(), before, "{file} must not write either table");
        assert_eq!(fixture.payloads(), pointers, "{file} must not notify");
        error
    } else {
        let reply = fixture.ok(&stamp, &method, params).await;
        assert_eq!(
            without_requester_label(reply.clone()),
            substitution.apply(&vector["reply"]),
            "{file}"
        );
        reply
    }
}

#[tokio::test]
async fn requester_vectors_follow_readme_scenario_and_adapter() {
    let fixture = Fixture::open(None).await;
    let request_vector = vector("elicitation-requester-v1", "request.json");
    let request = &request_vector["request"];
    let reply = fixture
        .ok(
            &route_stamp(request),
            &adapted_method(request_vector["method"].as_str().unwrap()),
            request.get("params").unwrap_or(request).clone(),
        )
        .await;
    let id = reply["elicitation_id"].as_str().unwrap().to_owned();
    assert!(id.starts_with("el_"));
    let first = Substitution {
        id: &id,
        now: NOW,
        expires: NOW + 60_000,
        answered: ANSWERED,
    };
    // The first call minted the id needed to substitute the request reply.
    assert_eq!(reply, first.apply(&request_vector["reply"]));
    step(&fixture, "get.json", &first).await;
    step(&fixture, "await.json", &first).await;

    // This is the README SHA table's order. not_found needs the later ack,
    // and unavailable belongs to core's subject resolver, not cingulate.
    for file in &REQUESTER_ORDER[3..12] {
        if *file != "elicitation_unavailable.json" {
            step(&fixture, file, &first).await;
        }
    }
    fixture.clock.set(ANSWERED);
    let answered = fixture
        .ok(
            &operator(),
            "consent.answer",
            json!({"elicitationId":id, "choiceId":"transfer:chrome"}),
        )
        .await;
    assert_eq!(answered["ok"], true);
    assert_eq!(answered["elicitation"]["state"], "answered");
    step(&fixture, "report_execution.json", &first).await;
    let answers = step(&fixture, "answers.json", &first).await;
    assert!(answers["cursor"].is_i64());
    assert_eq!(answers["cursor"], 1);
    step(&fixture, "ack.json", &first).await;
    step(&fixture, "elicitation_not_found.json", &first).await;

    fixture.clock.set(NOW + 2000);
    let second_id = fixture.request(&owner(), request_body()).await;
    assert_ne!(second_id, id);
    fixture.clock.set(NOW + 3000);
    let second = Substitution {
        id: &second_id,
        now: NOW + 2000,
        expires: NOW + 62_000,
        answered: NOW + 3000,
    };
    step(&fixture, "withdraw.json", &second).await;
    assert_eq!(fixture.rows()[0].len(), 2);
    assert_eq!(fixture.rows()[1].len(), 2);
    assert_eq!(fixture.payloads().len(), 4);
}

#[test]
fn route_adapter_preserves_authority_and_exact_placeholder_types() {
    let absent = route_stamp(&json!({"route":null}));
    assert!(absent.principal.is_none());
    assert!(!absent.operator_surface);
    let fallback = route_stamp(&json!({}));
    assert_eq!(fallback.principal.as_deref(), Some("reserved:plexus"));
    assert_eq!(fallback.bind_session, "provider-session");
    assert_eq!(fallback.carried_agent_id, "agent-alf");
    assert!(!fallback.operator_surface);
    let operator = route_stamp(
        &json!({"route":{"principal":"reserved:callosum","session":"phone","operator":true}}),
    );
    assert_eq!(operator.principal.as_deref(), Some("reserved:callosum"));
    assert_eq!(operator.bind_session, "phone");
    assert!(operator.operator_surface);
    assert!(operator.carried_agent_id.is_empty());
    assert!(operator.console_machine_id.is_none());
    let substitution = Substitution {
        id: "el_9",
        now: 100,
        expires: 200,
        answered: 150,
    };
    assert_eq!(
        substitution.apply(&json!([
            "@ELICITATION_ID@",
            "@HEAD_AGENT_ID@",
            "@NOW_MS@",
            "@EXPIRES_AT_MS@",
            "@ANSWERED_AT_MS@"
        ])),
        json!(["el_9", "agent-alf", 100, 200, 150])
    );
}
