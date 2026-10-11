use cingulate_decode::ConsentRequest;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ElicitationRecord {
    pub elicitation_id: String,
    pub request: ConsentRequest,
    pub created_at_ms: i64,
    pub expires_at_ms: i64,
    pub subject: Subject,
    pub state: State,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answered_choice_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answered_at_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution: Option<String>,
    pub requester: String,
}

impl ElicitationRecord {
    /// Build the flat camelCase JSON shown on the phone. Omit `binding` and
    /// `requester_ref`: private requester metadata must not become card text.
    /// Nested opaque JSON is copied verbatim.
    pub fn phone(&self, block_until: Option<i64>) -> Value {
        let mut value = serde_json::to_value(self).expect("record serialization");
        let object = value.as_object_mut().expect("record object");
        object.remove("request");
        object.remove("requester");
        for (key, value) in serde_json::to_value(&self.request)
            .expect("request serialization")
            .as_object()
            .expect("request object")
        {
            if key != "binding" && key != "requester_ref" {
                object.insert(camel(key), value.clone());
            }
        }
        object.insert("requesterLabel".into(), json!(self.requester));
        if self.state == State::Pending {
            if let Some(until) = block_until {
                object.insert("blockUntilMs".into(), json!(until.min(self.expires_at_ms)));
            }
        }
        value
    }

    /// Only record and subject keys change spelling; scope, preview, bindings
    /// and typed card payloads are not recursively renamed.
    pub fn requester_wire(&self, block_until: Option<i64>) -> Value {
        let mut value = self.phone(block_until);
        value["subject"] = snake_object(value["subject"].take());
        let mut value = snake_object(value);
        if let Some(reference) = &self.request.requester_ref {
            value["requester_ref"] = json!(reference);
        }
        if let Some(binding) = &self.request.binding {
            value["binding"] = binding.clone();
        }
        value
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
