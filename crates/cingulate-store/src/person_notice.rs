use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::Value;

use crate::{ids, load, ConsentStore, ElicitationRecord, Result, ScopeIdentity, State};

// Declaration order pins the stored JSON bytes independently of map ordering.
#[derive(Serialize)]
struct PersonNotice<'a> {
    elicitation_id: &'a str,
    purpose: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    round: Option<u64>,
    // Absent when the card was filed without a daemon-stamped session scope,
    // such as an install card core files itself. Core then pushes the card
    // with no agent label.
    #[serde(skip_serializing_if = "Option::is_none")]
    asker: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    head: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fields: Option<PersonFields<'a>>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum PersonFields<'a> {
    Escalated {
        card_kind: &'a str,
        subject: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        expires_at_ms: Option<i64>,
        // Presence tells prefrontal the marker is authoritative; absence means an
        // older Cingulate, so prefrontal must fall back to its phone-only kind list.
        // True requires a phone push regardless of operator presence because a
        // desktop answer would be refused.
        phone_only: bool,
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        via_worker: bool,
    },
    Settled {
        outcome: &'a str,
    },
}

impl ElicitationRecord {
    fn person_notice<'a>(
        &'a self,
        purpose: &'a str,
        asker: Option<&ScopeIdentity>,
    ) -> Option<PersonNotice<'a>> {
        if self.request.at_console.is_some() {
            return None;
        }
        let card_kind = match self.request.kind.as_str() {
            "consent" | "package" | "flow_decision" => "consent",
            "secret_use" => "secret_use",
            "manifest_binding" => "manifest_binding",
            "broker_action" => "broker_action",
            "sandbox_escape" => "sandbox_escape",
            "config_plan" => "config_plan",
            "flow_install" => "flow_install",
            _ => return None,
        };
        let fields = match purpose {
            "escalated" => {
                let mut subject: String = self.subject.label.chars().take(200).collect();
                if self.subject.label.chars().count() > 200 {
                    subject.pop();
                    subject.push('…');
                }
                Some(PersonFields::Escalated {
                    card_kind,
                    subject,
                    expires_at_ms: self.expires_at_ms,
                    phone_only: self.request.answer_needs_phone(),
                    via_worker: self.subject.via_worker,
                })
            }
            "settled" => Some(PersonFields::Settled {
                outcome: match self.state {
                    State::Answered => "answered",
                    State::Expired => "expired",
                    _ => return None,
                },
            }),
            "withdrawn" => None,
            _ => return None,
        };
        Some(PersonNotice {
            elicitation_id: &self.elicitation_id,
            purpose,
            round: (self.notice_round > 0).then_some(self.notice_round),
            asker: asker.map(ScopeIdentity::target),
            head: self.head.as_ref().map(|head| head.target()),
            fields,
        })
    }
}

pub(crate) fn enqueue_person_notice(
    conn: &Connection,
    record: &ElicitationRecord,
    purpose: &str,
) -> Result<()> {
    if let Some(body) = person_notice_body(conn, record, purpose)? {
        insert_person_notice(
            conn,
            &record.elicitation_id,
            purpose,
            record.notice_round,
            &body,
        )?;
    }
    Ok(())
}

fn person_notice_body(
    conn: &Connection,
    record: &ElicitationRecord,
    purpose: &str,
) -> Result<Option<String>> {
    // Older records predate the stored asker field. Their owner still contains
    // the daemon's scope coordinates; a bare requester-supplied session is not
    // sufficient to address a notice. A card with no attested scope at all still
    // queues its notices, with `asker` omitted, so the person's phone is told.
    let asker = match &record.asker {
        Some(asker) => Some(asker.clone()),
        None => {
            let owner: String = conn.query_row(
                "SELECT owner FROM elicitation_records WHERE elicitation_id=?1",
                [&record.elicitation_id],
                |row| row.get(0),
            )?;
            serde_json::from_str::<(Value, String)>(&owner)
                .ok()
                .and_then(|(_, session)| {
                    serde_json::from_str::<(Value, String, u64)>(&session).ok()
                })
                .map(|(_, scope_ref, scope_epoch)| ScopeIdentity {
                    scope_ref,
                    scope_epoch,
                    agent_id: None,
                })
        }
    };
    record
        .person_notice(purpose, asker.as_ref())
        .map(|body| serde_json::to_string(&body).map_err(crate::Error::from))
        .transpose()
}

fn insert_person_notice(
    conn: &Connection,
    id: &str,
    purpose: &str,
    round: u64,
    body: &str,
) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO access_notices(elicitation_id,purpose,body_json,round) VALUES (?1,?2,?3,?4)",
        params![id, purpose, body, round],
    )?;
    Ok(())
}

impl ConsentStore {
    pub(crate) fn backfill_person_notices(&self) -> Result<()> {
        // An already-backfilled store must not open a backfill write transaction.
        let missing = self.read(|conn| {
            let mut missing = Vec::new();
            for id in ids(conn, "SELECT elicitation_id FROM elicitation_records AS records WHERE state='pending' AND NOT EXISTS (SELECT 1 FROM access_notices AS notices WHERE notices.elicitation_id=records.elicitation_id AND notices.purpose='escalated' AND notices.round=COALESCE(json_extract(records.record_json,'$.noticeRound'),0)) ORDER BY elicitation_id", [])? {
                if let Some(record) = load(conn, &id)? {
                    if let Some(body) = person_notice_body(conn, &record, "escalated")? {
                        missing.push((id, record.notice_round, body));
                    }
                }
            }
            Ok(missing)
        })?;
        if missing.is_empty() {
            return Ok(());
        }
        self.transaction(|tx| {
            for (id, round, body) in missing {
                insert_person_notice(tx, &id, "escalated", round, &body)?;
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::scratch_root;
    use crate::{RequestIdentity, StorageBackend, Subject};
    use cingulate_decode::ConsentRequest;
    use std::{path::PathBuf, time::Duration};

    struct Scratch(PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn identity() -> RequestIdentity {
        RequestIdentity {
            owner: "attested-owner".into(),
            requester: "reserved:cerebellum".into(),
            subject: Subject {
                agent_id: None,
                head_agent_id: String::new(),
                label: "ALF".into(),
                via_worker: false,
                federated: None,
            },
            resolved_name: None,
            asker: Some(ScopeIdentity {
                scope_ref: "sc_asker".into(),
                scope_epoch: 3,
                agent_id: None,
            }),
            head: None,
        }
    }

    fn assert_escalated_body(request: Value, fields: &str) {
        assert_escalated_body_for(identity(), request, fields);
    }

    fn assert_escalated_body_for(identity: RequestIdentity, request: Value, fields: &str) {
        let scratch = Scratch(scratch_root("cingulate-phone-notice"));
        let descriptor = crate::storage_descriptor(None, scratch.0.to_str().unwrap());
        let store = ConsentStore::open(&descriptor, 100).unwrap().0;
        let record = store
            .create(&identity, ConsentRequest::decode(request).unwrap(), 100)
            .unwrap()
            .record;
        let expected = format!(
            r#"{{"elicitation_id":"{}","purpose":"escalated","asker":{{"scope_epoch":3,"scope_ref":"sc_asker"}},"fields":{fields}}}"#,
            record.elicitation_id
        );
        let notices = store.access_notices(100).unwrap();
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].body_json, expected);

        // Upgraded stores recreate missing notices through the same builder.
        store
            .transaction(|tx| {
                tx.execute("DELETE FROM access_notices", [])?;
                Ok(())
            })
            .unwrap();
        store.backfill_person_notices().unwrap();
        let notices = store.access_notices(100).unwrap();
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].body_json, expected);

        let asker = identity.asker;
        let withdrawn = record.person_notice("withdrawn", asker.as_ref()).unwrap();
        assert_eq!(
            serde_json::to_string(&withdrawn).unwrap(),
            format!(
                r#"{{"elicitation_id":"{}","purpose":"withdrawn","asker":{{"scope_epoch":3,"scope_ref":"sc_asker"}}}}"#,
                record.elicitation_id
            )
        );
        let mut settled = record;
        settled.state = State::Answered;
        let body = settled.person_notice("settled", asker.as_ref()).unwrap();
        assert_eq!(
            serde_json::to_string(&body).unwrap(),
            format!(
                r#"{{"elicitation_id":"{}","purpose":"settled","asker":{{"scope_epoch":3,"scope_ref":"sc_asker"}},"fields":{{"outcome":"answered"}}}}"#,
                settled.elicitation_id
            )
        );
    }

    fn flow_install_request() -> Value {
        let vector: Value = serde_json::from_str(include_str!(
            "../../../test-vectors/flow-install-secrets-v2/filing.json"
        ))
        .unwrap();
        vector["params"].clone()
    }

    #[test]
    fn escalated_phone_delivery_secret_use() {
        let vector: Value = serde_json::from_str(include_str!(
            "../../../test-vectors/secret-use-approval-v2/filer-request.json"
        ))
        .unwrap();
        assert_escalated_body(
            vector["params"].clone(),
            if cfg!(feature = "dev-local-operator") {
                r#"{"card_kind":"secret_use","subject":"ALF","expires_at_ms":1700000600000,"phone_only":false}"#
            } else {
                r#"{"card_kind":"secret_use","subject":"ALF","expires_at_ms":1700000600000,"phone_only":true}"#
            },
        );
    }

    #[test]
    fn escalated_phone_delivery_manifest_binding() {
        let samples: Value = serde_json::from_str(include_str!(
            "../../../test-vectors/manifest-binding-v1/vault-samples.json"
        ))
        .unwrap();
        assert_escalated_body(
            serde_json::json!({"kind":"manifest_binding","manifest_binding":{
                "statement_b64url":samples["samples"]["statement"]["statement_b64url"]
            }}),
            if cfg!(feature = "dev-local-operator") {
                r#"{"card_kind":"manifest_binding","subject":"ALF","expires_at_ms":1794441700123,"phone_only":false}"#
            } else {
                r#"{"card_kind":"manifest_binding","subject":"ALF","expires_at_ms":1794441700123,"phone_only":true}"#
            },
        );
    }

    #[test]
    fn escalated_phone_delivery_config_plan() {
        assert_escalated_body(
            serde_json::json!({"kind":"config_plan","expires_in_ms":300000,"binding":{"correlation":"fake-correlation"},"config_plan":{
                "plan_digest":"fake-plan-digest","base_revision":"42",
                "apply":{"operation":"change","rollback_to":null,"modules":[],"review":null},"changes":[{
                    "module_id":"example-module","project_id":"example-project","path":"settings.enabled",
                    "old":false,"new":true,"old_source":"default","new_source":"project","apply":"live"
                }],"other_sections":{}
            }}),
            r#"{"card_kind":"config_plan","subject":"ALF","expires_at_ms":300100,"phone_only":true}"#,
        );
    }

    #[test]
    fn escalated_phone_delivery_flow_install_with_secrets() {
        assert_escalated_body(
            flow_install_request(),
            if cfg!(feature = "dev-local-operator") {
                r#"{"card_kind":"flow_install","subject":"ALF","expires_at_ms":1700000600000,"phone_only":false}"#
            } else {
                r#"{"card_kind":"flow_install","subject":"ALF","expires_at_ms":1700000600000,"phone_only":true}"#
            },
        );
    }

    #[test]
    fn escalated_phone_delivery_consent_has_false_flag() {
        assert_escalated_body(
            serde_json::from_str(include_str!(
                "../../../test-vectors/elicitation-request-v1/valid/app-grant-once.json"
            ))
            .unwrap(),
            r#"{"card_kind":"consent","subject":"ALF","expires_at_ms":60100,"phone_only":false}"#,
        );
    }

    #[test]
    fn escalated_worker_consent_includes_via_worker() {
        let mut identity = identity();
        identity.subject.via_worker = true;
        assert_escalated_body_for(
            identity,
            serde_json::from_str(include_str!(
                "../../../test-vectors/elicitation-request-v1/valid/app-grant-once.json"
            ))
            .unwrap(),
            r#"{"card_kind":"consent","subject":"ALF","expires_at_ms":60100,"phone_only":false,"via_worker":true}"#,
        );
    }

    #[test]
    fn escalated_worker_sandbox_escape_includes_via_worker() {
        let mut identity = identity();
        identity.subject.via_worker = true;
        assert_escalated_body_for(
            identity,
            serde_json::json!({"kind":"sandbox_escape", "sandbox_escape":{
                "call_key":"example", "command":"printf example", "cwd":"/example",
                "shell":"/bin/sh", "escapes_level":"standard", "background":true,
                "reason":"Show the example.", "expires_in_ms":600000
            }}),
            r#"{"card_kind":"sandbox_escape","subject":"ALF","expires_at_ms":600100,"phone_only":true,"via_worker":true}"#,
        );
    }

    #[test]
    fn escalated_phone_delivery_flow_install_without_secrets_has_false_flag() {
        let mut request = flow_install_request();
        let flow = request["flow_install"].as_object_mut().unwrap();
        flow.remove("secrets");
        flow.remove("flow_identity");
        assert_escalated_body(
            request,
            r#"{"card_kind":"flow_install","subject":"ALF","phone_only":false}"#,
        );
    }

    #[test]
    fn fully_backfilled_cards_need_no_write_transaction() {
        let scratch = Scratch(scratch_root("cingulate-backfill"));
        let descriptor = crate::storage_descriptor(None, scratch.0.to_str().unwrap());
        let store = ConsentStore::open(&descriptor, 100).unwrap().0;
        let identity = identity();
        let request = ConsentRequest::decode(
            serde_json::from_str(include_str!(
                "../../../test-vectors/elicitation-request-v1/valid/app-grant-once.json"
            ))
            .unwrap(),
        )
        .unwrap();
        store.create(&identity, request, 100).unwrap();
        store
            .read(|conn| {
                conn.busy_timeout(Duration::ZERO)?;
                Ok(())
            })
            .unwrap();
        let StorageBackend::Sqlite { path } = &descriptor.backend else {
            unreachable!()
        };
        let writer = Connection::open(path).unwrap();
        writer.execute_batch("BEGIN IMMEDIATE").unwrap();
        let writes = store.write_count();
        assert_eq!(store.access_notices(100).unwrap().len(), 1);
        // A second connection owns the write lock, but a no-op backfill needs only reads.
        store.backfill_person_notices().unwrap();
        assert_eq!(store.write_count(), writes);
        writer.execute_batch("ROLLBACK").unwrap();
    }
}
