#[path = "support/phone.rs"]
#[allow(dead_code)]
mod phone;
mod support;

use cingulate_store::ScopeIdentity;
use ck_cingulate::ScopeLife;
use serde_json::{json, Value};
use subc_client_rs::HandlerOutcome;
use subc_protocol::scope::ScopeKind;
use support::{app_body, worker, Fixture, ANSWERED};

#[tokio::test]
async fn worker_live_head_is_subject_and_worker_is_asker() {
    for module in ["cerebellum", "plexus", "entorhinal", "direct"] {
        let fixture = Fixture::open(None).await;
        let runtime = worker::Runtime::new(Ok(ScopeLife::Live(worker::head())));
        fixture.handler.set_access_runtime(runtime.clone());
        let stamp = worker::stamp(module, ScopeKind::Worker);
        let id = fixture.request(&stamp, app_body()).await;
        let stored = fixture.stored_record(&id);
        assert_eq!(stored.subject.agent_id.as_deref(), Some("agent-alf"));
        assert_eq!(stored.subject.head_agent_id, "agent-alf");
        assert_eq!(stored.subject.label, "ALF");
        assert!(stored.subject.via_worker);
        assert_eq!(
            stored.asker,
            Some(ScopeIdentity {
                scope_ref: "sc_worker".into(),
                scope_epoch: 3,
                agent_id: None,
            })
        );
        assert_eq!(
            *runtime.described.lock().unwrap(),
            vec![ScopeIdentity {
                agent_id: None,
                ..worker::head()
            }]
        );
        let card = fixture
            .ok(&stamp, "consent.get", json!({"elicitation_id":id}))
            .await;
        assert_eq!(
            card["subject"],
            json!({"agent_id":"agent-alf","head_agent_id":"agent-alf","label":"ALF","via_worker":true})
        );
        let phone: Value = serde_json::from_slice(
            &phone::served(&fixture, "consent.get", json!({"elicitationId":id})).await,
        )
        .unwrap();
        assert_eq!(
            phone["result"]["subject"],
            json!({"agentId":"agent-alf","headAgentId":"agent-alf","label":"ALF","viaWorker":true})
        );
        fixture.clock.set(ANSWERED);
        fixture
            .ok(
                &phone::phone_stamp(),
                "consent.answer",
                json!({"elicitationId":id,"choiceId":"allow_once"}),
            )
            .await;
        let outcomes = fixture
            .ok(
                &support::stamp("reserved:prefrontal-core"),
                "consent.outcomes",
                json!({"since":0}),
            )
            .await;
        assert_eq!(outcomes["records"][0]["subject"], card["subject"]);
    }
}

#[tokio::test]
async fn worker_without_parent_is_refused_no_head() {
    let fixture = Fixture::open(None).await;
    let runtime = worker::Runtime::new(Ok(ScopeLife::Live(worker::head())));
    fixture.handler.set_access_runtime(runtime.clone());
    let mut stamp = worker::stamp("cerebellum", ScopeKind::Worker);
    stamp.scope.as_mut().unwrap().parent = None;
    let before = fixture.rows();
    assert_eq!(
        fixture
            .call(&stamp, "consent.request", app_body())
            .await
            .unwrap_err()["code"],
        "access_no_head"
    );
    assert!(runtime.described.lock().unwrap().is_empty());
    assert_eq!(fixture.rows(), before);
}

#[tokio::test]
async fn worker_gone_head_is_refused_no_head() {
    let fixture = Fixture::open(None).await;
    fixture
        .handler
        .set_access_runtime(worker::Runtime::new(Ok(ScopeLife::Gone)));
    let before = fixture.rows();
    assert_eq!(
        fixture
            .call(
                &worker::stamp("cerebellum", ScopeKind::Worker),
                "consent.request",
                app_body()
            )
            .await
            .unwrap_err()["code"],
        "access_no_head"
    );
    assert_eq!(fixture.rows(), before);
}

#[tokio::test]
async fn worker_unknown_or_failed_liveness_is_refused_unavailable() {
    for life in [Ok(ScopeLife::Unknown), Err("daemon disconnected".into())] {
        let fixture = Fixture::open(None).await;
        fixture
            .handler
            .set_access_runtime(worker::Runtime::new(life));
        let before = fixture.rows();
        assert_eq!(
            fixture
                .call(
                    &worker::stamp("cerebellum", ScopeKind::Worker),
                    "consent.request",
                    app_body()
                )
                .await
                .unwrap_err()["code"],
            "access_unavailable"
        );
        assert_eq!(fixture.rows(), before);
    }
}

#[tokio::test]
async fn head_card_remains_byte_identical_without_worker_flag() {
    let fixture = Fixture::open(None).await;
    let runtime = worker::Runtime::new(Err("a head must not describe a parent".into()));
    fixture.handler.set_access_runtime(runtime.clone());
    let stamp = worker::stamp("cerebellum", ScopeKind::Head);
    let id = fixture.request(&stamp, app_body()).await;
    let request =
        serde_json::to_vec(&json!({"method":"consent.get","params":{"elicitation_id":id}}))
            .unwrap();
    let HandlerOutcome::Response(bytes) = fixture.handler.handle_stamped(&stamp, &request).await
    else {
        panic!("head card get refused");
    };
    assert_eq!(
        bytes,
        include_bytes!("../../../test-vectors/consent-requester-v2/cerebellum-get-reply.json")
    );
    assert_eq!(
        phone::served(&fixture, "consent.get", json!({"elicitationId":id})).await,
        include_bytes!(
            "../../../test-vectors/consent-requester-v2/cerebellum-phone-get-reply.json"
        )
    );
    assert!(runtime.described.lock().unwrap().is_empty());
}

#[tokio::test]
async fn detached_worker_late_answer_notice_targets_worker_scope() {
    let fixture = Fixture::open(None).await;
    let runtime = worker::Runtime::new(Ok(ScopeLife::Live(worker::head())));
    fixture.handler.set_access_runtime(runtime.clone());
    let stamp = worker::stamp("cerebellum", ScopeKind::Worker);
    let id = fixture.request(&stamp, app_body()).await;
    fixture
        .ok(&stamp, "consent.detach", json!({"elicitation_id":id}))
        .await;
    fixture.clock.set(ANSWERED);
    fixture
        .ok(
            &phone::phone_stamp(),
            "consent.answer",
            json!({"elicitationId":id,"choiceId":"allow_once"}),
        )
        .await;
    tokio::task::yield_now().await;
    fixture.handler.deliver_access_notices().await.unwrap();
    tokio::task::yield_now().await;
    fixture.handler.deliver_access_notices().await.unwrap();
    assert_eq!(
        runtime
            .notices
            .lock()
            .unwrap()
            .iter()
            .filter(|notice| notice["purpose"] == "consent_answered")
            .cloned()
            .collect::<Vec<_>>(),
        vec![
            json!({"elicitation_id":id,"purpose":"consent_answered","asker":{"scope_ref":"sc_worker","scope_epoch":3},"fields":{"requester":"cerebellum","outcome":"answered","choice_id":"allow_once","choice_label":"Allow once","settled_at_ms":ANSWERED}})
        ]
    );
}

#[tokio::test]
async fn worker_fallback_does_not_admit_other_missing_agent_stamps_or_body_identity() {
    let fixture = Fixture::open(None).await;
    let runtime = worker::Runtime::new(Ok(ScopeLife::Live(worker::head())));
    fixture.handler.set_access_runtime(runtime.clone());
    let worker_stamp = worker::stamp("cerebellum", ScopeKind::Worker);
    let mut foreign = worker_stamp.clone();
    foreign.scope.as_mut().unwrap().owner = worker::principal("plexus");
    let mut unscoped = worker_stamp.clone();
    unscoped.scope = None;
    for stamp in [foreign, unscoped] {
        assert_eq!(
            fixture
                .call(&stamp, "consent.request", app_body())
                .await
                .unwrap_err()["code"],
            "elicitation_requester_refused"
        );
    }
    let mut head = worker::stamp("cerebellum", ScopeKind::Head);
    head.carried_agent_id.clear();
    head.scope.as_mut().unwrap().attributes = Default::default();
    assert_eq!(
        fixture
            .call(&head, "consent.request", app_body())
            .await
            .unwrap_err()["code"],
        "elicitation_requester_refused"
    );
    for key in ["agent_id", "parent", "asker", "subject"] {
        let mut body = app_body();
        body[key] = json!("forged");
        assert_eq!(
            fixture
                .call(&worker_stamp, "consent.request", body)
                .await
                .unwrap_err()["code"],
            "elicitation_invalid_request"
        );
    }
    assert!(runtime.described.lock().unwrap().is_empty());
}

#[tokio::test]
async fn worker_live_head_without_agent_is_refused_no_head() {
    let fixture = Fixture::open(None).await;
    fixture
        .handler
        .set_access_runtime(worker::Runtime::new(Ok(ScopeLife::Live(ScopeIdentity {
            agent_id: None,
            ..worker::head()
        }))));
    assert_eq!(
        fixture
            .call(
                &worker::stamp("cerebellum", ScopeKind::Worker),
                "consent.request",
                app_body()
            )
            .await
            .unwrap_err()["code"],
        "access_no_head"
    );
}

/// Core files an agent's flow install on that agent's scope, so the card is the
/// agent's ("ALF asks"), or its head's when a worker filed it. Unscoped, the
/// card stays Prefrontal's own.
#[tokio::test]
async fn core_flow_install_card_names_the_agent_of_its_scope() {
    let mut body = app_body();
    body["kind"] = json!("flow_install");
    body["flow_install"] = json!({"flow":"morning-summary","manifest":{"schedule":"daily"}});
    body["target"] = json!({"kind":"flow","label":"morning-summary"});
    body["scope"] = json!({"flow_id":"morning-summary"});
    for (kind, via_worker) in [(ScopeKind::Head, false), (ScopeKind::Worker, true)] {
        let fixture = Fixture::open(None).await;
        fixture
            .handler
            .set_access_runtime(worker::Runtime::new(Ok(ScopeLife::Live(worker::head()))));
        let stamp = worker::stamp("prefrontal-core", kind);
        let id = fixture.request(&stamp, body.clone()).await;
        let phone: Value = serde_json::from_slice(
            &phone::served(&fixture, "consent.get", json!({"elicitationId":id})).await,
        )
        .unwrap();
        let mut expected = json!({"agentId":"agent-alf","headAgentId":"agent-alf","label":"ALF"});
        if via_worker {
            expected["viaWorker"] = json!(true);
        }
        assert_eq!(phone["result"]["subject"], expected);
    }
    let fixture = Fixture::open(None).await;
    let mut unscoped = worker::stamp("prefrontal-core", ScopeKind::Head);
    unscoped.scope = None;
    unscoped.carried_agent_id.clear();
    let id = fixture.request(&unscoped, body.clone()).await;
    let phone: Value = serde_json::from_slice(
        &phone::served(&fixture, "consent.get", json!({"elicitationId":id})).await,
    )
    .unwrap();
    assert!(phone["result"]["subject"].get("headAgentId").is_none());
    assert_eq!(phone["result"]["requesterName"], "Prefrontal");
    // A body naming an agent changes nothing: the agent comes only from the stamp.
    let mut forged = body.clone();
    forged["agent_id"] = json!("agent-alf");
    assert_eq!(
        fixture
            .call(&unscoped, "consent.request", forged)
            .await
            .unwrap_err()["code"],
        "elicitation_invalid_request"
    );
}
