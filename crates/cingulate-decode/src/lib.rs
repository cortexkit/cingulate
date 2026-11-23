//! Consent request decoding, independent of requester identity and storage.
//!
//! The caller must authorize the daemon's route stamp before decoding a request.
//! Body fields never supply the requester or subject identity.

use std::{collections::HashSet, fmt};

use serde::{de, Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

mod access;
mod broker_action;
mod config_plan;
mod flow_card_structure;
mod flow_secrets;
mod manifest_binding;
mod sandbox_escape;
mod sealed_text;
mod secret_use;
pub use access::{validate_note, AccessRequest};
pub use broker_action::{BrokerAction, BrokerActionAnswer};
pub use config_plan::{
    config_answer_bytes, ConfigApply, ConfigChange, ConfigLaunchManifest, ConfigModuleApply,
    ConfigOperation, ConfigPlan, ConfigPolicyChange, ConfigPolicyKind, ConfigProgramChange,
    ConfigReview, ConfigRollbackProgram, ConfigTransition,
};
pub use flow_card_structure::{Capabilities, CapabilityRow, Comparison, ComparisonRow};
pub use flow_secrets::{FlowIdentity, FlowInstallSecrets};
pub use manifest_binding::{ManifestBinding, ManifestBindingAnswer, ManifestBindingStatement};
pub use sandbox_escape::{SandboxEscape, SandboxLevel};
pub use sealed_text::{
    sealed_answer_fingerprint, HpkeKey, SealedText, SealedTextAnswer, SEALED_TEXT_SUITE,
};
pub use secret_use::{SecretUse, SecretUseAnswer, SecretUseItem, UseStatement};

pub const MAX_EXPIRES_IN_MS: i64 = 86_400_000;
pub const FLOW_DECISION_EFFECT: &str = "choose";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OptionGroup {
    Danger,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ConsentOption {
    pub id: String,
    pub label: String,
    pub effect: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_option_group"
    )]
    pub group: Option<OptionGroup>,
}

fn deserialize_option_group<'de, D>(deserializer: D) -> Result<Option<OptionGroup>, D::Error>
where
    D: Deserializer<'de>,
{
    let group = String::deserialize(deserializer)?;
    serde_json::from_value(Value::String(group))
        .map(Some)
        .map_err(de::Error::custom)
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

pub fn lowercase_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub language: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DryRunText {
    pub summary: String,
    pub would: Vec<String>,
    pub problems: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AuthorText {
    pub label: String,
    pub text: String,
}

/// The request form stored after successful decoding.
///
/// Flow bodies stay opaque except for the vault identity and secret approvals;
/// the consent module never interprets executable manifests. Manifest-binding
/// facts are a read-only projection of the vault's statement, not filer text.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ConsentRequest {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_inputs"
    )]
    pub inputs: Option<Vec<SealedText>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_plan: Option<ConfigPlan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_binding: Option<ManifestBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub broker_action: Option<BrokerAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_escape: Option<SandboxEscape>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_use: Option<SecretUse>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<Package>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow_install: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow_decision: Option<Map<String, Value>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_source"
    )]
    pub source: Option<Source>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_dry_run_text"
    )]
    pub dry_run_text: Option<DryRunText>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_explanation"
    )]
    pub explanation: Option<AuthorText>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_changes"
    )]
    pub changes: Option<AuthorText>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "flow_card_structure::present"
    )]
    pub warnings: Option<Vec<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "flow_card_structure::present"
    )]
    pub comparison: Option<Comparison>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "flow_card_structure::present"
    )]
    pub capabilities: Option<Capabilities>,
    pub kind: String,
    pub title: String,
    pub prompt: String,
    pub options: Vec<ConsentOption>,
    pub default: String,
    pub urgency: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedup_key: Option<String>,
    /// Correlates a request with its later outcome record for prefrontal-core,
    /// without becoming card text or a deduplication key.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_requester_ref"
    )]
    pub requester_ref: Option<String>,
    /// Marks consent grants that are valid for one use only.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_single_use"
    )]
    pub single_use: Option<bool>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_in_ms: Option<i64>,
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

fn deserialize_requester_ref<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    let reference = value.as_str().filter(|reference| {
        (1..=128).contains(&reference.len())
            && reference.bytes().all(|byte| (b' '..=b'~').contains(&byte))
    });
    reference
        .map(|reference| Some(reference.to_owned()))
        .ok_or_else(|| {
            de::Error::custom(
                "requester_ref must be a string of 1 to 128 printable ASCII characters",
            )
        })
}

fn deserialize_inputs<'de, D>(deserializer: D) -> Result<Option<Vec<SealedText>>, D::Error>
where
    D: Deserializer<'de>,
{
    Vec::deserialize(deserializer).map(Some)
}

fn deserialize_single_use<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: Deserializer<'de>,
{
    Value::deserialize(deserializer)?
        .as_bool()
        .map(Some)
        .ok_or_else(|| de::Error::custom("single_use must be a boolean"))
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

fn deserialize_source<'de, D>(deserializer: D) -> Result<Option<Source>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    serde_json::from_value(value)
        .map(Some)
        .map_err(|error| {
            de::Error::custom(format!(
                "source: language is limited to 32 bytes and both values must be nonblank strings: {error}"
            ))
        })
}

fn deserialize_dry_run_text<'de, D>(deserializer: D) -> Result<Option<DryRunText>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    serde_json::from_value(value)
        .map(Some)
        .map_err(|error| {
            de::Error::custom(format!(
                "dry_run_text: summary is limited to 300 bytes; each list is limited to 8 items of at most 200 bytes, with no ASCII controls: {error}"
            ))
        })
}

fn deserialize_author_text<'de, D>(
    deserializer: D,
    field: &str,
) -> Result<Option<AuthorText>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    let object = value
        .as_object()
        .ok_or_else(|| de::Error::custom(format!("{field} must be an object")))?;
    for key in object.keys() {
        if !matches!(key.as_str(), "label" | "text") {
            return Err(de::Error::custom(format!("unknown field {field}.{key}")));
        }
    }
    let string = |member: &str| {
        object
            .get(member)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| de::Error::custom(format!("{field}.{member} must be a string")))
    };
    Ok(Some(AuthorText {
        label: string("label")?,
        text: string("text")?,
    }))
}

fn deserialize_explanation<'de, D>(deserializer: D) -> Result<Option<AuthorText>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_author_text(deserializer, "explanation")
}

fn deserialize_changes<'de, D>(deserializer: D) -> Result<Option<AuthorText>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_author_text(deserializer, "changes")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub code: &'static str,
    pub message: String,
    /// Fixed diagnostic label for logs only; never derived from caller data.
    pub reason: Option<&'static str>,
}

impl Refusal {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            reason: None,
        }
    }

    pub fn reason(mut self, reason: &'static str) -> Self {
        self.reason = Some(reason);
        self
    }

    fn invalid(message: impl Into<String>) -> Self {
        Self::new("elicitation_invalid_request", message).reason("envelope_invalid")
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
        let body = match body.get("kind").and_then(Value::as_str) {
            Some("broker_action") => broker_action::normalize(&body)?,
            Some("sandbox_escape") => sandbox_escape::normalize(body)?,
            _ => body,
        };
        if body.get("inputs").is_some()
            && body.get("kind").and_then(Value::as_str) != Some("consent")
        {
            return Err(Refusal::invalid("inputs require consent kind"));
        }
        if !matches!(
            body.get("kind").and_then(Value::as_str),
            Some("flow_install" | "package")
        ) {
            for field in [
                "explanation",
                "changes",
                "warnings",
                "comparison",
                "capabilities",
            ] {
                if body.get(field).is_some() {
                    return Err(Refusal::invalid(format!(
                        "{field} is only valid for flow_install or package cards"
                    )));
                }
            }
        }
        let body = if body.get("kind").and_then(Value::as_str) == Some("secret_use") {
            secret_use::normalize(body)?
        } else if body.get("kind").and_then(Value::as_str) == Some("config_plan") {
            config_plan::normalize(body)?
        } else if body.get("kind").and_then(Value::as_str) == Some("manifest_binding") {
            manifest_binding::normalize(body)?
        } else {
            body
        };
        let mut request: Self = serde_json::from_value(body).map_err(|error| {
            // Keep only our fixed custom field messages; serde's general shape
            // errors may quote caller-authored keys or values.
            Refusal::invalid(match error.to_string().as_str() {
                "single_use must be a boolean" => "single_use must be a boolean",
                "at_console must be nonempty" => "at_console must be nonempty",
                "requester_ref must be a string of 1 to 128 printable ASCII characters" => {
                    "requester_ref must be a string of 1 to 128 printable ASCII characters"
                }
                _ => "consent request fields are invalid",
            })
        })?;
        if request.flow_install_secrets()?.is_some() {
            request.on_expiry = "deny".into();
        }
        request.validate()?;
        request.requested_schema = None;
        Ok(request)
    }

    /// Validate in refusal precedence order. Serde shape checks belong in
    /// `decode`, so malformed fields cannot be masked by semantic refusals.
    pub fn validate(&self) -> Result<(), Refusal> {
        self.validate_inputs()?;
        self.validate_flow_card_structure()?;
        if (self.kind == "config_plan") != self.config_plan.is_some() {
            return Err(Refusal::invalid(
                "config_plan body requires config_plan kind",
            ));
        }
        for (field, value) in [
            ("explanation", &self.explanation),
            ("changes", &self.changes),
        ] {
            if let Some(value) = value {
                if !matches!(self.kind.as_str(), "flow_install" | "package") {
                    return Err(Refusal::invalid(format!(
                        "{field} is only valid for flow_install or package cards"
                    )));
                }
                validate_author_text(field, value)?;
            }
        }
        self.flow_install_secrets()?;
        match (&self.sandbox_escape, self.kind.as_str()) {
            (Some(body), "sandbox_escape") => body.validate()?,
            (None, "sandbox_escape") => return Err(Refusal::invalid("missing sandbox_escape")),
            (Some(_), _) => {
                return Err(Refusal::invalid(
                    "sandbox_escape body requires sandbox_escape kind",
                ))
            }
            (None, _) => {}
        }
        if self.kind == "broker_action" {
            self.broker_action
                .as_ref()
                .ok_or_else(|| Refusal::invalid("missing broker_action.statement_b64url"))?
                .expires_at_ms()?;
        } else if self.broker_action.is_some() {
            return Err(Refusal::invalid(
                "broker_action body requires broker_action kind",
            ));
        }
        if self.kind == "manifest_binding" {
            self.manifest_binding
                .as_ref()
                .ok_or_else(|| Refusal::invalid("missing manifest_binding"))?
                .statement()?;
        } else if self.manifest_binding.is_some() {
            return Err(Refusal::invalid(
                "manifest_binding body requires manifest_binding kind",
            ));
        }
        if self.kind == "secret_use" {
            self.secret_use
                .as_ref()
                .ok_or_else(|| Refusal::invalid("missing secret_use"))?
                .statements()?;
        } else if self.secret_use.is_some() {
            return Err(Refusal::invalid("secret_use body requires secret_use kind"));
        }
        if !matches!(self.kind.as_str(), "flow_install" | "package") {
            if self.source.is_some() {
                return Err(Refusal::invalid(
                    "source is only valid for flow_install or package cards",
                ));
            }
            if self.dry_run_text.is_some() {
                return Err(Refusal::invalid(
                    "dry_run_text is only valid for flow_install or package cards",
                ));
            }
        }
        if let Some(source) = &self.source {
            if source.language.trim().is_empty() || source.language.len() > 32 {
                return Err(Refusal::invalid(
                    "source.language must be nonblank and at most 32 bytes",
                ));
            }
            if source.text.trim().is_empty() {
                return Err(Refusal::invalid("source.text must be nonblank"));
            }
        }
        if let Some(dry_run_text) = &self.dry_run_text {
            validate_dry_run_text(dry_run_text)?;
        }
        if self.requester_ref.is_some() && self.kind != "consent" {
            return Err(Refusal::invalid("requester_ref requires consent kind"));
        }
        if self.single_use.is_some() && self.kind != "consent" {
            return Err(Refusal::invalid("single_use requires consent kind"));
        }
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
        } else if self.kind != "config_plan"
            && self
                .options
                .iter()
                .any(|option| option.effect == FLOW_DECISION_EFFECT)
        {
            return Err(Refusal::invalid("choose is only valid for flow_decision"));
        }
        if matches!(
            self.kind.as_str(),
            "flow_install" | "package" | "secret_use" | "manifest_binding" | "broker_action"
        ) {
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
                return Err(Refusal::invalid(match self.kind.as_str() {
                    "package" => "package needs exactly grant and decline",
                    _ => "flow install needs exactly grant and decline",
                }));
            }
        }
        if !matches!(
            self.kind.as_str(),
            "consent"
                | "flow_install"
                | "flow_decision"
                | "package"
                | "secret_use"
                | "manifest_binding"
                | "broker_action"
                | "sandbox_escape"
                | "config_plan"
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
            if option.id.chars().count() > 64
                || option.label.chars().count() > 200
                || option.id.chars().any(char::is_control)
                || option.label.chars().any(char::is_control)
            {
                return Err(Refusal::invalid("option.id must be 1-64 characters and option.label 1-200, without control characters"));
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
            )
            .reason(if default.effect != "decline" {
                "default_not_decline"
            } else {
                "expiry_not_deny"
            }));
        }
        if default.group == Some(OptionGroup::Danger) {
            return Err(Refusal::new(
                "elicitation_fail_closed_required",
                "the default option cannot be in the danger group",
            )
            .reason("default_danger"));
        }
        if self.options.iter().any(|option| option.effect == "grant")
            && !self
                .options
                .iter()
                .any(|option| option.effect == "grant" && option.group != Some(OptionGroup::Danger))
        {
            return Err(Refusal::invalid(
                "at least one grant option must be outside the danger group",
            ));
        }
        if !matches!(self.urgency.as_str(), "low" | "normal" | "high")
            || !matches!(self.late_execution.as_str(), "execute" | "notify_only")
        {
            return Err(Refusal::invalid("invalid urgency or late_execution"));
        }
        if self
            .expires_in_ms
            .is_some_and(|expiry| !(1..=MAX_EXPIRES_IN_MS).contains(&expiry))
        {
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

    pub fn flow_install_secrets(&self) -> Result<Option<FlowInstallSecrets>, Refusal> {
        if self.kind == "flow_install" {
            self.flow_install
                .as_ref()
                .map(FlowInstallSecrets::decode)
                .transpose()
                .map(Option::flatten)
        } else {
            Ok(None)
        }
    }

    pub fn is_secret_card(&self) -> bool {
        self.manifest_binding.is_some()
            || self.broker_action.is_some()
            || self.secret_use.is_some()
            || (self.kind == "flow_install"
                && self
                    .flow_install
                    .as_ref()
                    .is_some_and(|body| body.contains_key("secrets")))
    }

    /// Whether only the phone can approve this card.
    pub fn answer_needs_phone(&self) -> bool {
        Self::kind_answer_needs_phone(
            self.is_secret_card() || self.sandbox_escape.is_some(),
            self.kind == "config_plan",
        )
    }

    /// Phone-only rule shared by pending-card readers and notices announcing cards
    /// that need an operator's attention. A watch can check these flags even after
    /// the card is deleted.
    /// Development local-operator builds widen secret approvals to the desktop
    /// with unsigned development envelopes; config plans still need the phone
    /// because the daemon confirms them on the computer itself.
    pub fn kind_answer_needs_phone(secret_card: bool, config_plan: bool) -> bool {
        config_plan || (secret_card && !cfg!(feature = "dev-local-operator"))
    }
}

fn validate_author_text(field: &str, value: &AuthorText) -> Result<(), Refusal> {
    if value.label.is_empty() || value.label.chars().count() > 64 {
        return Err(Refusal::invalid(format!(
            "{field}.label must be nonempty and at most 64 Unicode scalar values"
        )));
    }
    if value.text.is_empty() || value.text.len() > 8192 {
        return Err(Refusal::invalid(format!(
            "{field}.text must be nonempty and at most 8192 UTF-8 bytes"
        )));
    }
    for (member, text) in [("label", &value.label), ("text", &value.text)] {
        if text.chars().any(|c| c.is_control() && c != '\n') {
            return Err(Refusal::invalid(format!(
                "{field}.{member} must not contain control characters except newline"
            )));
        }
    }
    Ok(())
}

fn validate_dry_run_text(dry_run_text: &DryRunText) -> Result<(), Refusal> {
    let has_control = |value: &str| value.chars().any(|character| character.is_ascii_control());
    if dry_run_text.summary.trim().is_empty() || dry_run_text.summary.len() > 300 {
        return Err(Refusal::invalid(
            "dry_run_text.summary must be nonblank and at most 300 bytes",
        ));
    }
    if has_control(&dry_run_text.summary) {
        return Err(Refusal::invalid(
            "dry_run_text.summary must not contain ASCII control characters",
        ));
    }
    for (field, items) in [
        ("would", &dry_run_text.would),
        ("problems", &dry_run_text.problems),
    ] {
        if items.len() > 8 {
            return Err(Refusal::invalid(format!(
                "dry_run_text.{field} must contain at most 8 items"
            )));
        }
        for item in items {
            if item.trim().is_empty() || item.len() > 200 {
                return Err(Refusal::invalid(format!(
                    "dry_run_text.{field} items must be nonblank and at most 200 bytes"
                )));
            }
            if has_control(item) {
                return Err(Refusal::invalid(format!(
                    "dry_run_text.{field} items must not contain ASCII control characters"
                )));
            }
        }
    }
    Ok(())
}
