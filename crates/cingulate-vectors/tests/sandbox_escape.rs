#[path = "support/phone.rs"]
#[allow(dead_code)]
mod phone;
#[path = "support/sandbox_escape.rs"]
mod sandbox_escape;
mod support;
#[path = "support/watch_pending.rs"]
#[allow(dead_code)]
mod watch_pending;

use std::sync::Arc;

use ck_cingulate::{ScopeLife, DEVELOPMENT_LOCAL_OPERATOR};
use sandbox_escape::*;
use serde_json::{json, Value};
use support::worker;

macro_rules! refused_field {
    ($name:ident, $field:literal, $value:expr, $message:literal) => {
        #[tokio::test]
        async fn $name() {
            let f = fixture().await;
            let error = f.call(&filer("aft"), "consent.request", invalid_filing($field, $value)).await
                .expect_err(concat!("filing must refuse ", stringify!($name)));
            assert_eq!(error, json!({"code":"elicitation_invalid_request","message":$message}));
            assert!(f.rows()[0].is_empty());
        }
    };
}

refused_field!(
    refuses_unknown_key,
    "agent_id",
    json!("substitute-agent"),
    "invalid sandbox_escape fields"
);
refused_field!(
    refuses_long_command,
    "command",
    json!("é".repeat(2001)),
    "invalid sandbox_escape command length"
);
refused_field!(
    refuses_command_control,
    "command",
    json!("printf example\u{0085}"),
    "invalid sandbox_escape command control character"
);
refused_field!(
    refuses_command_bidi,
    "command",
    json!("printf example\u{202e}"),
    "invalid sandbox_escape command text direction"
);
refused_field!(
    refuses_relative_cwd,
    "cwd",
    json!("example/orchard"),
    "invalid sandbox_escape cwd"
);
refused_field!(
    refuses_relative_shell,
    "shell",
    json!("sh"),
    "invalid sandbox_escape shell"
);
refused_field!(
    refuses_long_expiry,
    "expires_in_ms",
    json!(600_001),
    "invalid sandbox_escape expires_in_ms"
);
refused_field!(
    refuses_no_expiry,
    "expires_in_ms",
    json!(0),
    "invalid sandbox_escape expires_in_ms"
);
refused_field!(
    refuses_blank_call_key,
    "call_key",
    json!(" \t"),
    "invalid sandbox_escape call_key"
);
refused_field!(
    refuses_blank_command,
    "command",
    json!(" \n\t"),
    "invalid sandbox_escape command length"
);
refused_field!(
    refuses_long_reason,
    "reason",
    json!("é".repeat(161)),
    "invalid sandbox_escape reason"
);
refused_field!(
    refuses_multiline_reason,
    "reason",
    json!("first\u{2028}second"),
    "invalid sandbox_escape reason"
);
refused_field!(
    refuses_unknown_level,
    "escapes_level",
    json!("off"),
    "invalid sandbox_escape fields"
);
refused_field!(
    refuses_nonboolean_background,
    "background",
    json!("true"),
    "invalid sandbox_escape fields"
);

#[tokio::test]
async fn refuses_extra_filing_chrome_and_missing_fields() {
    let f = fixture().await;
    let aft = filer("aft");
    let mut extra = filing("extra");
    extra["title"] = json!("Replace the warning");
    let mut missing = filing("missing");
    missing["sandbox_escape"]
        .as_object_mut()
        .unwrap()
        .remove("expires_in_ms");
    for bad in [extra, missing] {
        assert_eq!(
            f.call(&aft, "consent.request", bad)
                .await
                .expect_err("filing must be the closed, complete object")["code"],
            "elicitation_invalid_request"
        );
    }
    assert!(f.rows()[0].is_empty());
}

#[tokio::test]
async fn refuses_filer_other_than_aft() {
    let f = fixture().await;
    assert_eq!(
        f.call(&filer("broca"), "consent.request", filing("wrong-filer"))
            .await
            .expect_err("a stamped agent is not enough without AFT requester authority")["code"],
        "elicitation_requester_refused"
    );
    assert!(f.rows()[0].is_empty());
}

#[tokio::test]
async fn refuses_aft_without_daemon_agent_scope() {
    let f = fixture().await;
    let mut aft = filer("aft");
    aft.scope.as_mut().unwrap().attributes.agent_id = None;
    for stamp in [
        &aft,
        &ck_cingulate::Stamp {
            scope: None,
            ..aft.clone()
        },
    ] {
        assert_eq!(
            f.call(stamp, "consent.request", filing("no-agent"))
                .await
                .expect_err("a carried agent ID cannot substitute for a daemon agent scope")
                ["code"],
            "elicitation_requester_refused"
        );
    }
    assert!(f.rows()[0].is_empty());
}

#[tokio::test]
async fn preserves_full_command_and_fixed_text_on_every_served_surface() {
    let f = fixture().await;
    let mut aft = filer("aft");
    aft.carried_agent_id = "untrusted-agent".into();
    let command = format!(" \n\t{} ", "é".repeat(1996));
    let mut request = filing("preserve");
    request["sandbox_escape"]["command"] = json!(command);
    let id = f.request(&aft, request.clone()).await;
    for surface in [phone(), desktop()] {
        let card = f.ok(&surface, "consent.get", get(&id)).await;
        assert_eq!(card["sandboxEscape"], request["sandbox_escape"]);
        assert_eq!(
            card["subject"],
            json!({"agentId":AGENT,"headAgentId":AGENT,"label":"Lark"})
        );
        assert_eq!(card["title"], "Run one command outside the sandbox?");
        assert_eq!(
            card["prompt"],
            "Runs outside the Standard sandbox, with full access to your logins and files."
        );
        assert_eq!(card["expiresAtMs"], EXPIRY);
        assert_eq!(card["default"], "decline");
        assert_eq!(card["onExpiry"], "deny");
        assert_eq!(
            card["options"],
            json!([
                {"id":"run_once","label":"Run once","effect":"grant"},
                {"id":"decline","label":"Don't run","effect":"decline"}
            ])
        );
    }
    let record = f
        .ok(&aft, "consent.get", json!({"elicitation_id":id}))
        .await;
    assert_eq!(record["sandbox_escape"], request["sandbox_escape"]);
    assert_eq!(record["expires_at_ms"], EXPIRY);
}

#[tokio::test]
async fn worker_uses_daemon_head_attribution_and_head_cannot_answer() {
    let f = fixture().await;
    let worker = from_worker();
    let id = f.request(&worker, filing("worker")).await;
    let card = f.ok(&phone(), "consent.get", get(&id)).await;
    assert_eq!(
        card["subject"],
        json!({"agentId":AGENT,"headAgentId":AGENT,"label":"Lark","viaWorker":true})
    );
    let head = filer("aft");
    assert_eq!(
        f.call(&head, "consent.answer", answer(&id, "run_once"))
            .await
            .expect_err("the worker's head must not answer the approval")["code"],
        "elicitation_operator_required"
    );
    assert_eq!(f.stored_record(&id).state, cingulate_store::State::Pending);

    let mut fallback = worker;
    fallback
        .scope
        .as_mut()
        .unwrap()
        .attributes
        .on_behalf_of_agent = None;
    let head_identity = cingulate_store::ScopeIdentity {
        agent_id: Some(AGENT.into()),
        ..worker::head()
    };
    let runtime = worker::Runtime::new(Ok(ScopeLife::Live(head_identity)));
    f.handler.set_access_runtime(runtime.clone());
    let id = f.request(&fallback, filing("parent-lookup")).await;
    assert!(!runtime.described.lock().unwrap().is_empty());
    assert_eq!(
        f.ok(&phone(), "consent.get", get(&id)).await["subject"],
        card["subject"]
    );
}

#[tokio::test]
async fn refuses_worker_with_unconfirmed_head() {
    let f = fixture().await;
    let mut worker = from_worker();
    worker.scope.as_mut().unwrap().attributes.on_behalf_of_agent = None;
    f.handler
        .set_access_runtime(worker::Runtime::new(Ok(ScopeLife::Unknown)));
    assert_eq!(
        f.call(&worker, "consent.request", filing("unknown-parent"))
            .await
            .expect_err("a worker with no confirmed head cannot file")["code"],
        "access_unavailable"
    );
    assert!(f.rows()[0].is_empty());
}

#[tokio::test]
async fn unsigned_desktop_reads_pending_but_cannot_answer() {
    let f = fixture().await;
    let id = f.request(&filer("aft"), filing("unsigned")).await;
    assert_eq!(
        f.ok(&desktop(), "consent.get", get(&id)).await["state"],
        "pending"
    );
    for choice in ["run_once", "decline"] {
        assert_eq!(
            f.call(&desktop(), "consent.answer", answer(&id, choice))
                .await
                .expect_err("feature support is not authority to answer")["code"],
            "elicitation_sandbox_escape_approval_required"
        );
    }
    assert_eq!(
        f.ok(&phone(), "consent.get", get(&id)).await["state"],
        "pending"
    );
}

#[tokio::test]
async fn phone_without_renderer_feature_cannot_see_or_answer() {
    let f = fixture().await;
    let id = f.request(&filer("aft"), filing("unsupported")).await;
    assert_eq!(
        f.call(&phone(), "consent.get", json!({"elicitationId":id}))
            .await
            .expect_err("a client without the full-command renderer must not see the card")["code"],
        "elicitation_not_found"
    );
    assert_eq!(f.call(&phone(), "consent.answer", json!({"elicitationId":id,"choiceId":"run_once","clientFeatures":["secret-use-approval"]})).await
        .expect_err("a device without renderer support cannot authorize the command")["code"], "elicitation_sandbox_escape_approval_required");
    assert_eq!(f.stored_record(&id).state, cingulate_store::State::Pending);
}

#[tokio::test]
async fn development_switch_is_required_for_local_operator() {
    let examples = development_cases().await;
    let reply = &examples["desktop-run-once"]["reply"];
    if DEVELOPMENT_LOCAL_OPERATOR {
        assert_eq!(
            reply["result"]["elicitation"]["answeredDevice"],
            "0".repeat(32)
        );
        assert_eq!(
            reply["result"]["elicitation"]["answeredChoiceId"],
            "run_once"
        );
    } else {
        assert_eq!(reply["error"]["code"], "elicitation_operator_required");
    }
}

#[tokio::test]
async fn one_approval_cannot_refresh_or_replace_the_command_even_after_restart() {
    let f = fixture().await;
    let aft = filer("aft");
    let original = filing("replay-once");
    let id = f.request(&aft, original.clone()).await;
    let mut changed = original.clone();
    changed["sandbox_escape"]["command"] = json!("printf changed");
    changed["sandbox_escape"]["cwd"] = json!("/example/greenhouse");
    changed["sandbox_escape"]["shell"] = json!("/example/bin/other-sh");
    f.clock.set(NOW + 1);
    assert_eq!(f.request(&aft, changed.clone()).await, id);
    let approved = f
        .ok(&phone(), "consent.answer", answer(&id, "run_once"))
        .await;
    assert_eq!(approved["alreadyAnswered"], false);
    assert_eq!(approved["elicitation"]["answeredDevice"], DEVICE);
    assert_eq!(
        approved["elicitation"]["sandboxEscape"],
        original["sandbox_escape"]
    );
    assert_eq!(
        f.ok(&phone(), "consent.answer", answer(&id, "run_once"))
            .await["alreadyAnswered"],
        true
    );
    let executed = f
        .ok(
            &aft,
            "consent.report_execution",
            json!({"elicitation_id":id,"execution":"executed"}),
        )
        .await;
    assert_eq!(executed["execution"], "executed");

    let restarted = support::Fixture::open_seeded(
        None,
        cingulate_store::CHANGE_LOG_ROWS,
        Arc::new(LarkResolver),
        |path| {
            f.sql()
                .execute("VACUUM INTO ?1", [path.to_str().unwrap()])
                .unwrap();
        },
    )
    .await;
    drop(f);
    restarted.clock.set(NOW + 10);
    assert_eq!(restarted.request(&aft, changed.clone()).await, id);
    let record = restarted
        .ok(
            &aft,
            "consent.await",
            json!({"elicitation_id":id,"timeout_ms":0}),
        )
        .await;
    assert_eq!(record["sandbox_escape"], original["sandbox_escape"]);
    assert_eq!(record["answered_choice_id"], "run_once");
    assert_eq!(record["execution"], "executed");
    assert_eq!(record["expires_at_ms"], EXPIRY);
    assert_eq!(restarted.rows()[0].len(), 1);

    restarted.clock.set(EXPIRY + cingulate_store::RETENTION_MS);
    restarted
        .ok(
            &phone(),
            "consent.list_pending",
            json!({"clientFeatures":[FEATURE]}),
        )
        .await;
    assert_eq!(
        restarted.request(&aft, changed).await,
        id,
        "pruning payload cannot permit the call again"
    );
}

#[tokio::test]
async fn decline_withdrawal_and_expiry_do_not_grant() {
    let f = fixture().await;
    let aft = filer("aft");
    let declined = f.request(&aft, filing("decline")).await;
    let decline = f
        .ok(&phone(), "consent.answer", answer(&declined, "decline"))
        .await;
    assert_eq!(decline["elicitation"]["answeredChoiceId"], "decline");
    assert_eq!(
        f.call(
            &aft,
            "consent.report_execution",
            json!({"elicitation_id":declined,"execution":"executed"})
        )
        .await
        .expect_err("a decline cannot be reported as an approved execution")["code"],
        "elicitation_invalid_execution"
    );

    let withdrawn = f.request(&aft, filing("withdraw")).await;
    let record = f
        .ok(
            &aft,
            "consent.withdraw",
            json!({"elicitation_id":withdrawn}),
        )
        .await;
    assert_eq!(record["state"], "withdrawn");
    assert!(record.get("answered_choice_id").is_none());
    let answer_after_withdrawal = f
        .ok(&phone(), "consent.answer", answer(&withdrawn, "run_once"))
        .await;
    assert_eq!(answer_after_withdrawal["ok"], false);
    assert_eq!(answer_after_withdrawal["elicitation"]["state"], "withdrawn");

    let expired = f.request(&aft, filing("expire")).await;
    f.clock.set(EXPIRY - 1);
    let get_pending = f
        .ok(
            &aft,
            "consent.await",
            json!({"elicitation_id":expired,"timeout_ms":0}),
        )
        .await;
    assert_eq!(get_pending["state"], "pending");
    f.clock.set(EXPIRY);
    assert_eq!(
        f.call(&phone(), "consent.answer", answer(&expired, "run_once"))
            .await
            .expect_err("answers at the deadline must refuse")["code"],
        "elicitation_expired"
    );
    let record = f
        .ok(
            &aft,
            "consent.await",
            json!({"elicitation_id":expired,"timeout_ms":0}),
        )
        .await;
    assert_eq!(record["state"], "expired");
    assert!(record.get("answered_choice_id").is_none());
}

#[tokio::test]
async fn watch_and_list_do_not_expose_card_to_old_renderers() {
    use watch_pending::{Watch, INSTANCE};
    let f = fixture().await;
    f.pin_store_instance(INSTANCE);
    let request = filing("watch");
    let id = f.request(&filer("aft"), request.clone()).await;
    let seen = f
        .ok(
            &desktop(),
            "consent.list_pending",
            json!({"clientFeatures":[FEATURE]}),
        )
        .await;
    assert_eq!(
        seen["records"][0]["sandboxEscape"]["command"],
        request["sandbox_escape"]["command"]
    );
    assert_eq!(
        f.ok(
            &phone(),
            "consent.list_pending",
            json!({"if_revision":seen["revision"]})
        )
        .await["records"],
        json!([])
    );

    let mut visible = Watch::open(
        &f.handler,
        &desktop(),
        json!({"clientFeatures":[FEATURE],"cursor":{"instance":INSTANCE,"version":0}}),
    );
    let mut hidden = Watch::open(
        &f.handler,
        &phone(),
        json!({"cursor":{"instance":INSTANCE,"version":0}}),
    );
    let card = visible.next_value().await["card"].clone();
    assert_eq!(card["sandboxEscape"], request["sandbox_escape"]);
    hidden.quiet().await;
    f.ok(&phone(), "consent.answer", answer(&id, "decline"))
        .await;
    assert_eq!(visible.next_value().await["card"]["state"], "answered");
    f.clock.set(NOW + cingulate_store::RETENTION_MS);
    f.ok(
        &phone(),
        "consent.list_pending",
        json!({"clientFeatures":[FEATURE]}),
    )
    .await;
    assert_eq!(visible.next_value().await["type"], "removed");
    hidden.quiet().await;
    for watch in [visible, hidden] {
        watch.unsubscribe();
        watch.outcome().await;
    }
}

#[tokio::test]
async fn notices_keep_the_approving_device_marker_and_never_include_command_data() {
    let f = fixture().await;
    let id = f.request(&from_worker(), filing("notices")).await;
    let notice = f
        .sql()
        .query_row(
            "SELECT body_json FROM access_notices ORDER BY rowid LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap();
    let notice: Value = serde_json::from_str(&notice).unwrap();
    assert_eq!(notice["elicitation_id"], id);
    assert_eq!(notice["purpose"], "escalated");
    assert_eq!(
        notice["fields"],
        json!({"card_kind":"sandbox_escape","subject":"Lark","expires_at_ms":EXPIRY,"phone_only":!DEVELOPMENT_LOCAL_OPERATOR,"via_worker":true})
    );
    assert_eq!(f.payloads().len(), 1);
    let pointer = f.payloads().pop().unwrap();
    assert!(pointer
        .as_object()
        .unwrap()
        .keys()
        .all(|key| ["elicitationId", "kind", "eventAtMs"].contains(&key.as_str())));
}

#[tokio::test]
async fn capability_and_served_vector_bytes_are_pinned() {
    let manifest = serde_json::to_value(ck_cingulate::manifest()).unwrap();
    assert!(manifest["capabilities"]["provides"]
        .as_array()
        .unwrap()
        .contains(&json!("sandbox-escape-approval/v1")));
    let root = support::corpus(CORPUS);
    for (name, bytes) in generate().await {
        assert_eq!(std::fs::read(root.join(name)).unwrap(), bytes, "{name}");
    }
}
