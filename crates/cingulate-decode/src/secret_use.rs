//! Closed vault statements and decision bindings. Signature verification belongs
//! to the vault; Cingulate records the hash of the exact envelope the phone used.

use std::collections::HashSet;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{lowercase_hex, Refusal};

pub(super) const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;
const DURATIONS: [&str; 4] = ["once", "hour", "session", "permanent"];

fn invalid() -> Refusal {
    Refusal::invalid("invalid secret approval statement or envelope").reason("statement_invalid")
}

fn invalid_envelope() -> Refusal {
    invalid().reason("envelope_invalid")
}

pub(super) fn ulid(value: &str) -> bool {
    value.len() == 26
        && value.as_bytes()[0] <= b'7'
        && value
            .bytes()
            .all(|b| b"0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(&b))
}

/// Accepts a key that is either absent (`None`) or a JSON string. A present
/// `null` is refused, so "the key is there" and "the key is missing" can never
/// be confused: a v1 statement must not carry `asking_member_id` at all.
pub(super) fn present_string<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    String::deserialize(deserializer).map(Some)
}

/// Checks a statement's `v` against its family and the presence of
/// `asking_member_id`.
///
/// The vault writes v1 for a request made on the home computer that holds the
/// vault with a legacy scope, and v2 for a forwarded legacy request. Extended
/// scopes use v3 for both local and forwarded requests. Only forwarded requests
/// carry `asking_member_id`, naming the computer that asked as `fed-computer:`
/// and 32 lowercase hex characters. `member_id` still names the serving vault.
/// `family` keeps secret_use and flow statements apart.
///
/// Cingulate deliberately does not compare `asking_member_id` with this
/// computer's own identity. The home vault sets the field from the paired
/// computer the forwarded request arrived from, as the local subc daemon
/// stamps it on that route (never from the request's own parameters), and
/// grants only a statement whose digest
/// matches the request it stored, so the vault already binds it; a check here
/// would need a call to another module on every card and add a failure mode.
pub(super) fn statement_version_ok(
    v: &str,
    family: &str,
    asking_member_id: Option<&str>,
    member_id: &str,
) -> bool {
    let Some(version) = v.strip_prefix(family).and_then(|v| v.strip_prefix('/')) else {
        return false;
    };
    match (version, asking_member_id) {
        ("v1" | "v3", None) => true,
        ("v2" | "v3", Some(asking)) => asking_member_ok(asking, member_id),
        _ => false,
    }
}

fn asking_member_ok(asking: &str, member_id: &str) -> bool {
    asking
        .strip_prefix("fed-computer:")
        .is_some_and(|hex| lowercase_hex(hex, 32))
        // The home vault never forwards to itself.
        && asking != member_id
}

fn safe_numbers(value: &Value) -> bool {
    match value {
        Value::Number(n) => n.as_u64().is_some_and(|n| n <= MAX_SAFE_INTEGER),
        Value::Array(a) => a.iter().all(safe_numbers),
        Value::Object(o) => o.values().all(safe_numbers),
        _ => true,
    }
}

pub(super) fn decode_json(encoded: &str) -> Result<(Value, Vec<u8>), Refusal> {
    let bytes = URL_SAFE_NO_PAD.decode(encoded).map_err(|_| invalid())?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    Ok((value, bytes))
}

pub(super) fn check_canonical(value: &Value, bytes: &[u8]) -> Result<(), Refusal> {
    if serde_jcs::to_vec(value).map_err(|_| invalid())? != bytes {
        return Err(invalid());
    }
    Ok(())
}

pub(super) fn canonical<T: DeserializeOwned>(encoded: &str) -> Result<(T, Vec<u8>), Refusal> {
    let (value, bytes) = decode_json(encoded)?;
    if !safe_numbers(&value) {
        return Err(invalid());
    }
    check_canonical(&value, &bytes)?;
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
    /// The paired computer the agent asked from; present exactly for forwarded
    /// requests (v2, forwarded v3, or forwarded local-use v1).
    #[serde(default, deserialize_with = "present_string")]
    pub asking_member_id: Option<String>,
    pub agent_id: Option<String>,
    pub scope: StatementScope,
    pub coverage: String,
    pub secret: Option<StatementSecret>,
    pub r#use: StatementUse,
    pub durations: Vec<String>,
    pub request_expires_at_ms: u64,
    pub program: Option<LocalProgram>,
    pub key: Option<StatementKey>,
    pub target: Option<LocalTarget>,
    pub command: Option<String>,
    pub workdir: Option<String>,
    pub purpose: Option<String>,
    pub expects: Option<String>,
    pub undoable: Option<Undoable>,
}

#[derive(Debug, Deserialize)]
#[serde(try_from = "String")]
pub enum LocalProgram {
    Sudo,
    Ssh,
}

impl TryFrom<String> for LocalProgram {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "sudo" => Ok(Self::Sudo),
            "ssh" => Ok(Self::Ssh),
            _ => Err("invalid local program"),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(try_from = "String")]
pub enum LocalUse {
    SudoPassword,
    SshSign,
}

impl TryFrom<String> for LocalUse {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "sudo_password" => Ok(Self::SudoPassword),
            "ssh_sign" => Ok(Self::SshSign),
            _ => Err("invalid local use"),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(try_from = "String")]
pub enum Undoable {
    Yes,
    No,
    Unknown,
}

impl TryFrom<String> for Undoable {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "yes" => Ok(Self::Yes),
            "no" => Ok(Self::No),
            "unknown" => Ok(Self::Unknown),
            _ => Err("invalid undoable value"),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatementKey {
    pub credential_id: String,
    pub incarnation: String,
    pub fingerprint: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum LocalTarget {
    Sudo {
        user: String,
    },
    Ssh {
        user: String,
        host: String,
        port: u16,
    },
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
    Local(LocalUse),
    ApiKey {
        host: String,
        // Signed legacy statements always emit this key, using "" for no prefix.
        path_prefix: String,
        location: String,
    },
    Login {
        origin: String,
    },
    ExtendedApiKey {
        rules: Vec<StatementRule>,
        location: String,
    },
    ExtendedLogin {
        origins: Vec<String>,
    },
}

impl StatementUse {
    fn shape_ok(&self, v3: bool, kind: &str) -> bool {
        match self {
            Self::Local(_) => false,
            Self::ApiKey { .. } | Self::Login { .. } => !v3,
            Self::ExtendedApiKey { .. } => v3 && kind == "agent_api_key",
            Self::ExtendedLogin { .. } => v3 && kind == "site_login",
        }
    }
}

impl UseStatement {
    pub fn is_local(&self) -> bool {
        self.v == "ck-secret-local-use-statement/v1"
    }

    fn shape_ok(&self, value: &Value) -> bool {
        const LOCAL_FIELDS: [&str; 8] = [
            "program", "key", "target", "command", "workdir", "purpose", "expects", "undoable",
        ];
        if !self.is_local() {
            return statement_version_ok(
                &self.v,
                "ck-secret-use-statement",
                self.asking_member_id.as_deref(),
                &self.member_id,
            ) && self.secret.as_ref().is_some_and(|secret| {
                self.r#use
                    .shape_ok(self.v == "ck-secret-use-statement/v3", &secret.kind)
            }) && LOCAL_FIELDS.iter().all(|field| value.get(field).is_none());
        }

        if self
            .asking_member_id
            .as_deref()
            .is_some_and(|asking| !asking_member_ok(asking, &self.member_id))
            || self.command.is_none()
            || self.workdir.is_none()
        {
            return false;
        }
        // The vault owns text limits and target grammar. Only the signed
        // member shapes and the closed program/use pairs are checked here.
        match (&self.program, &self.r#use, &self.target) {
            (
                Some(LocalProgram::Sudo),
                StatementUse::Local(LocalUse::SudoPassword),
                Some(LocalTarget::Sudo { .. }),
            ) => {
                self.secret
                    .as_ref()
                    .is_some_and(|secret| secret.kind == "local_program")
                    && value.get("key").is_none()
                    && self
                        .purpose
                        .as_ref()
                        .is_some_and(|purpose| !purpose.is_empty())
                    && self.expects.is_some()
                    && self.undoable.is_some()
                    && self.durations == ["once"]
            }
            (
                Some(LocalProgram::Ssh),
                StatementUse::Local(LocalUse::SshSign),
                Some(LocalTarget::Ssh { .. }),
            ) => {
                self.key.is_some()
                    && ["secret", "expects", "undoable", "purpose"]
                        .iter()
                        .all(|field| value.get(field).is_none())
                    && self
                        .durations
                        .iter()
                        .all(|duration| ["once", "hour", "session"].contains(&duration.as_str()))
                    && self.durations.iter().collect::<HashSet<_>>().len() == self.durations.len()
            }
            _ => false,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatementRule {
    pub host: String,
    pub path_prefix: String,
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
            let (value, _): (Value, _) = canonical(&item.statement_b64url)?;
            // Every vault statement carries agent_id, using null for no agent.
            if value.get("agent_id").is_none()
                || !statement.shape_ok(&value)
                || !ulid(&item.request_id)
                || statement.request_id != item.request_id
                || !seen.insert(&item.request_id)
                || statement.durations.is_empty()
                || statement
                    .durations
                    .iter()
                    .any(|d| !DURATIONS.contains(&d.as_str()))
                || statements.first().is_some_and(|first| {
                    // One card is one request from one agent on one computer:
                    // every statement shares the agent, the serving vault and
                    // the asking computer (or its absence for home requests).
                    first.agent_id != statement.agent_id
                        || first.member_id != statement.member_id
                        || first.asking_member_id != statement.asking_member_id
                        || first.is_local() != statement.is_local()
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
                let (envelope, bytes): (Envelope, _) =
                    canonical(encoded).map_err(|error| error.reason("envelope_invalid"))?;
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
                    return Err(invalid_envelope());
                }
                let mut allowed = HashSet::new();
                for item in &envelope.items {
                    let index = self
                        .items
                        .iter()
                        .position(|s| s.request_id == item.request_id)
                        .ok_or_else(invalid_envelope)?;
                    let bytes = URL_SAFE_NO_PAD
                        .decode(&self.items[index].statement_b64url)
                        .map_err(|_| invalid())?;
                    if !allowed.insert(item.request_id.clone())
                        || item.statement_sha256 != format!("{:x}", Sha256::digest(bytes))
                        || !durations[index].contains(&envelope.duration)
                    {
                        return Err(invalid_envelope());
                    }
                }
                let _ = (envelope.grant_expires_at_ms, envelope.issued_at_ms);
                (
                    Some(envelope.duration),
                    allowed,
                    Some(format!("{:x}", Sha256::digest(bytes))),
                )
            }
            _ => return Err(invalid_envelope()),
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
    let filing: Filing = serde_json::from_value(body).map_err(|_| invalid_envelope())?;
    Ok(json!({
        "kind": filing.kind, "secret_use": filing.secret_use,
        "title": "Secret use approval", "prompt": "Review the vault's requested access.",
        "options": [{"id":"allow", "label":"Allow", "effect":"grant"}, {"id":"decline", "label":"Decline", "effect":"decline"}],
        "default":"decline", "on_expiry":"deny", "urgency":"normal", "material_damage":false,
        "late_execution":"notify_only", "scope":{}, "target":{"kind":"secret_use", "label":"Vault access"}
    }))
}
