//! Closed vault statements and decision bindings. Signature verification belongs
//! to the vault; Cingulate records the hash of the exact envelope the phone used.

use std::collections::HashSet;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{lowercase_hex, Refusal};

const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;
const DURATIONS: [&str; 4] = ["once", "hour", "session", "permanent"];

fn invalid() -> Refusal {
    Refusal::invalid("invalid secret approval statement or envelope")
}

pub(super) fn ulid(value: &str) -> bool {
    value.len() == 26
        && value.as_bytes()[0] <= b'7'
        && value
            .bytes()
            .all(|b| b"0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(&b))
}

fn safe_numbers(value: &Value) -> bool {
    match value {
        Value::Number(n) => n.as_u64().is_some_and(|n| n <= MAX_SAFE_INTEGER),
        Value::Array(a) => a.iter().all(safe_numbers),
        Value::Object(o) => o.values().all(safe_numbers),
        _ => true,
    }
}

pub(super) fn canonical<T: DeserializeOwned>(encoded: &str) -> Result<(T, Vec<u8>), Refusal> {
    let bytes = URL_SAFE_NO_PAD.decode(encoded).map_err(|_| invalid())?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if !safe_numbers(&value) || serde_jcs::to_vec(&value).map_err(|_| invalid())? != bytes {
        return Err(invalid());
    }
    let parsed = serde_json::from_value(value).map_err(|_| invalid())?;
    Ok((parsed, bytes))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SecretUseItem {
    pub request_id: String,
    pub statement_b64url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SecretUse {
    pub items: Vec<SecretUseItem>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UseStatement {
    pub v: String,
    pub request_id: String,
    pub member_id: String,
    pub agent_id: Option<String>,
    pub scope: StatementScope,
    pub coverage: String,
    pub secret: StatementSecret,
    pub r#use: StatementUse,
    pub durations: Vec<String>,
    pub request_expires_at_ms: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatementScope {
    pub owner: String,
    pub kind: String,
    pub r#ref: String,
    pub epoch: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatementSecret {
    pub name: String,
    pub kind: String,
    pub incarnation: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum StatementUse {
    ApiKey {
        host: String,
        path_prefix: String,
        location: String,
    },
    Login {
        origin: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EnvelopeItem {
    request_id: String,
    statement_sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    v: String,
    items: Vec<EnvelopeItem>,
    decision: String,
    duration: String,
    grant_expires_at_ms: u64,
    device_fingerprint: String,
    issued_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SecretUseAnswer {
    pub decision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<String>,
    pub items: Vec<SecretUseItemDecision>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub envelope_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SecretUseItemDecision {
    pub request_id: String,
    pub decision: String,
}

impl SecretUse {
    pub fn statements(&self) -> Result<Vec<UseStatement>, Refusal> {
        if !(1..=16).contains(&self.items.len()) {
            return Err(invalid());
        }
        let mut seen = HashSet::new();
        let mut statements: Vec<UseStatement> = Vec::new();
        for item in &self.items {
            let (statement, _): (UseStatement, _) = canonical(&item.statement_b64url)?;
            // Option fields accept omission in serde, but agent_id is a required
            // nullable key in the vault statement.
            let (value, _): (Value, _) = canonical(&item.statement_b64url)?;
            if value.get("agent_id").is_none()
                || statement.v != "ck-secret-use-statement/v1"
                || !ulid(&item.request_id)
                || statement.request_id != item.request_id
                || !seen.insert(&item.request_id)
                || statement.durations.is_empty()
                || statement
                    .durations
                    .iter()
                    .any(|d| !DURATIONS.contains(&d.as_str()))
                || statements.first().is_some_and(|first| {
                    first.agent_id != statement.agent_id || first.member_id != statement.member_id
                })
            {
                return Err(invalid());
            }
            statements.push(statement);
        }
        Ok(statements)
    }

    pub fn expires_at_ms(&self) -> Result<i64, Refusal> {
        Ok(self
            .statements()?
            .iter()
            .map(|s| s.request_expires_at_ms)
            .min()
            .ok_or_else(invalid)? as i64)
    }

    pub fn answer(
        &self,
        choice: &str,
        device: Option<&str>,
        encoded: Option<&str>,
    ) -> Result<SecretUseAnswer, Refusal> {
        let statements = self.statements()?;
        self.answer_items(
            choice,
            device,
            encoded,
            &statements
                .into_iter()
                .map(|s| s.durations)
                .collect::<Vec<_>>(),
            false,
        )
    }

    pub(super) fn answer_items(
        &self,
        choice: &str,
        device: Option<&str>,
        encoded: Option<&str>,
        durations: &[Vec<String>],
        flow: bool,
    ) -> Result<SecretUseAnswer, Refusal> {
        let (duration, allowed, hash) = match (choice, encoded) {
            ("decline", None) => (None, HashSet::new(), None),
            ("allow", Some(encoded)) => {
                let (envelope, bytes): (Envelope, _) = canonical(encoded)?;
                if envelope.v != "ck-secret-use-envelope/v1"
                    || envelope.decision != "allow"
                    || device != Some(envelope.device_fingerprint.as_str())
                    || !lowercase_hex(&envelope.device_fingerprint, 32)
                    || !(1..=16).contains(&envelope.items.len())
                    || (flow
                        && (envelope.items.len() != self.items.len()
                            || envelope.duration != "version"
                            || envelope.grant_expires_at_ms != 0))
                {
                    return Err(invalid());
                }
                let mut allowed = HashSet::new();
                for item in &envelope.items {
                    let index = self
                        .items
                        .iter()
                        .position(|s| s.request_id == item.request_id)
                        .ok_or_else(invalid)?;
                    let bytes = URL_SAFE_NO_PAD
                        .decode(&self.items[index].statement_b64url)
                        .map_err(|_| invalid())?;
                    if !allowed.insert(item.request_id.clone())
                        || item.statement_sha256 != format!("{:x}", Sha256::digest(bytes))
                        || !durations[index].contains(&envelope.duration)
                    {
                        return Err(invalid());
                    }
                }
                let _ = (envelope.grant_expires_at_ms, envelope.issued_at_ms);
                (
                    Some(envelope.duration),
                    allowed,
                    Some(format!("{:x}", Sha256::digest(bytes))),
                )
            }
            _ => return Err(invalid()),
        };
        Ok(SecretUseAnswer {
            decision: choice.into(),
            duration,
            envelope_sha256: hash,
            items: self
                .items
                .iter()
                .map(|item| SecretUseItemDecision {
                    request_id: item.request_id.clone(),
                    decision: if allowed.contains(&item.request_id) {
                        "allow"
                    } else {
                        "decline"
                    }
                    .into(),
                })
                .collect(),
        })
    }
}

pub(super) fn normalize(body: Value) -> Result<Value, Refusal> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Filing {
        kind: String,
        secret_use: SecretUse,
    }
    let filing: Filing = serde_json::from_value(body).map_err(|_| invalid())?;
    Ok(json!({
        "kind": filing.kind, "secret_use": filing.secret_use,
        "title": "Secret use approval", "prompt": "Review the vault's requested access.",
        "options": [{"id":"allow", "label":"Allow", "effect":"grant"}, {"id":"decline", "label":"Decline", "effect":"decline"}],
        "default":"decline", "on_expiry":"deny", "urgency":"normal", "material_damage":false,
        "late_execution":"notify_only", "scope":{}, "target":{"kind":"secret_use", "label":"Vault access"}
    }))
}
