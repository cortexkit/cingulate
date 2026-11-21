//! Closed broker-action statements and answer bindings. The vault, not
//! Cingulate, verifies signatures and controls issuance for the approved request.

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::{
    lowercase_hex,
    secret_use::{canonical, check_canonical, decode_json, ulid, MAX_SAFE_INTEGER},
    Refusal,
};

const MAX_STATEMENT_BYTES: usize = 512 * 1024;
const MAX_STATEMENT_ENCODING: usize = (MAX_STATEMENT_BYTES * 4).div_ceil(3);
const MAX_ENTRIES: usize = 16;
static CONTEXT_CONTROL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\p{Cc}\p{Cf}\p{Zl}\p{Zp}]").expect("Unicode category regex"));

fn invalid(field: &str) -> Refusal {
    Refusal::invalid(format!("invalid broker_action.{field}")).reason("statement_invalid")
}

fn invalid_envelope(field: &str) -> Refusal {
    invalid(field).reason("envelope_invalid")
}

/// Display facts derived from the exact canonical bytes. The original bytes
/// stay available so clients can decode and render the full statement.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BrokerAction {
    pub statement_b64url: String,
    pub statement_sha256: String,
    pub request_id: String,
    pub agent_id: String,
    pub service: String,
    pub identity: String,
    pub action: String,
    pub resource: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BrokerActionAnswer {
    pub decision: String,
    pub statement_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub envelope_sha256: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    v: String,
    request_id: String,
    statement_sha256: String,
    decision: String,
    device_fingerprint: String,
    #[serde(rename = "issued_at_ms")]
    _issued_at_ms: u64,
}

fn member(parent: &str, key: &str) -> String {
    if parent.is_empty() {
        key.to_owned()
    } else {
        format!("{parent}.{key}")
    }
}

fn closed<'a>(
    value: &'a Value,
    parent: &str,
    required: &[&str],
    optional: &[&str],
) -> Result<&'a Map<String, Value>, Refusal> {
    let object = value.as_object().ok_or_else(|| {
        invalid(if parent.is_empty() {
            "statement_b64url"
        } else {
            parent
        })
    })?;
    for key in required {
        if !object.contains_key(*key) {
            return Err(invalid(&member(parent, key)));
        }
    }
    for key in object.keys() {
        if !required.contains(&key.as_str()) && !optional.contains(&key.as_str()) {
            // Do not echo arbitrary statement text in a refusal.
            let key =
                if key.len() <= 64 && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                    member(parent, key)
                } else if parent.is_empty() {
                    "statement_b64url".into()
                } else {
                    parent.into()
                };
            return Err(invalid(&key));
        }
    }
    Ok(object)
}

fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str, Refusal> {
    value
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| invalid(field))
}

fn unsigned(value: &Value, field: &str) -> Result<u64, Refusal> {
    value
        .as_u64()
        .filter(|&n| n <= MAX_SAFE_INTEGER)
        .ok_or_else(|| invalid(field))
}

fn field_numbers(value: &Value) -> bool {
    match value {
        Value::Number(n) => n
            .as_i64()
            .is_some_and(|n| n.unsigned_abs() <= MAX_SAFE_INTEGER),
        Value::Array(a) => a.iter().all(field_numbers),
        Value::Object(o) => o.values().all(field_numbers),
        _ => true,
    }
}

fn resource<'a>(value: &'a Value, field: &str) -> Result<&'a str, Refusal> {
    let valid_part = |part: &str| {
        !part.is_empty()
            && !matches!(part, "." | "..")
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    };
    text(value, field).and_then(|s| {
        if s.split_once('/')
            .is_some_and(|(owner, repo)| valid_part(owner) && valid_part(repo))
        {
            Ok(s)
        } else {
            Err(invalid(field))
        }
    })
}

fn method(value: &Value, field: &str) -> Result<(), Refusal> {
    if !text(value, field)?
        .bytes()
        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b"!#$%&'*+-.^_`|~".contains(&b))
    {
        return Err(invalid(field));
    }
    Ok(())
}

fn digest(value: &Value, field: &str) -> Result<(), Refusal> {
    if !lowercase_hex(text(value, field)?, 64) {
        return Err(invalid(field));
    }
    Ok(())
}

fn host_ok(host: &str) -> bool {
    let hostname = if let Some((hostname, port)) = host.split_once(':') {
        if port.is_empty()
            || !port.bytes().all(|b| b.is_ascii_digit())
            || !port.parse::<u16>().is_ok_and(|n| n > 0)
        {
            return false;
        }
        hostname
    } else {
        host
    };
    hostname.trim_end_matches('.').split('.').all(|label| {
        !label.is_empty()
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    })
}

fn statement(encoded: String) -> Result<(BrokerAction, i64), Refusal> {
    if encoded.len() > MAX_STATEMENT_ENCODING {
        return Err(invalid("statement_b64url"));
    }
    let (value, bytes) = decode_json(&encoded).map_err(|_| invalid("statement_b64url"))?;
    if bytes.len() > MAX_STATEMENT_BYTES {
        return Err(invalid("statement_b64url"));
    }
    let body = closed(
        &value,
        "",
        &[
            "v",
            "request_id",
            "agent_id",
            "scope",
            "service",
            "identity",
            "action",
            "host",
            "resource",
            "policy_version",
            "policy_sha256",
            "fields",
            "expires_at_ms",
        ],
        &["context", "legs", "kind", "method", "request_sha256"],
    )?;
    if body["v"].as_str() != Some("ck-broker-action-statement/v1") {
        return Err(invalid("v"));
    }
    let request_id = text(&body["request_id"], "request_id")?;
    if !ulid(request_id) {
        return Err(invalid("request_id"));
    }
    let agent_id = text(&body["agent_id"], "agent_id")?;
    if agent_id.len() > 128
        || !agent_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err(invalid("agent_id"));
    }
    let scope = closed(
        &body["scope"],
        "scope",
        &["owner", "kind", "ref", "epoch"],
        &[],
    )?;
    for key in ["owner", "kind", "ref"] {
        text(&scope[key], &member("scope", key))?;
    }
    unsigned(&scope["epoch"], "scope.epoch")?;
    let service = text(&body["service"], "service")?;
    if service != "github" {
        return Err(invalid("service"));
    }
    let identity = text(&body["identity"], "identity")?;
    if !identity.strip_prefix("github_app:").is_some_and(|slug| {
        !slug.is_empty()
            && slug
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    }) {
        return Err(invalid("identity"));
    }
    let action = text(&body["action"], "action")?;
    if !host_ok(text(&body["host"], "host")?) {
        return Err(invalid("host"));
    }
    let binding_resource = resource(&body["resource"], "resource")?;
    unsigned(&body["policy_version"], "policy_version")?;
    digest(&body["policy_sha256"], "policy_sha256")?;
    let fields = body["fields"]
        .as_array()
        .filter(|a| a.len() <= MAX_ENTRIES)
        .ok_or_else(|| invalid("fields"))?;
    for field in fields {
        let field = closed(field, "fields", &["name", "value"], &[])?;
        text(&field["name"], "fields.name")?;
        if !field_numbers(&field["value"]) {
            return Err(invalid("fields.value"));
        }
    }
    if let Some(legs) = body.get("legs") {
        for key in ["kind", "method", "request_sha256"] {
            if body.contains_key(key) {
                return Err(invalid(key));
            }
        }
        let legs = legs
            .as_array()
            .filter(|a| (1..=MAX_ENTRIES).contains(&a.len()))
            .ok_or_else(|| invalid("legs"))?;
        for leg in legs {
            let leg = closed(
                leg,
                "legs",
                &["kind", "method", "resource", "request_sha256"],
                &[],
            )?;
            text(&leg["kind"], "legs.kind")?;
            method(&leg["method"], "legs.method")?;
            resource(&leg["resource"], "legs.resource")?;
            digest(&leg["request_sha256"], "legs.request_sha256")?;
        }
    } else {
        if body.get("kind").and_then(Value::as_str) != Some(action) {
            return Err(invalid("kind"));
        }
        method(
            body.get("method").ok_or_else(|| invalid("method"))?,
            "method",
        )?;
        digest(
            body.get("request_sha256")
                .ok_or_else(|| invalid("request_sha256"))?,
            "request_sha256",
        )?;
    }
    if let Some(context) = body.get("context") {
        if !context
            .as_str()
            .is_some_and(|s| s.chars().count() <= 200 && !CONTEXT_CONTROL.is_match(s))
        {
            return Err(invalid("context"));
        }
    }
    let expiry = unsigned(&body["expires_at_ms"], "expires_at_ms")? as i64;
    check_canonical(&value, &bytes).map_err(|_| invalid("statement_b64url"))?;
    Ok((
        BrokerAction {
            statement_b64url: encoded,
            statement_sha256: format!("{:x}", Sha256::digest(bytes)),
            request_id: request_id.into(),
            agent_id: agent_id.into(),
            service: service.into(),
            identity: identity.into(),
            action: action.into(),
            resource: binding_resource.into(),
        },
        expiry,
    ))
}

impl BrokerAction {
    pub fn from_statement(statement_b64url: String) -> Result<Self, Refusal> {
        Ok(statement(statement_b64url)?.0)
    }

    pub fn expires_at_ms(&self) -> Result<i64, Refusal> {
        let (derived, expiry) = statement(self.statement_b64url.clone())?;
        if derived != *self {
            return Err(invalid("statement_sha256"));
        }
        Ok(expiry)
    }

    pub fn answer(
        &self,
        choice: &str,
        device: Option<&str>,
        encoded: Option<&str>,
    ) -> Result<BrokerActionAnswer, Refusal> {
        self.expires_at_ms()?;
        let envelope_sha256 = match (choice, encoded) {
            ("decline", None) => None,
            ("allow", Some(encoded)) => {
                let (envelope, bytes): (Envelope, _) =
                    canonical(encoded).map_err(|_| invalid_envelope("envelope_b64url"))?;
                if envelope.v != "ck-broker-action-envelope/v1" {
                    return Err(invalid_envelope("v"));
                }
                if envelope.request_id != self.request_id {
                    return Err(invalid_envelope("request_id"));
                }
                if envelope.statement_sha256 != self.statement_sha256 {
                    return Err(invalid_envelope("statement_sha256"));
                }
                if envelope.decision != choice {
                    return Err(invalid_envelope("decision"));
                }
                if device != Some(envelope.device_fingerprint.as_str())
                    || !lowercase_hex(&envelope.device_fingerprint, 32)
                {
                    return Err(invalid_envelope("device_fingerprint"));
                }
                Some(format!("{:x}", Sha256::digest(bytes)))
            }
            _ => return Err(invalid_envelope("decision")),
        };
        Ok(BrokerActionAnswer {
            decision: choice.into(),
            statement_sha256: self.statement_sha256.clone(),
            envelope_sha256,
        })
    }
}

pub(super) fn normalize(body: &Value) -> Result<Value, Refusal> {
    let filing = closed(body, "", &["kind", "broker_action"], &[])?;
    let body = closed(&filing["broker_action"], "", &["statement_b64url"], &[])?;
    let encoded = body["statement_b64url"]
        .as_str()
        .ok_or_else(|| invalid("statement_b64url"))?;
    let facts = BrokerAction::from_statement(encoded.into())?;
    Ok(json!({
        "kind":"broker_action", "broker_action":facts,
        "title":"Allow this action once?", "prompt":"Review the action to be sent.",
        "options":[{"id":"allow","label":"Allow once","effect":"grant"},{"id":"decline","label":"Decline","effect":"decline"}],
        "default":"decline", "on_expiry":"deny", "urgency":"normal", "material_damage":false,
        "late_execution":"notify_only", "scope":{}, "target":{"kind":"broker_action","label":"Broker action"}
    }))
}
