use cingulate_decode::config_answer_bytes;
use sha2::{Digest, Sha256};

use crate::*;

impl ConsentStore {
    pub fn answer_config_plan(
        &self,
        id: &str,
        choice: &str,
        device: Option<&str>,
        statement: Option<&str>,
        signature: Option<&str>,
        now_ms: i64,
    ) -> Result<Mutation<AnswerOutcome>> {
        self.transaction(|tx| {
            let Some(mut record) = load(tx, id)?.filter(|r| retained(r, now_ms)) else {
                return Ok(Mutation { value: AnswerOutcome::NotFound, settlements: vec![] });
            };
            if record.request.kind != "config_plan" {
                return Err(refuse("elicitation_invalid_request", "statement requires config_plan"));
            }
            if record.expires_at_ms.is_some_and(|at| at <= now_ms) {
                return Err(refuse("elicitation_expired", "config_plan has expired"));
            }
            if !matches!(choice, "approve" | "decline") {
                return Err(refuse("elicitation_invalid_choice", "choice must name an option"));
            }
            if device.is_none() {
                return Err(refuse("elicitation_requester_refused", "config_plan requires a phone"));
            }
            if choice == "approve" && (statement.is_none() || signature.is_none()) {
                return Err(refuse("elicitation_invalid_request", "config_plan approve requires statement and signature_b64url"));
            }
            let bytes = statement.map(|s| config_answer_bytes(s, 4096, "statement"))
                .transpose().map_err(Error::Refused)?;
            if let Some(signature) = signature {
                config_answer_bytes(signature, 1024, "signature_b64url").map_err(Error::Refused)?;
            }
            if record.state != State::Pending {
                let same = record.state == State::Answered
                    && record.answered_choice_id.as_deref() == Some(choice)
                    && record.statement.as_deref() == statement
                    && record.signature_b64url.as_deref() == signature;
                return Ok(Mutation { value: if same { AnswerOutcome::AlreadyAnswered(record) } else { AnswerOutcome::Conflict(record) }, settlements: vec![] });
            }
            let seq = record.answer_seq.unwrap_or(0) + 1;
            tx.execute("INSERT INTO config_plan_audit(elicitation_id,answer_seq,choice,answered_at_ms,statement_sha256,statement_length) VALUES (?1,?2,?3,?4,?5,?6)",
                params![id, seq, choice, now_ms, bytes.as_ref().map(|b| format!("{:x}", Sha256::digest(b))), bytes.as_ref().map(|b| b.len() as i64)])?;
            record.answer_seq = Some(seq);
            record.statement = statement.map(str::to_owned);
            record.signature_b64url = signature.map(str::to_owned);
            record.unverified = None;
            record.answered_device = device.map(str::to_owned);
            terminal(tx, &mut record, State::Answered, Some(choice.into()), now_ms)?;
            Ok(Mutation { value: AnswerOutcome::Answered(record.clone()), settlements: vec![record] })
        })
    }

    /// Reopening and removal of the old current-answer feed row share one
    /// transaction, so readers cannot observe a pending card with a stale answer.
    pub fn report_unverified(
        &self,
        owner: &str,
        id: &str,
        seq: u64,
        code: &str,
        now_ms: i64,
    ) -> Result<ElicitationRecord> {
        self.transaction(|tx| {
            let mut record = owned(tx, owner, id, now_ms)?;
            if record.request.kind != "config_plan" || record.requester != "reserved:callosum" {
                return Err(refuse("elicitation_requester_refused", "report_unverified requires callosum config_plan"));
            }
            if !matches!(code, "time" | "plan" | "decision" | "signature" | "key") {
                return Err(refuse("elicitation_invalid_request", "code must be time|plan|decision|signature|key"));
            }
            if record.expires_at_ms.is_some_and(|at| at <= now_ms) {
                return Err(refuse("elicitation_expired", "config_plan has expired"));
            }
            if record.answer_seq != Some(seq) || seq == 0 {
                return Err(refuse("elicitation_invalid_request", "answer_seq is stale or does not name the latest answer"));
            }
            if record.state != State::Answered {
                return Err(refuse("elicitation_invalid_request", "report_unverified requires an answered card"));
            }
            tx.execute("UPDATE config_plan_audit SET unverified_code=?3,reported_at_ms=?4 WHERE elicitation_id=?1 AND answer_seq=?2", params![id,seq,code,now_ms])?;
            tx.execute("DELETE FROM elicitation_answers WHERE elicitation_id=?1", [id])?;
            tx.execute("UPDATE elicitation_records SET acknowledged=0,notice_delivered=0 WHERE elicitation_id=?1", [id])?;
            record.notice_round += 1;
            record.state = State::Pending;
            record.answered_choice_id = None;
            record.answered_at_ms = None;
            record.answered_device = None;
            record.statement = None;
            record.signature_b64url = None;
            record.unverified = Some(json!({"code":code,"reported_at_ms":now_ms}));
            save(tx, &record)?;
            enqueue_person_notice(tx, &record, "escalated")?;
            Ok(record)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    struct Scratch {
        root: PathBuf,
        store: Option<ConsentStore>,
    }
    impl Scratch {
        fn new() -> Self {
            Self {
                root: std::env::temp_dir().join(format!(
                    "cingulate-config-plan-{}-{}",
                    std::process::id(),
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                )),
                store: None,
            }
        }
        fn descriptor(&self) -> StorageDescriptor {
            storage_descriptor(None, self.root.to_str().unwrap())
        }
        fn open(&mut self) {
            self.store = Some(ConsentStore::open(&self.descriptor(), 100).unwrap().0);
        }
        fn db(&self) -> &ConsentStore {
            self.store.as_ref().unwrap()
        }
        fn file(&self) -> String {
            let sample: Value = serde_json::from_str(include_str!(
                "../../../test-vectors/config-plan-v1/filing.json"
            ))
            .unwrap();
            self.db()
                .create(
                    &RequestIdentity {
                        owner: "callosum-owner".into(),
                        requester: "reserved:callosum".into(),
                        subject: Subject {
                            agent_id: None,
                            head_agent_id: String::new(),
                            label: "This computer's settings".into(),
                            via_worker: false,
                            federated: None,
                        },
                        resolved_name: None,
                        asker: None,
                        head: None,
                    },
                    ConsentRequest::decode(sample["params"].clone()).unwrap(),
                    100,
                )
                .unwrap()
                .record
                .elicitation_id
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            drop(self.store.take());
            if self.root.exists() {
                std::fs::remove_dir_all(&self.root).unwrap();
            }
        }
    }

    #[test]
    fn config_plan_store_refuses_wrong_kind_and_missing_phone() {
        let mut f = Scratch::new();
        f.open();
        let id = f.file();
        assert_eq!(
            f.db()
                .answer_config_plan(&id, "approve", None, Some("AA"), Some("AQ"), 101)
                .unwrap_err()
                .code(),
            "elicitation_requester_refused"
        );
        f.db()
            .transaction(|tx| {
                let mut r = load(tx, &id)?.unwrap();
                r.request.kind = "consent".into();
                save(tx, &r)
            })
            .unwrap();
        assert_eq!(
            f.db()
                .answer_config_plan(
                    &id,
                    "approve",
                    Some("fake-phone"),
                    Some("AA"),
                    Some("AQ"),
                    101
                )
                .unwrap_err()
                .code(),
            "elicitation_invalid_request"
        );
        assert_eq!(
            f.db()
                .report_unverified("callosum-owner", &id, 0, "key", 101)
                .unwrap_err()
                .code(),
            "elicitation_requester_refused"
        );
    }

    #[test]
    fn config_plan_notice_rounds_order_retries_and_isolate_stale_results() {
        let mut f = Scratch::new();
        f.open();
        let id = f.file();
        f.db()
            .answer_config_plan(
                &id,
                "approve",
                Some("fake-phone"),
                Some("AA"),
                Some("AQ"),
                101,
            )
            .unwrap();
        let settled = f.db().access_notices(102).unwrap().pop().unwrap();
        assert_eq!((settled.purpose.as_str(), settled.round), ("settled", 0));
        f.db()
            .report_unverified("callosum-owner", &id, 1, "signature", 102)
            .unwrap();
        assert_eq!(f.db().access_notices(102).unwrap().len(), 1);
        f.db()
            .access_notice_result(&settled, "transport", 102)
            .unwrap();
        assert!(f.db().access_notices(102).unwrap().is_empty());
        f.db().access_notice_result(&settled, "ok", 1000).unwrap();
        let escalated = f.db().access_notices(1000).unwrap().pop().unwrap();
        assert_eq!(
            (escalated.purpose.as_str(), escalated.round),
            ("escalated", 1)
        );
        assert_eq!(escalated.body["round"], 1);
        assert!(settled.body.get("round").is_none());
        f.db()
            .answer_config_plan(&id, "decline", Some("fake-phone"), None, None, 1001)
            .unwrap();
        let second = f.db().access_notices(1001).unwrap().pop().unwrap();
        assert_eq!((second.purpose.as_str(), second.round), ("settled", 1));
        f.db().access_notice_result(&settled, "ok", 1002).unwrap();
        assert!(f.db().access_notice_pending(&second).unwrap());
        assert!(!f.db().access_notice_pending(&settled).unwrap());
        drop(f.store.take());
        f.open();
        let record = f.db().get_owned("callosum-owner", &id, 1002).unwrap();
        assert_eq!(record.notice_round, 1);
        assert_eq!(record.answer_seq, Some(2));
        assert_eq!(f.db().access_notices(1002).unwrap()[0].round, 1);
    }

    #[test]
    fn config_plan_round_migration_preserves_outbox_history_and_order() {
        let mut f = Scratch::new();
        let old = open_sqlite(&f.descriptor()).unwrap();
        old.migrate(MIGRATION_NAMESPACE, &MIGRATIONS[..11]).unwrap();
        old.with_conn(|conn| {
            conn.execute("INSERT INTO access_notices(rowid,elicitation_id,purpose,body_json,status,attempts,retry_at_ms,last_error) VALUES (17,'el_fake','settled','{}','delivered',3,123,'transport')",[])?;
            Ok(())
        }).unwrap();
        drop(old);
        f.open();
        f.db().read(|conn| {
            let row:(i64,i64,String,i64,i64,Option<String>,u64)=conn.query_row("SELECT rowid,attempts,status,retry_at_ms,COUNT(*),last_error,round FROM access_notices",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?)))?;
            assert_eq!(row,(17,3,"delivered".into(),123,1,Some("transport".into()),0));
            Ok(())
        }).unwrap();
    }
}
