use super::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use subc_protocol::{
    peer::{OperatorApp, RouteBindPeer},
    BindIdentity, RouteTarget,
};

struct FixedClock;

#[async_trait]
impl Clock for FixedClock {
    fn now_ms(&self) -> i64 {
        1_700_000_000_000
    }
    async fn sleep_until(&self, _: i64) {
        std::future::pending().await
    }
}

struct AgentResolver;

#[async_trait]
impl Resolver for AgentResolver {
    async fn resolve(&self, _: &str) -> Result<Option<ResolvedAgent>, String> {
        Ok(Some(ResolvedAgent {
            name: Some("Test agent".into()),
            retired: false,
        }))
    }
}

fn requester(module: &str, agent: &str) -> Stamp {
    Stamp {
        principal: Some(format!("reserved:{module}")),
        scope: Some(
            serde_json::from_value(json!({
                "owner":{"kind":"reserved","module_id":"prefrontal-core"},
                "ref":"H","scope_epoch":7,"kind":"head",
                "attributes":{"agent_id":agent},"owner_authorized":true
            }))
            .unwrap(),
        ),
        carried_agent_id: agent.into(),
        ..Stamp::default()
    }
}

fn local_bind() -> RouteBindRequest {
    RouteBindRequest::new(
        RouteHandle::detached(1, 1),
        RouteTarget::ManagementSurface {
            module_id: MODULE_ID.into(),
        },
        BindIdentity::new("local-desktop", "untrusted-session"),
    )
    .with_principal(Principal::Direct)
}

fn desktop() -> Stamp {
    Stamp::from_bind_with_local_operator(
        &local_bind().with_peer(
            RouteBindPeer::new().with_operator_app(Some(OperatorApp::new("alfonso-desktop", 5))),
        ),
        false,
    )
}

async fn handler() -> (Handler, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let handler = Handler::new(Options {
        sweep_timer: false,
        clock: Arc::new(FixedClock),
        resolver: Arc::new(AgentResolver),
        ..Options::default()
    });
    handler
        .hello_ack(&ModuleHelloAckBody {
            negotiated_ver: 1,
            subc_ops: vec![],
            subc_capabilities: vec![],
            storage: Some(
                serde_json::to_value(storage_descriptor(None, dir.path().to_str().unwrap()))
                    .unwrap(),
            ),
            machine_id: None,
        })
        .await;
    (handler, dir)
}

async fn ordinary_card(handler: &Handler) -> String {
    let request = serde_json::from_str(include_str!(
        "../../../test-vectors/elicitation-request-v1/valid/app-grant-once.json"
    ))
    .unwrap();
    handler
        .dispatch(
            &Stamp::from_principal(Some(&Principal::Direct)),
            "consent.request",
            request,
        )
        .await
        .unwrap()["elicitation_id"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn desktop_stamp_accepts_ordinary_answer() {
    let (handler, _dir) = handler().await;
    let id = ordinary_card(&handler).await;
    let stamp = desktop();
    assert!(!stamp.local_operator);
    assert_eq!(stamp.operator_app.as_deref(), Some("alfonso-desktop"));
    let answer = handler
        .dispatch(
            &stamp,
            "consent.answer",
            json!({"elicitationId":id,"choiceId":"allow_once"}),
        )
        .await
        .unwrap();
    assert_eq!(answer["ok"], true);
    assert_eq!(answer["elicitation"]["answeredSurface"], "operator_app");
    assert!(answer["elicitation"].get("answeredDevice").is_none());
    let outcomes = handler
        .dispatch(
            &requester("prefrontal-core", "agent-A"),
            "consent.outcomes",
            json!({"since":0}),
        )
        .await
        .unwrap();
    assert_eq!(outcomes["records"][0]["answered_surface"], "operator_app");
    assert!(outcomes["records"][0].get("answered_device").is_none());
    let stored = handler
        .inner
        .store
        .lock()
        .await
        .as_ref()
        .unwrap()
        .get_phone(&id, FixedClock.now_ms())
        .unwrap()
        .unwrap();
    assert_eq!(stored.answered_surface.as_deref(), Some("operator_app"));
}

async fn assert_signed_card_refused(
    module: &str,
    agent: &str,
    body: Value,
    grant: &str,
    code: &str,
) {
    let (handler, _dir) = handler().await;
    let config = body["kind"] == "config_plan";
    let id = handler
        .dispatch(&requester(module, agent), "consent.request", body)
        .await
        .unwrap()["elicitation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let features = json!([
        "secret-use-approval",
        "manifest-binding-approval",
        "config-plan"
    ]);
    let mut stamps = vec![desktop()];
    if !DEVELOPMENT_LOCAL_OPERATOR || config {
        stamps.push(Stamp::from_bind_with_local_operator(&local_bind(), true));
    }
    for stamp in stamps {
        let pending = handler
            .dispatch(
                &stamp,
                "consent.list_pending",
                json!({"clientFeatures":features}),
            )
            .await
            .unwrap();
        assert_eq!(pending["records"], json!([]));
        assert_eq!(
            handler
                .dispatch(
                    &stamp,
                    "consent.get",
                    json!({"elicitationId":id,"clientFeatures":features})
                )
                .await
                .unwrap_err()
                .code,
            "elicitation_not_found"
        );
        for choice in [grant, "decline"] {
            let error = handler
                .dispatch(
                    &stamp,
                    "consent.answer",
                    json!({
                        "elicitationId":id,"choiceId":choice,"clientFeatures":features
                    }),
                )
                .await
                .unwrap_err();
            assert_eq!(error.code, code, "{}: {choice}", stamp.local_operator);
        }
    }
    let record = handler
        .inner
        .store
        .lock()
        .await
        .as_ref()
        .unwrap()
        .get_phone(&id, FixedClock.now_ms())
        .unwrap()
        .unwrap();
    assert_eq!(record.state, State::Pending);
    assert!(record.answered_surface.is_none());
}

#[tokio::test]
async fn desktop_secret_use_requires_phone_approval() {
    let request: Value = serde_json::from_str(include_str!(
        "../../../test-vectors/secret-use-approval-v1/filer-request.json"
    ))
    .unwrap();
    assert_signed_card_refused(
        "aft",
        "agent-A",
        request["params"].clone(),
        "allow",
        "elicitation_secret_use_approval_required",
    )
    .await;
}

#[tokio::test]
async fn desktop_flow_install_secrets_require_phone_approval() {
    let request: Value = serde_json::from_str(include_str!(
        "../../../test-vectors/flow-install-secrets-v1/filing.json"
    ))
    .unwrap();
    assert_signed_card_refused(
        "prefrontal-core",
        "agent-A",
        request["params"].clone(),
        "install",
        "elicitation_secret_use_approval_required",
    )
    .await;
}

#[tokio::test]
async fn desktop_manifest_binding_requires_phone_approval() {
    let samples: Value = serde_json::from_str(include_str!(
        "../../../test-vectors/manifest-binding-v1/vault-samples.json"
    ))
    .unwrap();
    assert_signed_card_refused(
        "plexus",
        "agent_4444444444444444",
        json!({
            "kind":"manifest_binding","manifest_binding":{
                "statement_b64url": samples["samples"]["statement"]["statement_b64url"]
            }
        }),
        "allow",
        "elicitation_manifest_binding_approval_required",
    )
    .await;
}

#[tokio::test]
async fn desktop_config_plan_requires_phone_approval() {
    assert_signed_card_refused(
        "callosum",
        "",
        json!({
            "kind":"config_plan","expires_in_ms":300000,"binding":{"correlation":"fake-correlation"},"config_plan":{
                "plan_digest":"fake-plan-digest","base_revision":"42",
                "apply":{"operation":"change","rollback_to":null,"modules":[],"review":null},
                "changes":[{"module_id":"example-module","project_id":"example-project",
                    "path":"settings.enabled","old":false,"new":true,"old_source":"default",
                    "new_source":"project","apply":"live"}]
            }
        }),
        "approve",
        "elicitation_config_plan_required",
    )
    .await;
}

async fn stored_card(handler: &Handler, id: &str) -> ElicitationRecord {
    handler
        .inner
        .store
        .lock()
        .await
        .as_ref()
        .unwrap()
        .get_phone(id, FixedClock.now_ms())
        .unwrap()
        .unwrap()
}

fn encoded_envelope(envelope: &Value) -> Value {
    json!(URL_SAFE_NO_PAD.encode(serde_jcs::to_vec(envelope).unwrap()))
}

async fn assert_local_secret_approval(module: &str, agent: &str, body: Value, answer: Value) {
    let zero_device = "00000000000000000000000000000000";
    for app in [false, true] {
        let (handler, _dir) = handler().await;
        let bind = if app {
            local_bind().with_peer(
                RouteBindPeer::new()
                    .with_operator_app(Some(OperatorApp::new("alfonso-desktop", 5))),
            )
        } else {
            local_bind()
        };
        // A forged local flag must not open secret approvals in a release build.
        let local = Stamp::from_bind_with_local_operator(&bind, true);
        let id = handler
            .dispatch(&requester(module, agent), "consent.request", body.clone())
            .await
            .unwrap()["elicitation_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let mut answer = answer.clone();
        answer["elicitationId"] = json!(id);
        let envelope: Value = serde_json::from_slice(
            &URL_SAFE_NO_PAD
                .decode(answer["envelope_b64url"].as_str().unwrap())
                .unwrap(),
        )
        .unwrap();
        let mut local_envelope = envelope.clone();
        local_envelope["device_fingerprint"] = json!(zero_device);
        answer["envelope_b64url"] = encoded_envelope(&local_envelope);
        let features = answer["clientFeatures"].clone();
        let get = json!({"elicitationId":id,"clientFeatures":features});
        let listed = handler
            .dispatch(
                &local,
                "consent.list_pending",
                json!({"clientFeatures":features}),
            )
            .await
            .unwrap();
        if !DEVELOPMENT_LOCAL_OPERATOR {
            assert_eq!(listed["records"], json!([]));
            assert_eq!(
                handler
                    .dispatch(&local, "consent.get", get)
                    .await
                    .unwrap_err()
                    .code,
                "elicitation_not_found"
            );
            assert!(handler
                .dispatch(&local, "consent.answer", answer)
                .await
                .is_err());
            assert_eq!(stored_card(&handler, &id).await.state, State::Pending);
            continue;
        }
        assert_eq!(listed["records"][0]["elicitationId"], id);
        assert_eq!(
            handler.dispatch(&local, "consent.get", get).await.unwrap()["elicitationId"],
            id
        );
        for missing in [json!([]), json!(["unrelated-feature"])] {
            assert_eq!(
                handler
                    .dispatch(
                        &local,
                        "consent.list_pending",
                        json!({"clientFeatures":missing})
                    )
                    .await
                    .unwrap()["records"],
                json!([])
            );
            assert!(handler
                .dispatch(
                    &local,
                    "consent.get",
                    json!({"elicitationId":id,"clientFeatures":missing})
                )
                .await
                .is_err());
            let mut bad = answer.clone();
            bad["clientFeatures"] = missing;
            assert!(handler
                .dispatch(&local, "consent.answer", bad)
                .await
                .is_err());
        }
        for field in ["signature", "signature_b64url"] {
            for value in [Value::Null, json!("not-a-signature")] {
                let mut bad = answer.clone();
                bad[field] = value.clone();
                assert!(handler
                    .dispatch(&local, "consent.answer", bad)
                    .await
                    .is_err());
                let mut signed = local_envelope.clone();
                signed[field] = value;
                let mut bad = answer.clone();
                bad["envelope_b64url"] = encoded_envelope(&signed);
                assert!(handler
                    .dispatch(&local, "consent.answer", bad)
                    .await
                    .is_err());
            }
        }
        let mut wrong_device = answer.clone();
        wrong_device["envelope_b64url"] = encoded_envelope(&envelope);
        assert!(handler
            .dispatch(&local, "consent.answer", wrong_device)
            .await
            .is_err());
        assert_eq!(stored_card(&handler, &id).await.state, State::Pending);

        let ordinary = ordinary_card(&handler).await;
        let ordinary_answer = handler
            .dispatch(
                &local,
                "consent.answer",
                json!({"elicitationId":ordinary,"choiceId":"allow_once"}),
            )
            .await
            .unwrap();
        let allowed = handler
            .dispatch(&local, "consent.answer", answer)
            .await
            .unwrap();
        assert_eq!(allowed["ok"], true);
        assert_eq!(allowed["elicitation"]["answeredDevice"], zero_device);
        assert_eq!(
            allowed["elicitation"]["answeredSurface"],
            ordinary_answer["elicitation"]["answeredSurface"]
        );
        let stored = stored_card(&handler, &id).await;
        assert_eq!(stored.state, State::Answered);
        assert_eq!(stored.answered_device.as_deref(), Some(zero_device));
        assert_eq!(
            stored.answered_surface.as_deref(),
            app.then_some("operator_app")
        );
        let hash = format!(
            "{:x}",
            Sha256::digest(serde_jcs::to_vec(&local_envelope).unwrap())
        );
        if module == "plexus" {
            let approval = stored.manifest_binding_answer.unwrap();
            assert_eq!(approval.decision, "allow");
            assert_eq!(approval.envelope_sha256.as_deref(), Some(hash.as_str()));
        } else {
            let approval = stored.secret_use_answer.unwrap();
            assert_eq!(approval.decision, "allow");
            assert_eq!(approval.envelope_sha256.as_deref(), Some(hash.as_str()));
            assert!(approval.items.iter().all(|item| item.decision == "allow"));
        }
    }
}

#[tokio::test]
async fn local_operator_secret_use_approval_is_build_and_route_gated() {
    let request: Value = serde_json::from_str(include_str!(
        "../../../test-vectors/secret-use-approval-v2/filer-request.json"
    ))
    .unwrap();
    let answer: Value = serde_json::from_str(include_str!(
        "../../../test-vectors/secret-use-approval-v2/phone-allow.json"
    ))
    .unwrap();
    assert_local_secret_approval(
        "aft",
        "agent-A",
        request["params"].clone(),
        answer["params"].clone(),
    )
    .await;
}

#[tokio::test]
async fn local_operator_manifest_binding_approval_is_build_and_route_gated() {
    let request: Value = serde_json::from_str(include_str!(
        "../../../test-vectors/manifest-binding-v1/filer-request.json"
    ))
    .unwrap();
    let answer: Value = serde_json::from_str(include_str!(
        "../../../test-vectors/manifest-binding-v1/phone-answer-allow.json"
    ))
    .unwrap();
    assert_local_secret_approval(
        "plexus",
        "agent_4444444444444444",
        request["params"].clone(),
        answer["params"].clone(),
    )
    .await;
}

#[tokio::test]
async fn local_operator_flow_install_secrets_approval_is_build_and_route_gated() {
    let request: Value = serde_json::from_str(include_str!(
        "../../../test-vectors/flow-install-secrets-v2/filing.json"
    ))
    .unwrap();
    let answer: Value = serde_json::from_str(include_str!(
        "../../../test-vectors/flow-install-secrets-v2/allow.json"
    ))
    .unwrap();
    assert_local_secret_approval(
        "prefrontal-core",
        "agent-A",
        request["params"].clone(),
        answer["params"].clone(),
    )
    .await;
}

#[tokio::test]
async fn desktop_access_answer_audits_provenance_without_changing_user_leg() {
    let (handler, _dir) = handler().await;
    let request: Value = serde_json::from_str(include_str!(
        "../../../test-vectors/tool-permission-v1/user-open-request.json"
    ))
    .unwrap();
    let id = handler
        .dispatch(
            &requester("aft", "agent-A"),
            "consent.request",
            request["params"].clone(),
        )
        .await
        .unwrap()["elicitation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let answer = handler
        .dispatch(
            &desktop(),
            "consent.answer",
            json!({
                "elicitationId":id,"choiceId":"allow_once"
            }),
        )
        .await
        .unwrap();
    assert_eq!(answer["ok"], true);
    let record = handler
        .inner
        .store
        .lock()
        .await
        .as_ref()
        .unwrap()
        .access_get(&id)
        .unwrap()
        .unwrap();
    assert_eq!(record.answered_by.as_deref(), Some("user"));
    assert_eq!(record.attempts[0].surface, "user");
    assert_eq!(record.attempts[0].via.as_deref(), Some("operator_app"));
    assert!(record.attempts[0].device.is_none());
}

#[cfg(not(feature = "dev-local-operator"))]
async fn assert_plain_local_refused() {
    let (handler, _dir) = handler().await;
    let id = ordinary_card(&handler).await;
    let stamp = Stamp::from_bind(&local_bind());
    let error = handler
        .dispatch(
            &stamp,
            "consent.answer",
            json!({"elicitationId":id,"choiceId":"allow_once","operator_app":"alfonso-desktop"}),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "elicitation_operator_required");
    let record = handler
        .dispatch(
            &Stamp::from_principal(Some(&Principal::Direct)),
            "consent.get",
            json!({"elicitation_id":id}),
        )
        .await
        .unwrap();
    assert_eq!(record["state"], "pending");
}

#[cfg(not(feature = "dev-local-operator"))]
#[tokio::test]
async fn feature_off_refuses_plain_local_answer() {
    assert_plain_local_refused().await;
}

#[cfg(not(feature = "dev-local-operator"))]
#[tokio::test]
async fn feature_off_ignores_environment_local_operator() {
    const CHILD: &str = "CINGULATE_LOCAL_OPERATOR_ENV_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "operator_app_tests::feature_off_ignores_environment_local_operator",
            ])
            .env(CHILD, "1")
            .env("DEVELOPMENT_LOCAL_OPERATOR", "true")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        return;
    }
    assert_eq!(std::env::var("DEVELOPMENT_LOCAL_OPERATOR").unwrap(), "true");
    assert_plain_local_refused().await;
}

#[tokio::test]
async fn health_reports_local_operator_feature_state() {
    let (handler, _dir) = handler().await;
    let expected = if cfg!(feature = "dev-local-operator") {
        "dev-local-operator: on"
    } else {
        "dev-local-operator: off"
    };
    assert_eq!(handler.health_report().detail.as_deref(), Some(expected));
}
