//! Permission asks use separate, durable rows: the ordinary card expiry and
//! retention sweep must never settle or erase a held tool call.
use cingulate_decode::AccessRequest;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{ConsentStore, Result};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScopeIdentity {
    pub scope_ref: String,
    pub scope_epoch: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
}
impl ScopeIdentity {
    pub fn target(&self) -> Value {
        json!({"scope_ref":self.scope_ref,"scope_epoch":self.scope_epoch})
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AccessIdentity {
    pub principal: String,
    pub asker: ScopeIdentity,
    /// The daemon-stamped asker scope owner; absent on older records.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_owner: Option<String>,
    pub scope_kind: String,
    pub harness: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<ScopeIdentity>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnswerAttempt {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub malformed_params: Option<Value>,
    pub surface: String,
    pub principal: String,
    pub at_ms: i64,
    pub answer: String,
    pub note: Option<String>,
    pub answered_by: Option<ScopeIdentity>,
    pub result: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AccessRecord {
    pub elicitation_id: String,
    pub request: AccessRequest,
    pub identity: AccessIdentity,
    pub created_at_ms: i64,
    pub state: String,
    pub surfaces: Vec<Value>,
    pub attempts: Vec<AnswerAttempt>,
    pub escalation_note: Option<String>,
    pub outcome: Option<String>,
    pub note: Option<String>,
    pub answered_by: Option<String>,
    /// Principal that withdrew the ask, stored for audit but omitted from wire projections.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub withdrawn_by: Option<String>,
    pub settled_at_ms: Option<i64>,
    pub unclaimed: bool,
}
impl AccessRecord {
    pub fn can_withdraw(&self, principal: &str) -> bool {
        principal == self.identity.principal
            || (principal == "reserved:prefrontal-core"
                && self.identity.scope_owner.as_deref() == Some(principal)
                && matches!(self.request, AccessRequest::ToolPermission { .. }))
    }
    pub fn pending(&self) -> bool {
        self.state.starts_with("pending:")
    }
    fn escalating_head(&self) -> Option<&ScopeIdentity> {
        // The accepted escalation attempt durably records the verified head.
        // A user-leg ask without that identity must not authorize a head answer.
        self.attempts.iter().find_map(|attempt| {
            (attempt.surface == "head"
                && attempt.principal == "reserved:prefrontal-core"
                && attempt.answer == "escalate"
                && attempt.result == "accepted")
                .then_some(attempt.answered_by.as_ref())
                .flatten()
        })
    }
    pub fn answers(&self) -> Vec<&'static str> {
        if self.request.informational() || !self.pending() {
            return vec![];
        }
        if self.state == "pending:head" {
            if self.request.head_policy() == Some("allow") {
                vec!["allow_once", "allow_scope", "deny", "escalate"]
            } else {
                vec!["deny", "escalate"]
            }
        } else {
            vec!["allow_once", "allow_scope", "deny"]
        }
    }
    pub fn wire(&self, phone: bool, block: Option<i64>) -> Value {
        let mut value = serde_json::to_value(&self.request).expect("access request serialization");
        let object = value.as_object_mut().expect("access request object");
        object.insert("elicitation_id".into(), json!(self.elicitation_id));
        object.insert("created_at_ms".into(), json!(self.created_at_ms));
        object.insert("state".into(), json!(self.state));
        if let Some(outcome) = &self.outcome {
            if outcome == "deny" {
                object.insert("code".into(), json!("access_declined"));
            } else if outcome.starts_with("access_") {
                object.insert("code".into(), json!(outcome));
            }
        }
        object.insert("requester_label".into(), json!(self.identity.principal));
        object.insert(
            "asker".into(),
            serde_json::to_value(&self.identity.asker).unwrap(),
        );
        if let Some(head) = &self.identity.head {
            object.insert("head".into(), serde_json::to_value(head).unwrap());
        }
        object.insert("options".into(), json!(self.answers().iter().map(|id| json!({"id":id,"label":id,"effect":match *id { "allow_once" | "allow_scope" => "grant", "escalate" => "escalate", _ => "decline" }})).collect::<Vec<_>>()));
        if !self.request.informational() {
            object.insert("default".into(), json!("deny"));
        }
        for (key, field) in [
            ("outcome", &self.outcome),
            ("note", &self.note),
            ("escalation_note", &self.escalation_note),
            ("answered_by", &self.answered_by),
        ] {
            if let Some(field) = field {
                object.insert(key.into(), json!(field));
            }
        }
        if let Some(at) = self.settled_at_ms {
            object.insert("answered_at_ms".into(), json!(at));
        }
        if self.pending() {
            if let Some(block) = block {
                object.insert("block_until_ms".into(), json!(block));
            }
        }
        if phone {
            let head = self.identity.head.as_ref().unwrap_or(&self.identity.asker);
            let agent_id = head.agent_id.as_deref().unwrap_or_default();
            let mut subject = json!({
                "agentId": agent_id,
                "headAgentId": agent_id,
                "label": agent_id,
            });
            if self.identity.scope_kind == "worker" {
                subject["viaWorker"] = json!(true);
            }
            object.insert("subject".into(), subject);
            // Shipped phones list and answer only cards whose state is exactly
            // "pending"; every card a phone sees waits on its user, so the leg
            // suffix tells it nothing and would hide the card.
            if self.state == "pending:user" {
                object.insert("state".into(), json!("pending"));
            }
            let (title, prompt, mut facts) = self.phone_description();
            if let Some(note) = &self.escalation_note {
                facts.push(json!({"label": "Lead agent's note", "value": note}));
            }
            object.insert("title".into(), json!(title));
            object.insert("prompt".into(), json!(prompt));
            object.insert("facts".into(), json!(facts));
            object.insert("options".into(), json!(self.answers().iter().filter(|id| **id != "escalate").map(|id| json!({
                "id": id,
                "label": match *id { "allow_once" => "Allow once", "allow_scope" => "Allow for this task", _ => "Don't allow" },
                "effect": match *id { "allow_once" | "allow_scope" => "grant", _ => "decline" },
            })).collect::<Vec<_>>()));
            Value::Object(
                object
                    .iter()
                    .map(|(key, val)| {
                        let mut parts = key.split('_');
                        let mut camel = parts.next().unwrap_or_default().to_owned();
                        for part in parts {
                            let mut chars = part.chars();
                            if let Some(c) = chars.next() {
                                camel.extend(c.to_uppercase());
                                camel.extend(chars);
                            }
                        }
                        (camel, val.clone())
                    })
                    .collect(),
            )
        } else {
            value
        }
    }
    fn phone_description(&self) -> (&'static str, String, Vec<Value>) {
        match &self.request {
            AccessRequest::ToolPermission {
                operation,
                paths,
                command,
                reason,
                pattern,
                ..
            } => {
                let actor = if self.identity.scope_kind == "worker" {
                    "An AFT worker"
                } else {
                    "An AFT agent"
                };
                let prompt = if let Some(command) = command {
                    format!("{actor} wants to run: {command}")
                } else {
                    let target = match paths.as_slice() {
                        [] => "0 paths".to_owned(),
                        [path] => format!("1 path: {path}"),
                        _ => {
                            let mut common = std::path::Path::new(&paths[0]).parent();
                            while common.is_some_and(|prefix| {
                                paths
                                    .iter()
                                    .any(|path| !std::path::Path::new(path).starts_with(prefix))
                            }) {
                                common = common.and_then(std::path::Path::parent);
                            }
                            match common {
                                Some(prefix) => {
                                    format!("{} paths under {}", paths.len(), prefix.display())
                                }
                                None => format!("{} paths: {}", paths.len(), paths.join(", ")),
                            }
                        }
                    };
                    format!("{actor} wants to {operation} {target}")
                };
                let mut facts = vec![json!({"label": "Operation", "value": operation})];
                if !paths.is_empty() {
                    facts.push(json!({"label": "Paths", "value": paths.join("\n")}));
                }
                if let Some(command) = command {
                    facts.push(json!({"label": "Command", "value": command}));
                }
                facts.push(json!({"label": "Reason", "value": reason}));
                facts.push(json!({"label": "Pattern", "value": pattern}));
                ("Worker permission", prompt, facts)
            }
            AccessRequest::MacosPermission {
                folder,
                program,
                steps,
            } => (
                "macOS permission",
                format!("{program} needs macOS permission to access {folder}"),
                vec![
                    json!({"label": "Paths", "value": folder}),
                    json!({"label": "Reason", "value": steps}),
                ],
            ),
        }
    }
    fn notice(&self, purpose: &str) -> Value {
        let mut fields = json!({});
        if matches!(purpose, "head_ask" | "user_ask") {
            if let AccessRequest::ToolPermission {
                tool,
                operation,
                paths,
                command,
                reason,
                head_policy,
                ..
            } = &self.request
            {
                fields = json!({"tool":tool,"operation":operation,"paths":paths,"reason":reason,"answers":self.answers()});
                if let Some(command) = command {
                    fields["command"] = json!(command);
                }
                if let Some(policy) = head_policy {
                    fields["head_policy"] = json!(policy);
                }
            }
        }
        if purpose == "settled" {
            fields["outcome"] = json!(self.outcome);
            if let Some(by) = &self.answered_by {
                fields["answered_by"] = json!(by);
            }
            if let Some(note) = &self.note {
                fields["note"] = json!(note);
            }
        }
        if purpose == "escalated" {
            if let Some(note) = &self.escalation_note {
                fields["note"] = json!(note);
            }
        }
        let mut body = json!({"elicitation_id":self.elicitation_id,"purpose":purpose,"asker":self.identity.asker.target(),"fields":fields});
        if let Some(head) = &self.identity.head {
            body["head"] = head.target();
        }
        body
    }
}

#[derive(Debug, Clone)]
pub struct AccessAnswer {
    pub surface: String,
    pub principal: String,
    pub answer: String,
    pub note: Option<String>,
    pub answered_by: Option<ScopeIdentity>,
}
#[derive(Debug, Clone)]
pub struct AccessMutation {
    pub record: AccessRecord,
    pub changed: bool,
    pub code: Option<String>,
}
#[derive(Debug, Clone)]
pub struct Notice {
    pub elicitation_id: String,
    pub purpose: String,
    pub body: Value,
    /// The stored body exactly as queued. Sending these bytes keeps the key
    /// order that pinned wire formats specify; `body` is the same JSON parsed.
    pub body_json: String,
    pub attempts: i64,
}

fn load(conn: &Connection, id: &str) -> Result<Option<AccessRecord>> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT record_json FROM access_asks WHERE elicitation_id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    raw.map(|raw| serde_json::from_str(&raw).map_err(Into::into))
        .transpose()
}
fn save(conn: &Connection, record: &AccessRecord) -> Result<()> {
    conn.execute(
        "UPDATE access_asks SET record_json=?2,state=?3 WHERE elicitation_id=?1",
        params![
            record.elicitation_id,
            serde_json::to_string(record)?,
            record.state
        ],
    )?;
    Ok(())
}
fn enqueue(conn: &Connection, record: &AccessRecord, purpose: &str) -> Result<()> {
    if !record.request.informational() {
        conn.execute("INSERT OR IGNORE INTO access_notices(elicitation_id,purpose,body_json) VALUES (?1,?2,?3)", params![record.elicitation_id, purpose, serde_json::to_string(&record.notice(purpose))?])?;
    }
    Ok(())
}
fn settle(conn: &Connection, record: &mut AccessRecord, outcome: &str, now: i64) -> Result<()> {
    record.state = if outcome.starts_with("access_") {
        format!("refused:{outcome}")
    } else {
        "settled".into()
    };
    record.outcome = Some(outcome.into());
    record.settled_at_ms = Some(now);
    conn.execute("UPDATE access_notices SET status='superseded' WHERE elicitation_id=?1 AND status='pending' AND purpose IN ('head_ask','user_ask','escalated')", [&record.elicitation_id])?;
    save(conn, record)?;
    enqueue(
        conn,
        record,
        if outcome == "access_withdrawn" {
            "withdrawn"
        } else {
            "settled"
        },
    )
}

impl ConsentStore {
    pub fn access_create(
        &self,
        identity: AccessIdentity,
        request: AccessRequest,
        now: i64,
    ) -> Result<AccessMutation> {
        self.transaction(|tx| {
            let matched: Option<String> = match &request {
                AccessRequest::ToolPermission { call_key, .. } => tx.query_row("SELECT elicitation_id FROM access_asks WHERE principal=?1 AND call_key=?2", params![identity.principal, call_key], |r| r.get(0)).optional()?,
                AccessRequest::MacosPermission { folder, program, .. } => tx.query_row("SELECT elicitation_id FROM access_asks WHERE folder=?1 AND program=?2 AND state='pending:user'", params![folder, program], |r| r.get(0)).optional()?,
            };
            if let Some(id) = matched { return Ok(AccessMutation { record: load(tx, &id)?.expect("matched access row"), changed: false, code: None }); }
            let seq: i64 = tx.query_row("UPDATE cingulate_id_seq SET value=value+1 WHERE singleton=1 AND value<9223372036854775807 RETURNING value", [], |r| r.get(0))?;
            let mut record = AccessRecord { elicitation_id: format!("acc_{seq}"), state: if identity.scope_kind == "worker" && !request.informational() { "pending:head" } else { "pending:user" }.into(), request, identity, created_at_ms: now, surfaces: vec![], attempts: vec![], escalation_note: None, outcome: None, note: None, answered_by: None, withdrawn_by: None, settled_at_ms: None, unclaimed: false };
            record.surfaces.push(json!({"surface":if record.state == "pending:head" { "head" } else { "user" },"at_ms":now,"answers":record.answers()}));
            let (folder, program) = match &record.request { AccessRequest::MacosPermission { folder, program, .. } => (Some(folder), Some(program)), _ => (None, None) };
            tx.execute("INSERT INTO access_asks(elicitation_id,principal,owner,call_key,folder,program,state,record_json) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)", params![record.elicitation_id,record.identity.principal,serde_json::to_string(&record.identity.asker)?,record.request.call_key(),folder,program,record.state,serde_json::to_string(&record)?])?;
            enqueue(tx, &record, if record.state == "pending:head" { "head_ask" } else { "user_ask" })?;
            Ok(AccessMutation { record, changed: true, code: None })
        })
    }
    pub fn access_get(&self, id: &str) -> Result<Option<AccessRecord>> {
        self.read(|conn| load(conn, id))
    }
    pub fn access_pending(&self) -> Result<Vec<AccessRecord>> {
        self.read(|conn| {
            let mut stmt = conn.prepare("SELECT record_json FROM access_asks WHERE state LIKE 'pending:%' ORDER BY elicitation_id")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<std::result::Result<Vec<_>, _>>()?;
            rows.into_iter().map(|s| serde_json::from_str(&s).map_err(Into::into)).collect()
        })
    }
    pub fn access_answer(
        &self,
        id: &str,
        answer: AccessAnswer,
        now: i64,
    ) -> Result<Option<AccessMutation>> {
        self.transaction(|tx| {
            let Some(mut record) = load(tx, id)? else { return Ok(None); };
            let code = if !record.pending() { Some("conflict") }
                else if record.request.informational() { Some("elicitation_invalid_choice") }
                else if answer.surface == "head" {
                    if answer.principal != "reserved:prefrontal-core" { Some("access_wrong_leg") }
                    else if record.state == "pending:user" && answer.answer == "deny" && matches!(record.request, AccessRequest::ToolPermission { .. }) {
                        match record.escalating_head() {
                            Some(head) if answer.answered_by.as_ref() == Some(head) => None,
                            Some(_) => Some("access_wrong_head"),
                            None => Some("access_wrong_leg"),
                        }
                    }
                    else if record.state != "pending:head" { Some("access_wrong_leg") }
                    else if answer.answered_by.as_ref() != record.identity.head.as_ref() { Some("access_wrong_head") }
                    else if matches!(answer.answer.as_str(), "allow_once" | "allow_scope") && record.request.head_policy() != Some("allow") { Some("access_head_not_held") }
                    else if !record.answers().contains(&answer.answer.as_str()) { Some("elicitation_invalid_choice") } else { None }
                } else if answer.surface != "user" || answer.principal != "reserved:callosum" || record.state != "pending:user" { Some("access_wrong_leg") }
                else if !record.answers().contains(&answer.answer.as_str()) { Some("elicitation_invalid_choice") } else { None };
            record.attempts.push(AnswerAttempt { malformed_params: None, surface: answer.surface.clone(), principal: answer.principal, at_ms: now, answer: answer.answer.clone(), note: answer.note.clone(), answered_by: answer.answered_by, result: code.unwrap_or("accepted").into() });
            if code.is_none() {
                if answer.answer == "escalate" {
                    record.state = "pending:user".into();
                    record.escalation_note = answer.note;
                    tx.execute("UPDATE access_notices SET status='superseded' WHERE elicitation_id=?1 AND status='pending' AND purpose='head_ask'", [&record.elicitation_id])?;
                    record.surfaces.push(json!({"surface":"user","at_ms":now,"answers":record.answers()}));
                    save(tx, &record)?;
                    enqueue(tx, &record, "escalated")?;
                } else {
                    record.answered_by = Some(answer.surface);
                    record.note = answer.note;
                    settle(tx, &mut record, &answer.answer, now)?;
                }
            } else { save(tx, &record)?; }
            Ok(Some(AccessMutation { record, changed: code.is_none(), code: code.map(str::to_owned) }))
        })
    }
    pub fn access_withdraw(&self, principal: &str, id: &str, now: i64) -> Result<AccessMutation> {
        self.transaction(|tx| {
            let mut record = load(tx, id)?.ok_or_else(|| {
                crate::refuse("elicitation_not_owner", "ask has no matching principal")
            })?;
            if !record.can_withdraw(principal) {
                return Err(crate::refuse(
                    "elicitation_not_owner",
                    "ask has no matching principal",
                ));
            }
            let changed = record.pending();
            if changed {
                record.withdrawn_by = Some(principal.into());
                settle(tx, &mut record, "access_withdrawn", now)?;
            }
            Ok(AccessMutation {
                record,
                changed,
                code: None,
            })
        })
    }
    pub fn access_rejected_attempt(&self, id: &str, attempt: AnswerAttempt) -> Result<()> {
        self.transaction(|tx| {
            if let Some(mut record) = load(tx, id)? {
                record.attempts.push(attempt);
                save(tx, &record)?;
            }
            Ok(())
        })
    }
    pub fn access_head_gone(&self, id: &str, now: i64) -> Result<Option<AccessRecord>> {
        self.transaction(|tx| {
            let Some(mut record) = load(tx, id)? else {
                return Ok(None);
            };
            if record.state != "pending:head" {
                return Ok(None);
            }
            settle(tx, &mut record, "access_head_gone", now)?;
            Ok(Some(record))
        })
    }
}

impl ConsentStore {
    pub fn access_notice_pending(&self, notice: &Notice) -> Result<bool> {
        self.read(|conn| Ok(conn.query_row("SELECT status='pending' FROM access_notices WHERE elicitation_id=?1 AND purpose=?2", params![notice.elicitation_id,notice.purpose], |r| r.get(0)).optional()?.unwrap_or(false)))
    }
    pub fn access_notices(&self, now: i64) -> Result<Vec<Notice>> {
        self.read(|conn| {
            let mut stmt = conn.prepare("SELECT elicitation_id,purpose,body_json,attempts FROM access_notices WHERE status='pending' AND retry_at_ms<=?1 ORDER BY rowid")?;
            let rows = stmt.query_map([now], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?)))?.collect::<std::result::Result<Vec<_>, _>>()?;
            rows.into_iter().map(|(elicitation_id,purpose,raw,attempts)| Ok(Notice { elicitation_id,purpose,body:serde_json::from_str(&raw)?,body_json:raw,attempts })).collect()
        })
    }
    /// Persist delivery results independently of the ask. A definitive refusal
    /// saying the worker's supervising session ended settles its head-only
    /// question; an unreachable server is not evidence that session ended.
    /// A late answer on a generic consent card whose session is gone is kept
    /// as `unclaimed` on its outbox row; the card itself is already settled.
    pub fn access_notice_result(
        &self,
        notice: &Notice,
        result: &str,
        now: i64,
    ) -> Result<Option<AccessRecord>> {
        self.transaction(|tx| {
            let retry = result == "transport";
            let missing = matches!(result, "unknown_session" | "session_not_live");
            let status = if retry { "pending" } else if result == "ok" { "delivered" } else if missing && matches!(notice.purpose.as_str(), "settled" | crate::LATE_ANSWER_PURPOSE) { "unclaimed" } else { "refused" };
            let backoff = (250_i64.saturating_mul(1_i64 << notice.attempts.min(8))).min(60_000);
            tx.execute("UPDATE access_notices SET status=?3,attempts=attempts+1,retry_at_ms=?4,last_error=?5 WHERE elicitation_id=?1 AND purpose=?2 AND status='pending'", params![notice.elicitation_id,notice.purpose,status,now.saturating_add(backoff),if result == "ok" { None } else { Some(result) }])?;
            let Some(mut record) = load(tx, &notice.elicitation_id)? else { return Ok(None); };
            if missing && notice.purpose == "head_ask" && record.state == "pending:head" {
                settle(tx, &mut record, "access_head_gone", now)?;
                return Ok(Some(record));
            }
            if status == "unclaimed" { record.unclaimed = true; save(tx, &record)?; }
            Ok(None)
        })
    }
}

impl ConsentStore {
    pub fn access_by_call_key(
        &self,
        principal: &str,
        call_key: &str,
    ) -> Result<Option<AccessRecord>> {
        self.read(|conn| {
            let id: Option<String> = conn
                .query_row(
                    "SELECT elicitation_id FROM access_asks WHERE principal=?1 AND call_key=?2",
                    params![principal, call_key],
                    |r| r.get(0),
                )
                .optional()?;
            id.map(|id| load(conn, &id))
                .transpose()
                .map(Option::flatten)
        })
    }
}

#[cfg(test)]
mod phone_tests {
    use super::*;

    fn record(request: Value) -> AccessRecord {
        AccessRecord {
            elicitation_id: "acc_test".into(),
            request: AccessRequest::decode(request).unwrap(),
            identity: AccessIdentity {
                principal: "reserved:aft".into(),
                scope_owner: Some("reserved:prefrontal-core".into()),
                asker: ScopeIdentity {
                    scope_ref: "worker".into(),
                    scope_epoch: 1,
                    agent_id: None,
                },
                scope_kind: "worker".into(),
                harness: "test".into(),
                head: Some(ScopeIdentity {
                    scope_ref: "head".into(),
                    scope_epoch: 2,
                    agent_id: Some("agent".into()),
                }),
            },
            created_at_ms: 1,
            state: "pending:head".into(),
            surfaces: vec![],
            attempts: vec![],
            escalation_note: None,
            outcome: None,
            note: None,
            answered_by: None,
            withdrawn_by: None,
            settled_at_ms: None,
            unclaimed: false,
        }
    }

    fn tool(operation: &str, paths: Value, command: Option<&str>) -> AccessRecord {
        record(
            json!({"kind":"tool_permission", "call_key":"test", "tool":"test", "operation":operation, "paths":paths, "command":command, "reason":"Test permission", "args_hash":"hash", "pattern":"test", "head_policy":"allow"}),
        )
    }

    #[test]
    fn phone_write_prompt_uses_common_path_components_and_keeps_all_paths() {
        let record = tool(
            "write",
            json!(["/common/prefix/a", "/common/prefix/b", "/common/prefix/c"]),
            None,
        );
        let phone = record.wire(true, None);
        assert_eq!(
            phone["prompt"],
            "An AFT worker wants to write 3 paths under /common/prefix"
        );
        assert_eq!(
            phone["facts"][1]["value"],
            "/common/prefix/a\n/common/prefix/b\n/common/prefix/c"
        );
        let disjoint = tool(
            "read",
            json!(["/common/abc/file", "/common/abd/file"]),
            None,
        );
        assert_eq!(
            disjoint.wire(true, None)["prompt"],
            "An AFT worker wants to read 2 paths under /common"
        );
    }

    #[test]
    fn phone_exec_prompt_and_fact_preserve_the_entire_command() {
        let command = format!("printf {}", "x".repeat(580));
        let record = tool("exec", json!([]), Some(&command));
        let phone = record.wire(true, None);
        assert_eq!(
            phone["prompt"],
            format!("An AFT worker wants to run: {command}")
        );
        assert_eq!(
            phone["facts"][1],
            json!({"label":"Command", "value":command})
        );
        assert!(phone["facts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|fact| fact["value"].is_string()));
        assert!(!phone["facts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|fact| fact["label"] == "Paths"));
    }

    #[test]
    fn phone_projection_does_not_change_core_choices_or_fields() {
        let record = tool("read", json!(["/x/y"]), None);
        let core = record.wire(false, Some(123));
        assert_eq!(
            core["options"],
            json!([
                {"id":"allow_once","label":"allow_once","effect":"grant"},
                {"id":"allow_scope","label":"allow_scope","effect":"grant"},
                {"id":"deny","label":"deny","effect":"decline"},
                {"id":"escalate","label":"escalate","effect":"escalate"}
            ])
        );
        let phone = record.wire(true, Some(123));
        assert_eq!(phone["options"].as_array().unwrap().len(), 3);
        assert_eq!(phone["blockUntilMs"], 123);
        for key in ["prompt", "title", "subject", "facts"] {
            assert!(core.get(key).is_none());
        }
        assert_eq!(core, record.wire(false, Some(123)));
    }

    #[test]
    fn macos_phone_card_describes_its_request_without_grant_choices() {
        let record = record(
            json!({"kind":"macos_permission", "folder":"/protected", "program":"Terminal", "steps":"Open System Settings"}),
        );
        let phone = record.wire(true, None);
        assert_eq!(
            phone["prompt"],
            "Terminal needs macOS permission to access /protected"
        );
        assert_eq!(phone["title"], "macOS permission");
        assert_eq!(phone["subject"]["headAgentId"], "agent");
        assert_eq!(phone["options"], json!([]));
        assert!(phone.get("default").is_none());
        assert_eq!(
            phone["facts"],
            json!([
                {"label":"Paths","value":"/protected"},
                {"label":"Reason","value":"Open System Settings"}
            ])
        );
    }
}
