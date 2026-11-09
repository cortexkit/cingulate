use super::*;
use ck_cingulate::federated::{holder_label, holder_selects};
use subc_client_rs::ModuleHandler;
use subc_protocol::scope::{
    FederatedAttributes, FederatedPeerClass, ParentState, RemoteClaim, ScopeAttributes, ScopeKind,
    ScopeStamp,
};

fn facts(claim: Option<RemoteClaim>) -> FederatedAttributes {
    FederatedAttributes::new(
        FederatedPeerClass::Computer,
        "a".repeat(32),
        "pairing-invented-1".into(),
        "Juniper".into(),
    )
    .with_remote_claim(claim)
}

fn bind(
    facts: FederatedAttributes,
    principal: &str,
    scope_ref: &str,
    epoch: u64,
) -> RouteBindRequest {
    RouteBindRequest::new(
        RouteHandle::detached(epoch.try_into().unwrap(), epoch.try_into().unwrap()),
        RouteTarget::ManagementSurface {
            module_id: "cingulate".into(),
        },
        // A computer must not become an operator just because the harness says phone.
        BindIdentity::new("fed-phone:forged", "invented-session"),
    )
    .with_principal(Principal::Reserved {
        module_id: principal.into(),
    })
    .with_scope(
        ScopeStamp::new(
            Principal::Reserved {
                module_id: "callosum".into(),
            },
            scope_ref,
            epoch,
            ScopeKind::Federated,
        )
        .with_attributes(ScopeAttributes::new().with_federated(Some(facts)))
        .with_parent_state(Some(ParentState::Linked)),
    )
}

fn subject(facts: FederatedAttributes) -> Stamp {
    Stamp::from_bind(&bind(facts, "plexus", "claim-invented-1", 1))
}

fn request() -> Value {
    let mut value = body();
    value["prompt"] = json!("read your mail.");
    value["title"] = json!("Read mail");
    value["scope"] = json!({"app":"example.invented.mail"});
    value["target"]["label"] = json!("Forged caller name");
    value
}

#[tokio::test]
async fn stamped_subject_and_exact_text_for_every_claim() {
    let fixture = Fixture::open(None).await;
    *fixture.resolver.reply.lock().unwrap() = Some(Ok(Some(ResolvedAgent {
        name: Some("Forged local name".into()),
        retired: true,
    })));
    let cases = [
        (
            Some(RemoteClaim::agent("Sage")),
            "“Juniper” says agent Sage wants to read your mail.",
            Some("Juniper · Sage"),
        ),
        (
            Some(RemoteClaim::worker(
                "invented-owner",
                "b".repeat(64),
                Some("Sage".into()),
            )),
            "“Juniper” says a worker for agent Sage wants to read your mail.",
            Some("Juniper · worker for Sage"),
        ),
        (
            Some(RemoteClaim::worker("invented-owner", "b".repeat(64), None)),
            "“Juniper” says one of its workers wants to read your mail.",
            Some("Juniper · worker"),
        ),
        (
            Some(RemoteClaim::flow(
                "Morning-digest",
                3,
                "install-invented",
                "c".repeat(64),
            )),
            "“Juniper” says flow “Morning-digest” (version 3) wants to read your mail.",
            Some("Juniper · flow Morning-digest v3"),
        ),
        (
            Some(RemoteClaim::run("Sage")),
            "“Juniper” says a run for agent Sage wants to read your mail. (this time only)",
            None,
        ),
        (
            None,
            "A program on “Juniper” (no agent named) wants to read your mail.",
            Some("Juniper · any program"),
        ),
    ];
    for (claim, prompt, label) in cases {
        let facts = facts(claim);
        let stamp = subject(facts.clone());
        let mut body = request();
        if matches!(facts.remote_claim, Some(RemoteClaim::Run { .. })) {
            body["options"]
                .as_array_mut()
                .unwrap()
                .retain(|option| option["effect"] == "decline" || option["id"] == "allow_once");
        }
        let id = fixture.request(&stamp, body).await;
        let card = ok(
            &fixture.handler,
            &stamp,
            "consent.get",
            json!({"elicitation_id":id}),
        )
        .await;
        assert_eq!(card["prompt"], prompt);
        assert_eq!(card["subject"]["federated"], json!(facts));
        assert!(card["subject"].get("agent_id").is_none());
        assert!(card["subject"].get("head_agent_id").is_none());
        assert_eq!(
            card["facts"][0]["value"],
            "“Juniper” is paired with this computer."
        );
        let selector = if facts.remote_claim.is_none() {
            "claimless"
        } else {
            "exact_claim"
        };
        assert_eq!(holder_label(&facts, selector).as_deref(), label);
        assert_eq!(card["subject"]["label"], label.unwrap_or("Juniper"));
        assert!(!card["prompt"].as_str().unwrap().contains("Forged"));
    }
    assert_eq!(
        holder_label(&facts(None), "all_claims").as_deref(),
        Some("Juniper · everything on it")
    );
    let mut long = facts(Some(RemoteClaim::agent("S".repeat(256))));
    long.peer_name = "界".repeat(80);
    assert_eq!(
        holder_label(&long, "exact_claim").unwrap().chars().count(),
        80
    );
}

#[tokio::test]
async fn unlinked_federated_parent_is_refused() {
    let fixture = Fixture::open(None).await;
    for state in [ParentState::Pending, ParentState::Ended] {
        let mut stamp = subject(facts(None));
        stamp.scope.as_mut().unwrap().parent_state = Some(state);
        fixture
            .unchanged_error(
                &stamp,
                "consent.request",
                request(),
                "elicitation_federated_unlinked",
            )
            .await;
    }
}

#[tokio::test]
async fn claimless_class_and_pairing_isolate_cards() {
    let fixture = Fixture::open(None).await;
    let original = facts(None);
    let id = fixture.request(&subject(original.clone()), request()).await;
    for class in [
        FederatedPeerClass::Phone,
        FederatedPeerClass::Sim,
        FederatedPeerClass::Unknown,
    ] {
        let mut changed = original.clone();
        changed.peer_class = class;
        error(
            &fixture.handler,
            &subject(changed),
            "consent.get",
            json!({"elicitation_id":id}),
            "elicitation_not_owner",
        )
        .await;
    }
    let mut changed = original;
    changed.pairing_id = "pairing-invented-2".into();
    error(
        &fixture.handler,
        &subject(changed),
        "consent.get",
        json!({"elicitation_id":id}),
        "elicitation_not_owner",
    )
    .await;
}

#[tokio::test]
async fn card_survives_route_loss_and_subject_identity_controls_collection() {
    let fixture = Fixture::open(None).await;
    let original = facts(Some(RemoteClaim::worker(
        "invented-owner",
        "b".repeat(64),
        Some("Sage".into()),
    )));
    let first = bind(original.clone(), "plexus", "claim-invented-1", 1);
    fixture.handler.on_bind(&first).await;
    let id = fixture.request(&Stamp::from_bind(&first), request()).await;
    fixture.handler.on_route_gone(&first.handle).await;
    let mut renamed = original.clone();
    renamed.peer_name = "Spruce".into();
    renamed.remote_claim = Some(RemoteClaim::worker(
        "invented-owner",
        "b".repeat(64),
        Some("Other-display-only".into()),
    ));
    let next = bind(renamed, "plexus", "claim-recreated-9", 9);
    fixture.handler.on_bind(&next).await;
    let next = Stamp::from_bind(&next);
    assert_eq!(
        ok(
            &fixture.handler,
            &next,
            "consent.get",
            json!({"elicitation_id":id})
        )
        .await["state"],
        "pending"
    );
    fixture.answer(&id, "allow_once").await;
    assert_eq!(
        ok(
            &fixture.handler,
            &next,
            "consent.await",
            json!({"elicitation_id":id,"timeout_ms":1})
        )
        .await["answered_choice_id"],
        "allow_once"
    );
    assert_eq!(
        ok(
            &fixture.handler,
            &next,
            "consent.answers",
            json!({"since":0})
        )
        .await["records"][0]["elicitation_id"],
        id
    );
    let mut variants = vec![];
    let mut peer = original.clone();
    peer.peer_fingerprint = "e".repeat(32);
    variants.push(subject(peer));
    let mut pairing = original.clone();
    pairing.pairing_id = "pairing-invented-2".into();
    variants.push(subject(pairing));
    let mut class = original.clone();
    class.peer_class = FederatedPeerClass::Sim;
    class.remote_claim = None;
    variants.push(subject(class));
    let mut claim = original.clone();
    claim.remote_claim = Some(RemoteClaim::worker("invented-owner", "d".repeat(64), None));
    variants.push(subject(claim));
    variants.push(Stamp::from_bind(&bind(
        original,
        "entorhinal",
        "claim-other-provider",
        2,
    )));
    for other in variants {
        error(
            &fixture.handler,
            &other,
            "consent.get",
            json!({"elicitation_id":id}),
            "elicitation_not_owner",
        )
        .await;
        assert_eq!(
            ok(
                &fixture.handler,
                &other,
                "consent.answers",
                json!({"since":0})
            )
            .await["records"],
            json!([])
        );
    }

    let agent = subject(facts(Some(RemoteClaim::agent("Sage"))));
    let agent_card = fixture.request(&agent, request()).await;
    let run = subject(facts(Some(RemoteClaim::run("Sage"))));
    error(
        &fixture.handler,
        &run,
        "consent.get",
        json!({"elicitation_id":agent_card}),
        "elicitation_not_owner",
    )
    .await;
}

#[tokio::test]
async fn run_answer_is_immutable_recoverable_and_single_use() {
    let fixture = Fixture::open(None).await;
    let stamp = subject(facts(Some(RemoteClaim::run("Sage"))));
    let mut body = request();
    body["options"].as_array_mut().unwrap().insert(
        1,
        json!({"id":"always","label":"Always allow","effect":"grant"}),
    );
    let grants = body["options"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|option| option["effect"] == "grant")
        .count();
    assert!(grants > 1, "fixture includes standing choices");
    fixture
        .unchanged_error(
            &stamp,
            "consent.request",
            body.clone(),
            "elicitation_invalid_request",
        )
        .await;
    body["options"]
        .as_array_mut()
        .unwrap()
        .retain(|option| option["effect"] == "decline" || option["id"] == "allow_once");
    body["options"][0]["label"] = json!("Always allow");
    body["requester_ref"] = json!("invented-call-1");
    let id = fixture.request(&stamp, body.clone()).await;
    let card = ok(
        &fixture.handler,
        &stamp,
        "consent.get",
        json!({"elicitation_id":id}),
    )
    .await;
    assert_eq!(card["single_use"], true);
    assert_eq!(card["options"][0]["label"], "Allow this time");
    assert_eq!(card["requester_ref"], "invented-call-1");
    assert!(card.get("duration").is_none());
    assert_eq!(fixture.answer(&id, "allow_once").await["ok"], true);
    assert_eq!(fixture.answer(&id, "deny").await["ok"], false);
    let replacement = Stamp::from_bind(&bind(
        facts(Some(RemoteClaim::run("Sage"))),
        "plexus",
        "run-recreated",
        7,
    ));
    for _ in 0..2 {
        assert_eq!(
            ok(
                &fixture.handler,
                &replacement,
                "consent.get",
                json!({"elicitation_id":id})
            )
            .await["answered_choice_id"],
            "allow_once"
        );
    }
    body["requester_ref"] = json!("invented-call-2");
    let next = fixture.request(&replacement, body).await;
    assert_ne!(next, id);
    assert_eq!(
        ok(
            &fixture.handler,
            &replacement,
            "consent.get",
            json!({"elicitation_id":next})
        )
        .await["state"],
        "pending"
    );
    assert_eq!(fixture.counts(), (2, 1));
}

#[test]
fn selectors_never_select_local_unstamped_or_run_authority() {
    let claims = [
        None,
        Some(RemoteClaim::agent("Sage")),
        Some(RemoteClaim::worker("invented-owner", "b".repeat(64), None)),
        Some(RemoteClaim::flow(
            "digest",
            3,
            "install-invented",
            "c".repeat(64),
        )),
        Some(RemoteClaim::run("Sage")),
    ];
    for claim in claims {
        let facts = facts(claim.clone());
        let stamp = subject(facts.clone());
        let mut holder = json!({"kind":"federated", "peer_fingerprint":facts.peer_fingerprint,"pairing_id":facts.pairing_id,"peer_class":"computer"});
        assert!(
            !holder_selects(&holder, stamp.scope.as_ref()),
            "missing selector"
        );
        holder["selector"] = json!("future");
        assert!(
            !holder_selects(&holder, stamp.scope.as_ref()),
            "unknown selector"
        );
        let run = matches!(claim, Some(RemoteClaim::Run { .. }));
        for selector in ["exact_claim", "claimless", "all_claims"] {
            holder["selector"] = json!(selector);
            if selector == "exact_claim" {
                if let Some(claim) = &claim {
                    holder["claim"] = json!(claim);
                }
            } else {
                holder.as_object_mut().unwrap().remove("claim");
            }
            let expected = !run
                && match selector {
                    "exact_claim" => claim.is_some(),
                    "claimless" => claim.is_none(),
                    _ => true,
                };
            assert_eq!(
                holder_selects(&holder, stamp.scope.as_ref()),
                expected,
                "{selector}: {claim:?}"
            );
            assert!(!holder_selects(&holder, None));
            let legacy = bound_surface(
                Principal::Reserved {
                    module_id: "callosum".into(),
                },
                "fed-computer:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            );
            assert!(!holder_selects(&holder, legacy.scope.as_ref()));
            let local = ScopeStamp::new(
                Principal::Reserved {
                    module_id: "prefrontal-core".into(),
                },
                "local-invented",
                1,
                ScopeKind::Head,
            )
            .with_attributes(ScopeAttributes::new().with_agent_id(Some("Sage".into())));
            assert!(!holder_selects(&holder, Some(&local)));
            if expected {
                for key in ["peer_fingerprint", "pairing_id", "peer_class"] {
                    let mut changed = holder.clone();
                    changed[key] = json!("different");
                    assert!(!holder_selects(&changed, stamp.scope.as_ref()), "{key}");
                }
            }
        }
    }
}

#[test]
fn exact_claim_compares_only_canonical_identity_and_broad_selectors_forbid_claims() {
    let original = facts(Some(RemoteClaim::worker(
        "invented-owner",
        "b".repeat(64),
        Some("Sage".into()),
    )));
    let stamp = subject(original.clone());
    let mut holder = json!({"kind":"federated","peer_fingerprint":original.peer_fingerprint,
        "pairing_id":original.pairing_id,"peer_class":"computer","selector":"exact_claim",
        "claim":RemoteClaim::worker("invented-owner", "b".repeat(64), Some("Other-display-only".into()))});
    assert!(
        holder_selects(&holder, stamp.scope.as_ref()),
        "worker attribution is display only"
    );
    for claim in [
        RemoteClaim::worker("different-owner", "b".repeat(64), None),
        RemoteClaim::worker("invented-owner", "d".repeat(64), None),
        RemoteClaim::agent("Sage"),
    ] {
        holder["claim"] = json!(claim);
        assert!(!holder_selects(&holder, stamp.scope.as_ref()));
    }
    for selector in ["claimless", "all_claims"] {
        holder["selector"] = json!(selector);
        assert!(
            !holder_selects(&holder, stamp.scope.as_ref()),
            "broad selectors forbid claim"
        );
    }
    let flow = RemoteClaim::flow("digest", 3, "install-invented", "c".repeat(64));
    let stamp = subject(facts(Some(flow.clone())));
    holder["selector"] = json!("exact_claim");
    holder["claim"] = json!(flow);
    assert!(holder_selects(&holder, stamp.scope.as_ref()));
    for (field, value) in [
        ("flow_id", json!("another-flow")),
        ("flow_version", json!(4)),
        ("flow_install_id", json!("another-install")),
        ("flow_code_hash", json!("d".repeat(64))),
    ] {
        let mut changed = holder.clone();
        changed["claim"][field] = value;
        assert!(!holder_selects(&changed, stamp.scope.as_ref()), "{field}");
    }
}

#[tokio::test]
async fn federated_computer_cannot_borrow_harness_operator_authority() {
    let fixture = Fixture::open(None).await;
    let stamp = Stamp::from_bind(&bind(facts(None), "callosum", "base-invented", 1));
    error(
        &fixture.handler,
        &stamp,
        "consent.list_pending",
        json!({}),
        "elicitation_operator_required",
    )
    .await;
    error(
        &fixture.handler,
        &stamp,
        "consent.request",
        request(),
        "elicitation_requester_refused",
    )
    .await;
}
