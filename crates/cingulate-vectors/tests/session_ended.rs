#[path = "support/phone.rs"]
#[allow(dead_code)]
mod phone;
#[path = "support/session_ended.rs"]
mod session_ended;
mod support;

use ck_cingulate::ScopeLife;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs};
use subc_protocol::scope::ScopeKind;
use support::{app_body, stamp, worker, Fixture, ANSWERED};

#[tokio::test]
async fn session_ended_vectors_regenerate_byte_for_byte() {
    let root = support::corpus(session_ended::CORPUS);
    let mut generated = session_ended::generate().await;
    generated.insert(
        "README.md".into(),
        fs::read(root.join("README.md")).unwrap(),
    );
    let sums: String = generated
        .iter()
        .map(|(name, bytes)| format!("{:x}  {name}\n", Sha256::digest(bytes)))
        .collect();
    generated.insert("SHA256SUMS".into(), sums.into_bytes());
    let actual: BTreeSet<_> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(
        actual,
        generated.keys().cloned().collect(),
        "corpus file coverage"
    );
    for (name, bytes) in generated {
        assert_eq!(
            fs::read(root.join(&name)).unwrap(),
            bytes,
            "served vector differs: {name}"
        );
    }
}

#[tokio::test]
async fn acknowledged_card_reports_while_retained_but_reads_stay_hidden() {
    let fixture = Fixture::open(None).await;
    let requester = worker::stamp("cerebellum", ScopeKind::Head);
    let id = session_ended::answered(&fixture, &requester).await;
    let feed = fixture
        .ok(&requester, "consent.answers", json!({"since":0}))
        .await;
    fixture
        .ok(&requester, "consent.ack", json!({"cursor":feed["cursor"]}))
        .await;
    for method in ["consent.get", "consent.await"] {
        assert_eq!(
            fixture
                .call(
                    &requester,
                    method,
                    json!({"elicitation_id":id,"timeout_ms":0})
                )
                .await
                .unwrap_err()["code"],
            "elicitation_not_found",
            "{method}"
        );
    }
    assert!(fixture
        .ok(&requester, "consent.answers", json!({"since":0}))
        .await["records"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        fixture
            .call(
                &stamp("direct"),
                "consent.report_execution",
                json!({"elicitation_id":id,"execution":"executed"})
            )
            .await
            .unwrap_err()["code"],
        "elicitation_not_owner"
    );
    let report = fixture
        .ok(
            &requester,
            "consent.report_execution",
            json!({"elicitation_id":id,"execution":"executed"}),
        )
        .await;
    assert_eq!(report["execution"], "executed");
    fixture.clock.set(ANSWERED + cingulate_store::RETENTION_MS);
    assert_eq!(
        fixture
            .call(
                &requester,
                "consent.report_execution",
                json!({"elicitation_id":id,"execution":"executed"})
            )
            .await
            .unwrap_err()["code"],
        "elicitation_not_found"
    );
    fixture.handler.timer_tick().await.unwrap();
    assert_eq!(
        fixture
            .call(
                &requester,
                "consent.report_execution",
                json!({"elicitation_id":id,"execution":"executed"})
            )
            .await
            .unwrap_err()["code"],
        "elicitation_not_owner"
    );
}

#[tokio::test]
async fn ended_asker_backstops_acknowledged_card_and_requester_report_replaces_it() {
    let fixture = Fixture::open(None).await;
    fixture
        .handler
        .set_access_runtime(worker::Runtime::new(Ok(ScopeLife::Live(worker::head()))));
    let requester = worker::stamp("cerebellum", ScopeKind::Worker);
    let id = session_ended::answered(&fixture, &requester).await;
    let feed = fixture
        .ok(&requester, "consent.answers", json!({"since":0}))
        .await;
    fixture
        .ok(&requester, "consent.ack", json!({"cursor":feed["cursor"]}))
        .await;
    let runtime = worker::Runtime::new(Ok(ScopeLife::Gone));
    fixture.handler.set_access_runtime(runtime.clone());
    fixture.handler.timer_tick().await.unwrap();
    let described = runtime.described.lock().unwrap().clone();
    assert_eq!(described.len(), 1);
    assert_eq!(described[0].scope_ref, "sc_worker");
    assert_eq!(
        fixture.stored_record(&id).execution.as_deref(),
        Some("approved_not_executed")
    );
    assert_eq!(
        fixture.stored_record(&id).execution_reason.as_deref(),
        Some("session_ended")
    );
    let report = fixture
        .ok(
            &requester,
            "consent.report_execution",
            json!({"elicitation_id":id,"execution":"executed"}),
        )
        .await;
    assert_eq!(report["execution"], "executed");
    assert!(report.get("execution_reason").is_none());
    let phone = fixture
        .ok(
            &phone::phone_stamp(),
            "consent.get",
            json!({"elicitationId":id}),
        )
        .await;
    assert_eq!(phone["execution"], "executed");
    assert!(phone.get("executionReason").is_none());
}

#[tokio::test]
async fn unknown_or_alive_asker_is_not_session_ended() {
    for life in [
        Ok(ScopeLife::Unknown),
        Ok(ScopeLife::Live(worker::head())),
        Err("daemon unreachable".into()),
    ] {
        let fixture = Fixture::open(None).await;
        let requester = worker::stamp("cerebellum", ScopeKind::Head);
        let id = session_ended::answered(&fixture, &requester).await;
        let runtime = worker::Runtime::new(life);
        fixture.handler.set_access_runtime(runtime.clone());
        fixture.handler.timer_tick().await.unwrap();
        assert_eq!(runtime.described.lock().unwrap().len(), 1);
        assert!(fixture.stored_record(&id).execution.is_none());
    }
}

#[tokio::test]
async fn execution_backstop_ignores_reported_pending_unscoped_and_pruned_cards() {
    let fixture = Fixture::open(None).await;
    let requester = worker::stamp("cerebellum", ScopeKind::Head);
    let reported = session_ended::answered(&fixture, &requester).await;
    fixture
        .ok(
            &requester,
            "consent.report_execution",
            json!({"elicitation_id":reported,"execution":"executed"}),
        )
        .await;
    let pending = fixture.request(&requester, app_body()).await;
    let unscoped = session_ended::answered(&fixture, &stamp("direct")).await;
    let runtime = worker::Runtime::new(Ok(ScopeLife::Gone));
    fixture.handler.set_access_runtime(runtime.clone());
    fixture.handler.timer_tick().await.unwrap();
    assert!(runtime.described.lock().unwrap().is_empty());
    assert_eq!(
        fixture.stored_record(&reported).execution.as_deref(),
        Some("executed")
    );
    assert!(fixture.stored_record(&pending).execution.is_none());
    assert!(fixture.stored_record(&unscoped).execution.is_none());
    let pruned = session_ended::answered(&fixture, &requester).await;
    fixture.clock.set(ANSWERED + cingulate_store::RETENTION_MS);
    fixture.handler.timer_tick().await.unwrap();
    assert!(runtime.described.lock().unwrap().is_empty());
    assert_eq!(
        fixture
            .call(&requester, "consent.get", json!({"elicitation_id":pruned}))
            .await
            .unwrap_err()["code"],
        "elicitation_not_found"
    );
}

#[tokio::test]
async fn execution_backstop_pages_past_live_cards_and_backs_off_unknown_scopes() {
    let fixture = Fixture::open(None).await;
    let requester = worker::stamp("cerebellum", ScopeKind::Head);
    for _ in 0..40 {
        session_ended::answered(&fixture, &requester).await;
    }
    let runtime = worker::Runtime::new(Ok(ScopeLife::Unknown));
    fixture.handler.set_access_runtime(runtime.clone());
    fixture.handler.timer_tick().await.unwrap();
    assert_eq!(runtime.described.lock().unwrap().len(), 16);
    fixture.handler.timer_tick().await.unwrap();
    assert_eq!(runtime.described.lock().unwrap().len(), 16);
    fixture.clock.set(ANSWERED + 250);
    fixture.handler.timer_tick().await.unwrap();
    assert_eq!(runtime.described.lock().unwrap().len(), 32);
    let ended = worker::Runtime::new(Ok(ScopeLife::Gone));
    fixture.handler.set_access_runtime(ended.clone());
    fixture.clock.set(ANSWERED + 750);
    for _ in 0..6 {
        fixture.handler.timer_tick().await.unwrap();
    }
    assert_eq!(ended.described.lock().unwrap().len(), 40);
}

#[tokio::test(start_paused = true)]
async fn execution_backstop_bounds_unreachable_scope_probe_time() {
    struct HungRuntime;
    #[async_trait::async_trait]
    impl ck_cingulate::AccessRuntime for HungRuntime {
        async fn describe(&self, _: &cingulate_store::ScopeIdentity) -> Result<ScopeLife, String> {
            std::future::pending().await
        }
        async fn notice(&self, _: serde_json::Value) -> ck_cingulate::NoticeResult {
            unreachable!("no notices are queued")
        }
    }
    let fixture = Fixture::open(None).await;
    let requester = worker::stamp("cerebellum", ScopeKind::Head);
    for _ in 0..20 {
        session_ended::answered(&fixture, &requester).await;
    }
    fixture
        .handler
        .set_access_runtime(std::sync::Arc::new(HungRuntime));
    let start = tokio::time::Instant::now();
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        fixture.handler.timer_tick(),
    )
    .await
    .expect("maintenance exceeded its bounded pass")
    .unwrap();
    assert_eq!(start.elapsed(), std::time::Duration::from_secs(1));
    assert!(fixture
        .ok(&requester, "consent.answers", json!({"since":0}))
        .await["records"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r.get("execution").is_none()));
}
