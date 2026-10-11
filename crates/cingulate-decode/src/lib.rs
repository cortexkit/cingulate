//! Consent request decoding, independent of requester identity and storage.
//!
//! The caller must authorize the daemon's route stamp before decoding a request.
//! Body fields never supply the requester or subject identity.

use std::{collections::HashSet, fmt};

use serde::{de, Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

mod access;
pub use access::{validate_note, AccessRequest};

pub const MAX_EXPIRES_IN_MS: i64 = 86_400_000;
pub const FLOW_DECISION_EFFECT: &str = "choose";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ConsentOption {
    pub id: String,
    pub label: String,
    pub effect: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub kind: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Fact {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Preview {
    pub label: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Package {
    pub package: String,
    pub version: serde_json::Number,
    pub code_hash: String,
    pub script: String,
    pub manifest_json: String,
}

/// The request form stored after successful decoding.
///
/// Flow bodies are opaque objects: the consent module checks their presence and
/// option effects, but neither interprets manifests nor synthesizes card facts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ConsentRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<Package>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow_install: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow_decision: Option<Map<String, Value>>,
    pub kind: String,
    pub title: String,
    pub prompt: String,
    pub options: Vec<ConsentOption>,
    pub default: String,
    pub urgency: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedup_key: Option<String>,
    pub on_expiry: String,
    pub material_damage: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args_digest: Option<String>,
    pub late_execution: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_ref: Option<String>,
    pub target: Target,
    #[serde(default)]
    pub facts: Vec<Fact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<Preview>,
    pub expires_in_ms: i64,
    /// Checked against the option ids, then cleared by `decode`. Never serialized
    /// into a stored request, projection or dedup comparison.
    #[serde(default, rename = "requestedSchema", skip_serializing)]
    pub requested_schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding: Option<Value>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_at_console"
    )]
    pub at_console: Option<String>,
}

fn deserialize_at_console<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    if value.as_ref().is_some_and(|id| id.trim().is_empty()) {
        return Err(de::Error::custom("at_console must be nonempty"));
    }
    Ok(value)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub code: &'static str,
    pub message: String,
}

impl Refusal {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn invalid(message: impl Into<String>) -> Self {
        Self::new("elicitation_invalid_request", message)
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for Refusal {}

impl ConsentRequest {
    /// Decode the closed serde shape first, then apply ordered semantic rules.
    /// The returned form has no answer schema, including for equality/dedup.
    pub fn decode(body: Value) -> Result<Self, Refusal> {
        let mut request: Self =
            serde_json::from_value(body).map_err(|error| Refusal::invalid(error.to_string()))?;
        request.validate()?;
        request.requested_schema = None;
        Ok(request)
    }

    /// Validate in refusal precedence order. Serde shape checks belong in
    /// `decode`, so malformed fields cannot be masked by semantic refusals.
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.kind == "package" {
            let package = self
                .package
                .as_ref()
                .ok_or_else(|| Refusal::invalid("missing package"))?;
            if self.flow_install.is_some() || self.flow_decision.is_some() {
                return Err(Refusal::invalid("package cannot carry flow bodies"));
            }
            if !(package.version.is_i64() || package.version.is_u64()) {
                return Err(Refusal::invalid("package version must be a JSON integer"));
            }
            if package.code_hash.len() != 64
                || !package
                    .code_hash
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(Refusal::invalid(
                    "package code_hash must be lowercase 64-hex BLAKE3",
                ));
            }
            serde_json::from_str::<Value>(&package.manifest_json).map_err(|_| {
                Refusal::invalid("package manifest_json must be JSON encoded as a string")
            })?;
        } else if self.package.is_some() {
            return Err(Refusal::invalid("package body requires package kind"));
        }
        if self.kind == "flow_decision"
            && (self.session_ref.is_some()
                || self.parent_session_ref.is_some()
                || self.flow_install.is_some())
        {
            return Err(Refusal::invalid(
                "flow_decision cannot carry session references or flow_install",
            ));
        }
        if self.kind != "flow_decision" && self.flow_decision.is_some() {
            return Err(Refusal::invalid(
                "flow_decision body requires flow_decision kind",
            ));
        }
        if self.kind == "consent" && self.flow_install.is_some() {
            return Err(Refusal::invalid("consent cannot carry flow_install"));
        }
        if self.kind == "flow_decision" {
            if !(2..=4).contains(&self.options.len())
                || self
                    .options
                    .iter()
                    .filter(|option| option.effect == "decline")
                    .count()
                    != 1
                || self.options.iter().any(|option| {
                    option.effect != "decline" && option.effect != FLOW_DECISION_EFFECT
                })
            {
                return Err(Refusal::invalid(
                    "flow_decision needs 2 to 4 options: exactly one decline and otherwise choose",
                ));
            }
            if self.flow_decision.is_none() {
                return Err(Refusal::invalid("flow_decision body is required"));
            }
        } else if self
            .options
            .iter()
            .any(|option| option.effect == FLOW_DECISION_EFFECT)
        {
            return Err(Refusal::invalid("choose is only valid for flow_decision"));
        }
        if matches!(self.kind.as_str(), "flow_install" | "package") {
            if self.kind == "flow_install" && self.flow_install.is_none() {
                return Err(Refusal::invalid("missing flow_install"));
            }
            if self.options.len() != 2
                || self
                    .options
                    .iter()
                    .filter(|option| option.effect == "grant")
                    .count()
                    != 1
                || self
                    .options
                    .iter()
                    .filter(|option| option.effect == "decline")
                    .count()
                    != 1
            {
                return Err(Refusal::invalid(if self.kind == "package" {
                    "package needs exactly grant and decline"
                } else {
                    "flow install needs exactly grant and decline"
                }));
            }
        }
        if !matches!(
            self.kind.as_str(),
            "consent" | "flow_install" | "flow_decision" | "package"
        ) {
            return Err(Refusal::new(
                "elicitation_unknown_kind",
                "only consent is supported",
            ));
        }
        if self.title.trim().is_empty() || self.prompt.trim().is_empty() || self.options.is_empty()
        {
            return Err(Refusal::invalid(
                "title, prompt and nonempty options are required",
            ));
        }
        let mut ids = HashSet::new();
        for option in &self.options {
            if option.id.trim().is_empty()
                || option.label.trim().is_empty()
                || !ids.insert(&option.id)
            {
                return Err(Refusal::invalid(
                    "option ids must be unique and nonempty; labels must be nonempty",
                ));
            }
        }
        if let Some(schema) = &self.requested_schema {
            let mut schema_ids = schema
                .pointer("/properties/decision/enum")
                .and_then(Value::as_array)
                .and_then(|values| values.iter().map(Value::as_str).collect::<Option<Vec<_>>>())
                .ok_or_else(|| {
                    Refusal::invalid("requestedSchema decision enum must equal option ids")
                })?;
            let mut option_ids: Vec<_> = self
                .options
                .iter()
                .map(|option| option.id.as_str())
                .collect();
            schema_ids.sort_unstable();
            option_ids.sort_unstable();
            if schema_ids != option_ids {
                return Err(Refusal::invalid(
                    "requestedSchema decision enum must equal option ids",
                ));
            }
        }
        let default = self
            .options
            .iter()
            .find(|option| option.id == self.default)
            .ok_or_else(|| Refusal::invalid("default must name an option"))?;
        if default.effect != "decline" || self.on_expiry != "deny" {
            return Err(Refusal::new(
                "elicitation_fail_closed_required",
                "consent requires a decline default and on_expiry deny",
            ));
        }
        if !matches!(self.urgency.as_str(), "low" | "normal" | "high")
            || !matches!(self.late_execution.as_str(), "execute" | "notify_only")
        {
            return Err(Refusal::invalid("invalid urgency or late_execution"));
        }
        if !(1..=MAX_EXPIRES_IN_MS).contains(&self.expires_in_ms) {
            return Err(Refusal::invalid(
                "expires_in_ms must be positive and at most 24 hours",
            ));
        }
        if self.scope.is_none()
            && self
                .args_digest
                .as_ref()
                .is_none_or(|digest| digest.trim().is_empty())
        {
            return Err(Refusal::invalid("provide args_digest or scope"));
        }
        if self.target.kind.trim().is_empty() || self.target.label.trim().is_empty() {
            return Err(Refusal::invalid("target kind and label must be nonempty"));
        }
        Ok(())
    }
}
