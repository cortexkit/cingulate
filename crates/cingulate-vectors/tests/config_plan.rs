#[path = "support/config_plan.rs"]
mod config_plan;
#[path = "support/phone.rs"]
#[allow(dead_code)]
mod phone;
mod support;
#[path = "support/watch_pending.rs"]
#[allow(dead_code)]
mod watch_pending;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use cingulate_decode::ConsentRequest;
use config_plan::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

fn invalid(body: Value, field: &str) {
    let e = ConsentRequest::decode(body).unwrap_err();
    assert_eq!(e.code, "elicitation_invalid_request");
    assert!(
        e.message.contains(field) || e.message == "config_plan filing has invalid fields",
        "{e}"
    );
}

#[test]
fn config_plan_closed_shapes_and_required_fields() {
    for pointer in ["", "/config_plan", "/config_plan/changes/0"] {
        let mut b = body();
        b.pointer_mut(pointer).unwrap()["unexpected"] = json!(true);
        invalid(b, "unexpected");
    }
    for pointer in ["", "/config_plan", "/config_plan/changes/0"] {
        let original = body();
        for field in original
            .pointer(pointer)
            .unwrap()
            .as_object()
            .unwrap()
            .keys()
        {
            if ["dedup_key", "other_sections", "project_id"].contains(&field.as_str()) {
                continue;
            }
            let mut b = original.clone();
            b.pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(field);
            if field == "kind" {
                assert!(ConsentRequest::decode(b).is_err());
            } else {
                invalid(b, field);
            }
        }
    }
}

#[test]
fn config_plan_lifetime_binding_and_row_count_bounds() {
    for value in [json!(0), json!(300001), Value::Null, json!("1")] {
        let mut b = body();
        b["expires_in_ms"] = value;
        invalid(b, "expires_in_ms");
    }
    for value in [Value::Null, json!([]), json!({"x":"x".repeat(4096)})] {
        let mut b = body();
        b["binding"] = value;
        invalid(b, "binding");
    }
    for n in [0, 65] {
        let mut b = body();
        b["config_plan"]["changes"] = json!(vec![body()["config_plan"]["changes"][0].clone(); n]);
        invalid(b, "changes");
    }
}

#[test]
fn config_plan_text_and_json_bounds() {
    for (path, max) in [
        ("/config_plan/plan_digest", 128),
        ("/config_plan/changes/0/module_id", 128),
        ("/config_plan/changes/0/project_id", 128),
        ("/config_plan/changes/0/path", 1024),
        ("/config_plan/changes/0/old_source", 64),
        ("/config_plan/changes/0/new_source", 64),
        ("/config_plan/changes/0/apply", 64),
    ] {
        for value in [String::new(), "x".repeat(max + 1)] {
            let mut b = body();
            *b.pointer_mut(path).unwrap() = json!(value);
            invalid(b, path.rsplit('/').next().unwrap());
        }
        let mut b = body();
        *b.pointer_mut(path).unwrap() = json!("x".repeat(max));
        ConsentRequest::decode(b).unwrap();
    }
    for revision in ["", "-1", "1.0", " 1", "a"] {
        let mut b = body();
        b["config_plan"]["base_revision"] = json!(revision);
        invalid(b, "base_revision");
    }
    for field in ["old", "new"] {
        let mut b = body();
        b["config_plan"]["changes"][0][field] = json!("x".repeat(4095));
        invalid(b, field);
        let mut b = body();
        b["config_plan"]["changes"][0][field] = Value::Null;
        ConsentRequest::decode(b).unwrap();
    }
    let mut b = body();
    b["config_plan"]["other_sections"] = json!({"x":"x".repeat(16384)});
    invalid(b, "other_sections");
    let mut b = body();
    b["config_plan"]["other_sections"] = json!([]);
    invalid(b, "other_sections");
    let mut b = body();
    b["config_plan"]["other_sections"] = Value::Null;
    invalid(b, "other_sections");
    let mut b = body();
    b["config_plan"]["changes"][0]["project_id"] = Value::Null;
    invalid(b, "project_id");
    let mut b = body();
    b["config_plan"]["changes"][0]["apply"] = json!("future_apply_mode");
    ConsentRequest::decode(b).unwrap();
}

#[tokio::test]
async fn config_plan_requester_admission_and_scope_independence() {
    let f = support::Fixture::open(None).await;
    for principal in [
        "direct",
        "callosum",
        "reserved:plexus",
        "reserved:aft",
        "reserved:prefrontal-core",
        "enrolled:callosum",
    ] {
        assert_eq!(
            f.call(&support::stamp(principal), "consent.request", body())
                .await
                .unwrap_err()["code"],
            "elicitation_requester_refused"
        );
    }
    for kind in [
        "consent",
        "manifest_binding",
        "secret_use",
        "flow_install",
        "package",
        "tool_permission",
    ] {
        assert_eq!(
            f.call(&filer(), "consent.request", json!({"kind":kind}))
                .await
                .unwrap_err()["code"],
            "elicitation_requester_refused"
        );
    }
    let mut scoped = support::worker::stamp("callosum", subc_protocol::scope::ScopeKind::Head);
    scoped.bind_session = "other-session".into();
    scoped.carried_agent_id = "not-the-subject".into();
    let id = f.request(&scoped, body()).await;
    let r = f
        .ok(&filer(), "consent.get", json!({"elicitation_id":id}))
        .await;
    assert_eq!(r["subject"], json!({"label":"This computer's settings"}));
    assert!(f.stored_record(&id).asker.is_none());
    assert!(f.stored_record(&id).head.is_none());
    let notice: String = f
        .sql()
        .query_row(
            "SELECT body_json FROM access_notices WHERE elicitation_id=?1 AND purpose='escalated'",
            [&id],
            |r| r.get(0),
        )
        .unwrap();
    let notice: Value = serde_json::from_str(&notice).unwrap();
    assert!(notice.get("asker").is_none());
    assert!(notice.get("head").is_none());
    assert_eq!(notice["fields"]["card_kind"], "config_plan");
    for method in [
        "consent.get",
        "consent.await",
        "consent.withdraw",
        "consent.report_unverified",
    ] {
        let e = f
            .call(
                &support::stamp("reserved:plexus"),
                method,
                json!({"elicitation_id":id,"timeout_ms":0,"answer_seq":1,"code":"key"}),
            )
            .await
            .unwrap_err();
        assert_eq!(e["code"], "elicitation_requester_refused");
    }
    for method in [
        "consent.detach",
        "consent.report_execution",
        "consent.answers",
        "consent.ack",
    ] {
        assert_eq!(
            f.call(&filer(), method, json!({"elicitation_id":id}))
                .await
                .unwrap_err()["code"],
            "elicitation_requester_refused"
        );
    }
}

#[tokio::test]
async fn config_plan_revision_tracks_phone_feature_projection() {
    let f = support::Fixture::open(None).await;
    let id = f.request(&filer(), body()).await;
    let hidden = f.ok(&phone(), "consent.list_pending", json!({})).await;
    let visible = f
        .ok(
            &phone(),
            "consent.list_pending",
            json!({
                "clientFeatures":["config-plan"], "if_revision":hidden["revision"]
            }),
        )
        .await;
    assert_eq!(visible["records"][0]["elicitationId"], id);
    assert_ne!(hidden["revision"], visible["revision"]);
    let hidden_again = f
        .ok(
            &phone(),
            "consent.list_pending",
            json!({"if_revision":visible["revision"]}),
        )
        .await;
    assert_eq!(hidden_again["records"], json!([]));
}

#[tokio::test]
async fn config_plan_schema14_migration_invalidates_old_replay_and_hides_new_changes() {
    use watch_pending::{Watch, INSTANCE};
    let source = support::Fixture::open(None).await;
    let id = source.request(&filer(), body()).await;
    let mut legacy = source.stored_record(&id);
    legacy.elicitation_id = "el_legacy".into();
    let raw = serde_json::to_string(&legacy).unwrap();
    drop(source);
    let f = support::Fixture::open_seeded(
        None, cingulate_store::CHANGE_LOG_ROWS, std::sync::Arc::new(support::AlfResolver),
        |path| {
            let descriptor = support::descriptor(path);
            let old = cortexkit_store::open_sqlite(&descriptor).unwrap();
            old.migrate(cingulate_store::MIGRATION_NAMESPACE, &cingulate_store::MIGRATIONS[..13]).unwrap();
            let db = rusqlite::Connection::open(path).unwrap();
            db.execute("UPDATE cingulate_change_log SET instance=?1", [INSTANCE]).unwrap();
            db.execute("INSERT INTO cingulate_changes(elicitation_id) VALUES('el_pruned_config')", []).unwrap();
            db.execute("INSERT INTO elicitation_records(elicitation_id,owner,record_json,state,expires_at_ms) VALUES('el_legacy','reserved:callosum',?1,'pending',?2)", rusqlite::params![raw,legacy.expires_at_ms]).unwrap();
            db.execute("UPDATE cingulate_id_seq SET value=10", []).unwrap();
        },
    ).await;
    let prune: i64 = f
        .sql()
        .query_row(
            "SELECT pruned_through FROM cingulate_change_log",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(prune, 2);
    assert_eq!(
        f.sql()
            .query_row(
                "SELECT config_plan FROM elicitation_records WHERE elicitation_id='el_legacy'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    let mut hidden = Watch::open(
        &f.handler,
        &phone(),
        json!({"cursor":{"instance":INSTANCE,"version":0}}),
    );
    assert_eq!(hidden.next_value().await, json!({"type":"reread"}));
    hidden.quiet().await;
    let mut next = body();
    next["config_plan"]["plan_digest"] = json!("new-plan-digest");
    f.request(&filer(), next).await;
    hidden.quiet().await;
    hidden.unsubscribe();
    hidden.outcome().await;
}

#[tokio::test]
async fn config_plan_watch_hides_unsupported_replay_live_and_pruned_removal_ids() {
    use watch_pending::{Watch, INSTANCE};
    let f = support::Fixture::open(None).await;
    f.pin_store_instance(INSTANCE);
    let id = f.request(&filer(), body()).await;
    let mut visible = Watch::open(
        &f.handler,
        &phone(),
        json!({
            "cursor":{"instance":INSTANCE,"version":0},"clientFeatures":["config-plan"]
        }),
    );
    assert_eq!(visible.next_value().await["card"]["elicitationId"], id);
    let mut hidden = Watch::open(
        &f.handler,
        &phone(),
        json!({"cursor":{"instance":INSTANCE,"version":0}}),
    );
    hidden.quiet().await;
    f.ok(&phone(), "consent.answer", answer(&id, "decline"))
        .await;
    assert_eq!(visible.next_value().await["card"]["state"], "answered");
    f.clock
        .set(support::NOW + cingulate_store::RETENTION_MS + 1);
    f.ok(
        &phone(),
        "consent.list_pending",
        json!({"clientFeatures":["config-plan"]}),
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
async fn config_plan_phone_opt_in_and_binding_privacy() {
    let f = support::Fixture::open(None).await;
    let id = f.request(&filer(), body()).await;
    let list = f.ok(&phone(), "consent.list_pending", json!({})).await;
    assert_eq!(list["records"], json!([]));
    assert_eq!(
        f.call(&phone(), "consent.get", json!({"elicitationId":id}))
            .await
            .unwrap_err()["code"],
        "elicitation_not_found"
    );
    let mut p = answer(&id, "approve");
    p["clientFeatures"] = json!([]);
    assert_eq!(
        f.call(&phone(), "consent.answer", p).await.unwrap_err()["code"],
        "elicitation_config_plan_required"
    );
    let mut impostor = phone();
    impostor.harness = "fed-sim:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into();
    assert_eq!(
        f.call(&impostor, "consent.answer", answer(&id, "decline"))
            .await
            .unwrap_err()["code"],
        "elicitation_config_plan_required"
    );
    let p = f.ok(&phone(), "consent.get", get(&id)).await;
    assert!(p.get("binding").is_none());
    assert_eq!(p["requesterName"], "This computer's settings");
    let p = f
        .ok(
            &phone(),
            "consent.list_pending",
            json!({"clientFeatures":["config-plan"]}),
        )
        .await;
    assert!(p["records"][0].get("binding").is_none());
}

#[tokio::test]
async fn config_plan_approve_requires_both_signed_fields() {
    let f = support::Fixture::open(None).await;
    let id = f.request(&filer(), body()).await;
    for field in ["statement", "signature_b64url"] {
        let mut p = answer(&id, "approve");
        p.as_object_mut().unwrap().remove(field);
        assert_eq!(
            f.call(&phone(), "consent.answer", p).await.unwrap_err()["code"],
            "elicitation_invalid_request"
        );
    }
    assert_eq!(f.stored_record(&id).answer_seq, Some(0));
}

#[tokio::test]
async fn config_plan_answer_encoding_size_and_choice_refusals() {
    let f = support::Fixture::open(None).await;
    let id = f.request(&filer(), body()).await;
    for field in ["statement", "signature_b64url"] {
        for value in [
            json!(""),
            json!("AB"),
            json!("AA=="),
            json!("!"),
            json!("A".repeat(6000)),
            Value::Null,
            json!(3),
        ] {
            let mut p = answer(&id, "approve");
            p[field] = value;
            assert_eq!(
                f.call(&phone(), "consent.answer", p).await.unwrap_err()["code"],
                "elicitation_invalid_request"
            );
        }
        let max = if field == "statement" { 4096 } else { 1024 };
        let mut p = answer(&id, "approve");
        p[field] = json!(URL_SAFE_NO_PAD.encode(vec![0; max + 1]));
        assert_eq!(
            f.call(&phone(), "consent.answer", p).await.unwrap_err()["code"],
            "elicitation_invalid_request"
        );
    }
    let mut p = answer(&id, "approve");
    p["envelope_b64url"] = json!("AA");
    assert_eq!(
        f.call(&phone(), "consent.answer", p).await.unwrap_err()["code"],
        "elicitation_invalid_request"
    );
    assert_eq!(
        f.call(&phone(), "consent.answer", answer(&id, "allow"))
            .await
            .unwrap_err()["code"],
        "elicitation_invalid_choice"
    );
}

#[tokio::test]
async fn config_plan_passthrough_and_reopen_sequence() {
    let f = support::Fixture::open(None).await;
    let id = f.request(&filer(), body()).await;
    f.ok(&phone(), "consent.answer", answer(&id, "approve"))
        .await;
    for method in ["consent.get", "consent.await"] {
        let r = f
            .ok(
                &filer(),
                method,
                json!({"elicitation_id":id,"timeout_ms":0}),
            )
            .await;
        assert_eq!(r["statement"], STATEMENT);
        assert_eq!(r["signature_b64url"], SIGNATURE);
        assert_eq!(r["answer_seq"], 1);
        assert_eq!(r["answered_choice_id"], "approve");
        assert_eq!(r["binding"], body()["binding"]);
    }
    let r = f
        .ok(&phone(), "consent.answer", answer(&id, "approve"))
        .await;
    assert_eq!(r["alreadyAnswered"], true);
    let before = f
        .ok(
            &phone(),
            "consent.list_pending",
            json!({"clientFeatures":["config-plan"]}),
        )
        .await["revision"]
        .clone();
    f.clock.set(support::NOW + 1);
    let r = f
        .ok(
            &filer(),
            "consent.report_unverified",
            json!({"elicitation_id":id,"answer_seq":1,"code":"signature"}),
        )
        .await;
    assert_eq!(r["state"], "pending");
    assert!(r.get("statement").is_none());
    let p = f.ok(&phone(), "consent.get", get(&id)).await;
    assert_eq!(
        p["unverified"],
        json!({"code":"signature","reported_at_ms":support::NOW + 1})
    );
    assert!(p.get("binding").is_none());
    let after = f
        .ok(
            &phone(),
            "consent.list_pending",
            json!({"clientFeatures":["config-plan"]}),
        )
        .await;
    assert_ne!(after["revision"], before);
    assert_eq!(
        f.payloads().last().unwrap(),
        &json!({"elicitationId":id,"kind":"elicitation","eventAtMs":support::NOW+1})
    );
    let count: i64 = f
        .sql()
        .query_row(
            "SELECT COUNT(*) FROM elicitation_answers WHERE elicitation_id=?1",
            [&id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    let notices:i64=f.sql().query_row("SELECT COUNT(*) FROM access_notices WHERE elicitation_id=?1 AND purpose='escalated' AND status='pending'",[&id],|r|r.get(0)).unwrap();
    assert_eq!(notices, 1);
    let core = f
        .ok(
            &support::stamp("reserved:prefrontal-core"),
            "consent.outcomes",
            json!({"since":0}),
        )
        .await;
    assert_eq!(core["records"], json!([]));
    let r = f
        .ok(&phone(), "consent.answer", answer_round(&id, "approve", 1))
        .await;
    assert_eq!(r["elicitation"]["answerSeq"], 2);
    let count: i64 = f
        .sql()
        .query_row(
            "SELECT COUNT(*) FROM config_plan_audit WHERE elicitation_id=?1",
            [&id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
    let notices: Vec<(String, u64)> = f
        .sql()
        .prepare("SELECT purpose,round FROM access_notices WHERE elicitation_id=?1 ORDER BY rowid")
        .unwrap()
        .query_map([&id], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        notices,
        vec![
            ("escalated".into(), 0),
            ("settled".into(), 0),
            ("escalated".into(), 1),
            ("settled".into(), 1)
        ]
    );
    let (hash,len):(String,i64)=f.sql().query_row("SELECT statement_sha256,statement_length FROM config_plan_audit WHERE elicitation_id=?1 AND answer_seq=1",[&id],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!(len, 14);
    assert_eq!(
        hash,
        format!(
            "{:x}",
            Sha256::digest([255, 0, 1, 120, 32, 110, 111, 116, 45, 74, 83, 79, 78, 10])
        )
    );
}

#[tokio::test]
async fn config_plan_report_guards() {
    let f = support::Fixture::open(None).await;
    let id = f.request(&filer(), body()).await;
    for seq in [0, 1] {
        assert_eq!(
            f.call(
                &filer(),
                "consent.report_unverified",
                json!({"elicitation_id":id,"answer_seq":seq,"code":"key"})
            )
            .await
            .unwrap_err()["code"],
            "elicitation_invalid_request"
        );
    }
    f.ok(&phone(), "consent.answer", answer(&id, "approve"))
        .await;
    for p in [
        json!({"elicitation_id":id,"answer_seq":0,"code":"key"}),
        json!({"elicitation_id":id,"answer_seq":2,"code":"key"}),
        json!({"elicitation_id":id,"answer_seq":1,"code":"unknown"}),
        json!({"elicitation_id":id,"answer_seq":1,"code":"key","extra":1}),
    ] {
        assert_eq!(
            f.call(&filer(), "consent.report_unverified", p)
                .await
                .unwrap_err()["code"],
            "elicitation_invalid_request"
        );
    }
    f.ok(
        &filer(),
        "consent.report_unverified",
        json!({"elicitation_id":id,"answer_seq":1,"code":"key"}),
    )
    .await;
    assert_eq!(
        f.call(
            &filer(),
            "consent.report_unverified",
            json!({"elicitation_id":id,"answer_seq":1,"code":"key"})
        )
        .await
        .unwrap_err()["code"],
        "elicitation_invalid_request"
    );
    f.ok(&phone(), "consent.answer", answer_round(&id, "approve", 1))
        .await;
    let e = f
        .call(
            &filer(),
            "consent.report_unverified",
            json!({"elicitation_id":id,"answer_seq":1,"code":"key"}),
        )
        .await
        .unwrap_err();
    assert!(e["message"].as_str().unwrap().contains("answer_seq"));
    f.clock.set(support::NOW + 300000);
    assert_eq!(
        f.call(
            &filer(),
            "consent.report_unverified",
            json!({"elicitation_id":id,"answer_seq":2,"code":"key"})
        )
        .await
        .unwrap_err()["code"],
        "elicitation_expired"
    );
}

#[tokio::test]
async fn config_plan_reopen_rollback_keeps_settlement_and_audit() {
    let f = support::Fixture::open(None).await;
    let id = f.request(&filer(), body()).await;
    f.ok(&phone(), "consent.answer", answer(&id, "approve"))
        .await;
    // Abort after feed and notice deletion, before saving the pending record.
    f.sql().execute_batch("CREATE TRIGGER fail_reopen BEFORE UPDATE ON elicitation_records WHEN NEW.state='pending' BEGIN SELECT RAISE(ABORT,'injected crash'); END;").unwrap();
    assert!(f
        .call(
            &filer(),
            "consent.report_unverified",
            json!({"elicitation_id":id,"answer_seq":1,"code":"plan"})
        )
        .await
        .is_err());
    assert_eq!(f.stored_record(&id).state, cingulate_store::State::Answered);
    let count: i64 = f
        .sql()
        .query_row(
            "SELECT COUNT(*) FROM elicitation_answers WHERE elicitation_id=?1",
            [&id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
    let reported: Option<i64> = f
        .sql()
        .query_row(
            "SELECT reported_at_ms FROM config_plan_audit WHERE elicitation_id=?1",
            [&id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(reported, None);
    f.sql().execute_batch("DROP TRIGGER fail_reopen").unwrap();
    f.ok(
        &filer(),
        "consent.report_unverified",
        json!({"elicitation_id":id,"answer_seq":1,"code":"plan"}),
    )
    .await;
}

#[tokio::test]
async fn config_plan_decline_withdraw_expiry_and_retention() {
    for mode in ["decline", "signed_decline", "withdraw", "expiry"] {
        let f = support::Fixture::open(None).await;
        let id = f.request(&filer(), body()).await;
        match mode {
            "decline" => {
                f.ok(&phone(), "consent.answer", answer(&id, "decline"))
                    .await;
            }
            "signed_decline" => {
                let mut p = answer(&id, "approve");
                p["choiceId"] = json!("decline");
                f.ok(&phone(), "consent.answer", p).await;
            }
            "withdraw" => {
                f.ok(&filer(), "consent.withdraw", json!({"elicitation_id":id}))
                    .await;
            }
            _ => f.clock.set(support::NOW + 300000),
        }
        if ["withdraw", "expiry"].contains(&mode) {
            let r = f
                .call(&phone(), "consent.answer", answer(&id, "approve"))
                .await;
            if mode == "expiry" {
                assert_eq!(r.unwrap_err()["code"], "elicitation_expired");
            } else {
                assert_eq!(r.unwrap()["code"], "conflict");
            }
        }
        let r = f
            .ok(&filer(), "consent.get", json!({"elicitation_id":id}))
            .await;
        assert_ne!(r["state"], "pending");
        assert_ne!(r["answered_choice_id"], "approve");
        f.clock
            .set(support::NOW + 300000 + cingulate_store::RETENTION_MS);
        assert_eq!(
            f.call(&filer(), "consent.get", json!({"elicitation_id":id}))
                .await
                .unwrap_err()["code"],
            "elicitation_not_found"
        );
    }
}

#[tokio::test]
async fn config_plan_reopened_projections_expose_notice_round() {
    fn assert_round(record: &Value, round: u64) {
        if round == 0 {
            assert!(record.get("round").is_none(), "{record}");
        } else {
            assert_eq!(record["round"].as_u64(), Some(round), "{record}");
        }
        assert!(record.get("noticeRound").is_none());
        assert!(record.get("notice_round").is_none());
    }

    for filing in [
        all_sections_body(),
        rollback_body(),
        flag_only_body(),
        route_rebind_body(),
    ] {
        let f = support::Fixture::open(None).await;
        let mut watch = watch_pending::Watch::open(
            &f.handler,
            &phone(),
            json!({"clientFeatures":["config-plan"]}),
        );
        assert_eq!(watch.next_value().await, json!({"type":"reread"}));
        let expected = filing["config_plan"].clone();
        let id = f.request(&filer(), filing).await;
        for round in 0..=2 {
            let item = watch.next_value().await;
            assert_eq!(item["type"], "changed");
            assert_eq!(item["card"]["elicitationId"], id);
            assert_round(&item["card"], round);
            assert_eq!(item["card"]["configPlan"], expected);
            let list = f
                .ok(
                    &phone(),
                    "consent.list_pending",
                    json!({"clientFeatures":["config-plan"]}),
                )
                .await;
            assert_round(&list["records"][0], round);
            assert_eq!(list["records"][0]["configPlan"], expected);
            for state in ["pending", "answered"] {
                let phone_record = f.ok(&phone(), "consent.get", get(&id)).await;
                assert_eq!(phone_record["state"], state);
                assert_round(&phone_record, round);
                assert_eq!(phone_record["configPlan"], expected);
                for method in ["consent.get", "consent.await"] {
                    let requester_record = f
                        .ok(
                            &filer(),
                            method,
                            json!({"elicitation_id":id,"timeout_ms":0}),
                        )
                        .await;
                    assert_eq!(requester_record["state"], state);
                    assert_round(&requester_record, round);
                    assert_eq!(requester_record["config_plan"], expected);
                }
                let notice_round: u64 = f.sql().query_row(
                "SELECT round FROM access_notices WHERE elicitation_id=?1 ORDER BY rowid DESC LIMIT 1",
                [&id], |row| row.get(0),
            ).unwrap();
                assert_eq!(notice_round, round);
                if state == "pending" {
                    let answer = f
                        .ok(
                            &phone(),
                            "consent.answer",
                            answer_round(&id, "approve", round),
                        )
                        .await;
                    assert_round(&answer["elicitation"], round);
                    assert_eq!(answer["elicitation"]["configPlan"], expected);
                    let answered = watch.next_value().await;
                    assert_eq!(answered["type"], "changed");
                    assert_eq!(answered["card"]["state"], "answered");
                    assert_round(&answered["card"], round);
                    assert_eq!(answered["card"]["configPlan"], expected);
                }
            }
            if round < 2 {
                f.ok(
                    &filer(),
                    "consent.report_unverified",
                    json!({"elicitation_id":id,"answer_seq":round+1,"code":"signature"}),
                )
                .await;
            }
        }
        watch.unsubscribe();
        assert!(matches!(
            watch.outcome().await,
            subc_client_rs::HandlerOutcome::Streamed
        ));
    }
}

#[tokio::test]
async fn config_plan_020_stored_card_keeps_exact_plan_shape() {
    let sample: Value = serde_json::from_slice(
        &std::fs::read(support::corpus("config-plan-v2").join("change-all-sections.json")).unwrap(),
    )
    .unwrap();
    let expected = &sample["params"]["config_plan"];
    let f = support::Fixture::open(None).await;
    let id = f.request(&filer(), body()).await;
    f.sql().execute(
        "UPDATE elicitation_records SET record_json=json_set(record_json,'$.request.config_plan',json(?1)) WHERE elicitation_id=?2",
        [expected.to_string(), id.clone()],
    ).unwrap();
    assert_eq!(
        serde_json::to_value(f.stored_record(&id).request.config_plan).unwrap(),
        *expected
    );
    assert_eq!(
        f.ok(&phone(), "consent.get", get(&id)).await["configPlan"],
        *expected
    );
    assert_eq!(
        f.ok(&filer(), "consent.get", json!({"elicitation_id":id}))
            .await["config_plan"],
        *expected
    );
}

#[tokio::test]
async fn config_plan_legacy_stored_card_reads_decline_only() {
    let f = support::Fixture::open(None).await;
    let id = f.request(&filer(), body()).await;
    f.sql().execute(
        "UPDATE elicitation_records SET record_json=json_remove(record_json,'$.request.config_plan.apply') WHERE elicitation_id=?1",
        [&id],
    ).unwrap();
    let record = f.ok(&phone(), "consent.get", get(&id)).await;
    assert!(record["configPlan"].get("apply").is_none());
    assert_eq!(
        record["options"],
        json!([{"id":"decline","label":"Decline","effect":"decline"}])
    );
    for method in ["consent.get", "consent.await"] {
        let record = f
            .ok(
                &filer(),
                method,
                json!({"elicitation_id":id,"timeout_ms":0}),
            )
            .await;
        assert_eq!(record["options"].as_array().unwrap().len(), 1);
        assert_eq!(record["options"][0]["id"], "decline");
    }
    assert_eq!(
        f.call(&phone(), "consent.answer", answer(&id, "approve"))
            .await
            .unwrap_err()["code"],
        "elicitation_invalid_choice"
    );
    f.ok(&phone(), "consent.answer", answer(&id, "decline"))
        .await;
}

#[tokio::test]
async fn config_plan_round_guard_refuses_stale_answers_without_writes() {
    let f = support::Fixture::open(None).await;
    let id = f.request(&filer(), body()).await;
    f.ok(&phone(), "consent.answer", answer(&id, "approve"))
        .await;
    f.ok(
        &filer(),
        "consent.report_unverified",
        json!({"elicitation_id":id,"answer_seq":1,"code":"signature"}),
    )
    .await;
    for expected in [None, Some(0), Some(2)] {
        let mut params = answer(&id, "approve");
        if let Some(round) = expected {
            params["expectedRound"] = json!(round);
        }
        assert_eq!(
            f.ok(&phone(), "consent.answer", params).await,
            json!({"ok":false,"code":"round_changed","detail":{"currentRound":1}})
        );
        let record = f.stored_record(&id);
        assert_eq!(record.answer_seq, Some(1));
        assert_eq!(record.state, cingulate_store::State::Pending);
        assert!(record.statement.is_none());
    }
    let audits: i64 = f
        .sql()
        .query_row(
            "SELECT COUNT(*) FROM config_plan_audit WHERE elicitation_id=?1",
            [&id],
            |r| r.get(0),
        )
        .unwrap();
    let notices: i64 = f
        .sql()
        .query_row(
            "SELECT COUNT(*) FROM access_notices WHERE elicitation_id=?1",
            [&id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(audits, 1);
    assert_eq!(notices, 3);
}

#[tokio::test]
async fn config_plan_round_guard_rejects_reused_statements_and_keeps_exact_retries() {
    let f = support::Fixture::open(None).await;
    let id = f.request(&filer(), body()).await;
    f.ok(&phone(), "consent.answer", answer(&id, "approve"))
        .await;
    assert_eq!(
        f.ok(&phone(), "consent.answer", answer(&id, "approve"))
            .await["alreadyAnswered"],
        true
    );
    for round in 1..=2 {
        f.ok(
            &filer(),
            "consent.report_unverified",
            json!({"elicitation_id":id,"answer_seq":round,"code":"signature"}),
        )
        .await;
        let mut reused = answer(&id, "approve");
        reused["expectedRound"] = json!(round);
        assert_eq!(
            f.ok(&phone(), "consent.answer", reused).await,
            json!({"ok":false,"code":"statement_reused"})
        );
        let record = f.stored_record(&id);
        assert_eq!(record.answer_seq, Some(round));
        assert_eq!(record.state, cingulate_store::State::Pending);
        let mut fresh = answer(&id, "approve");
        fresh["expectedRound"] = json!(round);
        fresh["statement"] =
            json!(URL_SAFE_NO_PAD.encode(format!("fresh statement for round {round}")));
        assert_eq!(
            f.ok(&phone(), "consent.answer", fresh.clone()).await["alreadyAnswered"],
            false
        );
        assert_eq!(
            f.ok(&phone(), "consent.answer", fresh).await["alreadyAnswered"],
            true
        );
    }
    let mut changed_signature = answer(&id, "approve");
    changed_signature["expectedRound"] = json!(2);
    changed_signature["statement"] = json!(URL_SAFE_NO_PAD.encode("fresh statement for round 2"));
    changed_signature["signature_b64url"] = json!("BQ");
    assert_eq!(
        f.ok(&phone(), "consent.answer", changed_signature).await,
        json!({"ok":false,"code":"statement_reused"})
    );
    let audits: i64 = f
        .sql()
        .query_row(
            "SELECT COUNT(*) FROM config_plan_audit WHERE elicitation_id=?1",
            [&id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(audits, 3);
}

#[tokio::test]
async fn config_plan_expected_round_is_integer_but_other_kinds_ignore_it() {
    let f = support::Fixture::open(None).await;
    let id = f.request(&filer(), body()).await;
    for value in [Value::Null, json!(-1), json!(1.5), json!("0"), json!(false)] {
        let mut params = answer(&id, "decline");
        params["expectedRound"] = value;
        let error = f
            .call(&phone(), "consent.answer", params)
            .await
            .unwrap_err();
        assert_eq!(error["code"], "elicitation_invalid_request");
        assert!(error["message"].as_str().unwrap().contains("expectedRound"));
    }
    let id = f
        .request(
            &support::stamp("reserved:prefrontal-core"),
            phone::package_body(),
        )
        .await;
    assert_eq!(f.ok(&phone::phone_stamp(), "consent.answer", json!({"elicitationId":id,"choiceId":"allow","expectedRound":{"ignored":true},"unknown":true})).await["ok"], true);
}

#[tokio::test]
async fn config_plan_v3_keeps_revision_only_success_compatibility_and_refusal_codes() {
    fn remove_revision_suffix(value: &mut Value) {
        match value {
            Value::Object(fields) => {
                for (key, value) in fields {
                    if key == "revision" {
                        if let Some(revision) =
                            value.as_str().and_then(|s| s.strip_suffix(":config-plan"))
                        {
                            *value = json!(revision);
                        }
                    } else {
                        remove_revision_suffix(value);
                    }
                }
            }
            Value::Array(items) => {
                for item in items {
                    remove_revision_suffix(item);
                }
            }
            _ => {}
        }
    }
    for (name, bytes) in generate().await {
        let mut current: Value = serde_json::from_slice(&bytes).unwrap();
        let mut historical: Value = serde_json::from_slice(
            &std::fs::read(support::corpus("config-plan-v3").join(&name)).unwrap(),
        )
        .unwrap();
        current["tag"] = json!("config-plan-v3");
        remove_revision_suffix(&mut current);
        // Human refusal text is deliberately not a pinned protocol contract.
        // Codes, request shapes and every non-message field still match v3.
        for case in [&mut current, &mut historical] {
            if let Some(error) = case
                .pointer_mut("/reply/error")
                .and_then(Value::as_object_mut)
            {
                assert!(error.remove("message").unwrap().is_string());
            }
        }
        assert_eq!(current, historical, "{name}");
    }
}

#[tokio::test]
async fn config_plan_served_vectors_and_checksums() {
    let manifest = serde_json::to_value(ck_cingulate::manifest()).unwrap();
    assert!(manifest["capabilities"]["provides"]
        .as_array()
        .unwrap()
        .contains(&json!("consent-config-plan/v1")));
    for (name, bytes) in generate().await {
        assert_eq!(
            std::fs::read(support::corpus(CORPUS).join(name)).unwrap(),
            bytes
        );
    }
    for corpus in ["config-plan-v1", "config-plan-v2", "config-plan-v3", CORPUS] {
        let sums = std::fs::read_to_string(support::corpus(corpus).join("SHA256SUMS")).unwrap();
        for line in sums.lines() {
            let (hash, name) = line.split_once("  ").unwrap();
            assert_eq!(
                hash,
                format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(support::corpus(corpus).join(name)).unwrap())
                )
            );
        }
    }
}
