use cingulate_decode::ConsentRequest;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::ScopeIdentity;

/// The `session.notice` purpose for an answer that arrives after the requester
/// stopped holding the agent's call.
pub const LATE_ANSWER_PURPOSE: &str = "consent_answered";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Pending,
    Answered,
    Expired,
    Withdrawn,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Answered => "answered",
            Self::Expired => "expired",
            Self::Withdrawn => "withdrawn",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Subject {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub head_agent_id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub via_worker: bool,
    /// Daemon-attested remote facts, never a local agent identity. Nested keys
    /// retain their protocol spelling on both phone and requester projections.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub federated: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ElicitationRecord {
    #[serde(
        default,
        rename = "input_supplied",
        skip_serializing_if = "std::ops::Not::not"
    )]
    pub input_supplied: bool,
    #[serde(skip)]
    pub sealed_inputs: Option<Vec<cingulate_decode::SealedTextAnswer>>,
    pub elicitation_id: String,
    pub request: ConsentRequest,
    pub created_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<i64>,
    pub subject: Subject,
    pub state: State,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answered_choice_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answered_at_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answered_device: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answered_surface: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_use_answer: Option<cingulate_decode::SecretUseAnswer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_binding_answer: Option<cingulate_decode::ManifestBindingAnswer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub broker_action_answer: Option<cingulate_decode::BrokerActionAnswer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statement: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature_b64url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unverified: Option<Value>,
    #[serde(default, skip_serializing_if = "zero")]
    pub notice_round: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_reason: Option<String>,
    pub requester: String,
    /// Set once the requester stops holding the agent's call. Requester-private:
    /// no phone or feed projection shows it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub detached: bool,
    /// Persist whether the card settled after its requester detached so later
    /// outcome reads do not infer that timing from a clock or old request history.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub late: bool,
    /// The session stamped on the requesting route, used for late-answer notices
    /// and the session-ended execution backstop. No projection shows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asker: Option<ScopeIdentity>,
    /// Parent scope attested when the card was filed; older cards have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<ScopeIdentity>,
}

impl ElicitationRecord {
    pub fn requester_inputs_wire(&self, block_until: Option<i64>) -> Value {
        let mut value = self.requester_wire(block_until);
        if self.input_supplied {
            value
                .as_object_mut()
                .expect("card object")
                .remove("input_supplied");
            if let Some(inputs) = &self.sealed_inputs {
                value["answer_inputs"] = json!(inputs);
            } else {
                value["input_released"] = json!(true);
            }
        }
        value
    }
    /// Build the flat camelCase card JSON. Hide the requester principal,
    /// `binding`, `requester_ref`, and `single_use`; these values are not card text.
    /// Nested JSON values are copied verbatim.
    pub fn phone(&self, block_until: Option<i64>) -> Value {
        let mut value = serde_json::to_value(self).expect("record serialization");
        let object = value.as_object_mut().expect("record object");
        object.remove("request");
        object.remove("requester");
        object.remove("detached");
        object.remove("asker");
        object.remove("head");
        object.remove("late");
        object.remove("noticeRound");
        if self.notice_round > 0 {
            object.insert("round".into(), json!(self.notice_round));
        }
        for (key, value) in serde_json::to_value(&self.request)
            .expect("request serialization")
            .as_object()
            .expect("request object")
        {
            if key != "binding" && key != "requester_ref" && key != "single_use" {
                object.insert(camel(key), value.clone());
            }
        }
        // Historical cards without a restart summary cannot show the full
        // change, so their only available decision is to decline.
        if self
            .request
            .config_plan
            .as_ref()
            .is_some_and(|plan| plan.apply.is_none())
        {
            if let Some(options) = object.get_mut("options").and_then(Value::as_array_mut) {
                options.retain(|option| option["id"] == "decline");
            }
        }
        object.insert("requesterLabel".into(), json!(self.requester));
        if self.state == State::Pending {
            if let Some(until) = block_until {
                object.insert(
                    "blockUntilMs".into(),
                    json!(self.expires_at_ms.map_or(until, |expiry| until.min(expiry))),
                );
            }
        }
        value
    }

    /// Only record and subject keys change spelling; scope, preview, bindings
    /// and typed card payloads are not recursively renamed.
    pub fn requester_wire(&self, block_until: Option<i64>) -> Value {
        let mut value = self.requester_base_wire(block_until, true);
        if let Some(single_use) = self.request.single_use {
            value["single_use"] = json!(single_use);
        }
        if let Some(binding) = &self.request.binding {
            value["binding"] = binding.clone();
        }
        value
    }

    /// Build a `consent.outcomes` record for prefrontal-core. Include
    /// `requester_ref` unchanged and expose `single_use` and `late` only when true.
    pub fn outcome_wire(&self) -> Value {
        let mut value = self.requester_base_wire(None, false);
        if let Some(reference) = &self.request.requester_ref {
            value["requester_ref"] = json!(reference);
        }
        if self.request.single_use == Some(true) {
            value["single_use"] = json!(true);
        }
        if self.late {
            value["late"] = json!(true);
        }
        value
    }

    fn requester_base_wire(&self, block_until: Option<i64>, include_requester_ref: bool) -> Value {
        let mut value = self.phone(block_until);
        value["subject"] = snake_object(value["subject"].take());
        let mut value = snake_object(value);
        if include_requester_ref {
            if let Some(reference) = &self.request.requester_ref {
                value["requester_ref"] = json!(reference);
            }
        }
        value
    }
}

/// Do not reorder these fields: serde emits them in declaration order, and the
/// notice bytes must match the files in test-vectors/consent-late-answer-v1/.
#[derive(Serialize)]
struct LateAnswerNotice<'a> {
    elicitation_id: &'a str,
    purpose: &'static str,
    asker: NoticeTarget<'a>,
    fields: LateAnswerFields<'a>,
}

#[derive(Serialize)]
struct NoticeTarget<'a> {
    scope_ref: &'a str,
    scope_epoch: u64,
}

#[derive(Serialize)]
struct LateAnswerFields<'a> {
    requester: &'a str,
    outcome: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    choice_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    choice_label: Option<&'a str>,
    settled_at_ms: u64,
}

impl ElicitationRecord {
    /// The exact `session.notice` body for a settled detached card, or `None`
    /// for a pending card or one raised on a route with no stamped session.
    /// It carries only the outcome: never `requester_ref`, the prompt, facts
    /// or any other request content.
    pub fn late_answer_notice(&self) -> Option<String> {
        let asker = self.asker.as_ref()?;
        let settled_at_ms = self.answered_at_ms?;
        let outcome = match self.state {
            State::Pending => return None,
            State::Answered => "answered",
            State::Expired => "expired",
            State::Withdrawn => "withdrawn",
        };
        // Only an operator's answer names a choice; expiry and withdrawal do not.
        let choice = (self.state == State::Answered)
            .then_some(self.answered_choice_id.as_deref())
            .flatten()
            .and_then(|id| self.request.options.iter().find(|option| option.id == id));
        let notice = LateAnswerNotice {
            elicitation_id: &self.elicitation_id,
            purpose: LATE_ANSWER_PURPOSE,
            asker: NoticeTarget {
                scope_ref: &asker.scope_ref,
                scope_epoch: asker.scope_epoch,
            },
            fields: LateAnswerFields {
                // The module id of the raising principal, without `reserved:`.
                requester: self
                    .requester
                    .strip_prefix("reserved:")
                    .unwrap_or(&self.requester),
                outcome,
                choice_id: choice.map(|option| option.id.as_str()),
                choice_label: choice.map(|option| option.label.as_str()),
                settled_at_ms: settled_at_ms.max(0) as u64,
            },
        };
        Some(serde_json::to_string(&notice).expect("notice serialization"))
    }
}

fn camel(key: &str) -> String {
    let mut parts = key.split('_');
    let mut result = parts.next().unwrap_or_default().to_owned();
    for part in parts {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            result.extend(first.to_uppercase());
            result.extend(chars);
        }
    }
    result
}

fn zero(value: &u64) -> bool {
    *value == 0
}

fn snake_object(value: Value) -> Value {
    let fields: Map<String, Value> = value
        .as_object()
        .expect("record object")
        .iter()
        .map(|(key, value)| {
            let mut snake = String::new();
            for ch in key.chars() {
                if ch.is_ascii_uppercase() {
                    snake.push('_');
                    snake.push(ch.to_ascii_lowercase());
                } else {
                    snake.push(ch);
                }
            }
            (snake, value.clone())
        })
        .collect();
    Value::Object(fields)
}
