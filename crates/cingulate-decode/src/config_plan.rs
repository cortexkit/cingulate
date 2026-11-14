use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::Refusal;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ConfigChange {
    pub module_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub path: String,
    pub old: Value,
    pub new: Value,
    pub old_source: String,
    pub new_source: String,
    pub apply: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ConfigPlan {
    pub plan_digest: String,
    pub base_revision: String,
    pub changes: Vec<ConfigChange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub other_sections: Option<Map<String, Value>>,
}

fn bounded(value: &str, max: usize, field: &str) -> Result<(), Refusal> {
    if value.is_empty() || value.len() > max {
        return Err(Refusal::invalid(format!(
            "{field} must be 1..={max} UTF-8 bytes"
        )));
    }
    Ok(())
}

pub(super) fn normalize(body: Value) -> Result<Value, Refusal> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Filing {
        kind: String,
        expires_in_ms: i64,
        binding: Map<String, Value>,
        #[serde(default)]
        dedup_key: Option<String>,
        config_plan: ConfigPlan,
    }
    // Optional fields may be omitted, but a present JSON null must not be
    // silently treated as an omitted object or project id by serde's Option.
    if body["config_plan"]
        .get("other_sections")
        .is_some_and(|v| !v.is_object())
    {
        return Err(Refusal::invalid(
            "config_plan.other_sections must be an object",
        ));
    }
    if let Some(rows) = body["config_plan"]["changes"].as_array() {
        for row in rows {
            if row.get("project_id").is_some_and(|v| !v.is_string()) {
                return Err(Refusal::invalid(
                    "config_plan.changes.project_id must be a string",
                ));
            }
        }
    }
    let filing: Filing = serde_json::from_value(body)
        .map_err(|e| Refusal::invalid(format!("config_plan filing: {e}")))?;
    if !(1..=300_000).contains(&filing.expires_in_ms) {
        return Err(Refusal::invalid(
            "config_plan.expires_in_ms must be 1..=300000",
        ));
    }
    if serde_json::to_vec(&filing.binding).expect("JSON").len() > 4096 {
        return Err(Refusal::invalid("config_plan binding exceeds 4096 bytes"));
    }
    let plan = &filing.config_plan;
    bounded(&plan.plan_digest, 128, "config_plan.plan_digest")?;
    if plan.base_revision.is_empty() || !plan.base_revision.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Refusal::invalid(
            "config_plan.base_revision must be a decimal string",
        ));
    }
    if !(1..=64).contains(&plan.changes.len()) {
        return Err(Refusal::invalid(
            "config_plan.changes must contain 1..=64 rows",
        ));
    }
    for row in &plan.changes {
        bounded(&row.module_id, 128, "config_plan.changes.module_id")?;
        if let Some(project) = &row.project_id {
            bounded(project, 128, "config_plan.changes.project_id")?;
        }
        bounded(&row.path, 1024, "config_plan.changes.path")?;
        bounded(&row.old_source, 64, "config_plan.changes.old_source")?;
        bounded(&row.new_source, 64, "config_plan.changes.new_source")?;
        bounded(&row.apply, 64, "config_plan.changes.apply")?;
        for (field, value) in [("old", &row.old), ("new", &row.new)] {
            if serde_json::to_vec(value).expect("JSON").len() > 4096 {
                return Err(Refusal::invalid(format!(
                    "config_plan.changes.{field} exceeds 4096 bytes"
                )));
            }
        }
    }
    if plan
        .other_sections
        .as_ref()
        .is_some_and(|sections| serde_json::to_vec(sections).expect("JSON").len() > 16384)
    {
        return Err(Refusal::invalid(
            "config_plan.other_sections exceeds 16384 bytes",
        ));
    }
    Ok(json!({
        "kind":filing.kind, "config_plan":plan, "binding":filing.binding,
        "expires_in_ms":filing.expires_in_ms, "dedup_key":filing.dedup_key,
        "title":"Settings change approval", "prompt":"Review this computer's settings changes.",
        "options":[{"id":"approve","label":"Approve","effect":"choose"},{"id":"decline","label":"Decline","effect":"decline"}],
        "default":"decline", "on_expiry":"deny", "urgency":"normal", "material_damage":false,
        "late_execution":"notify_only", "scope":{}, "target":{"kind":"config_plan","label":"This computer's settings"}
    }))
}

/// Decode only the transport encoding and bound its size. The verifier, not
/// Cingulate, interprets the signed bytes or checks their signature.
pub fn config_answer_bytes(value: &str, max: usize, field: &str) -> Result<Vec<u8>, Refusal> {
    if value.is_empty() || value.len() > max.div_ceil(3) * 4 {
        return Err(Refusal::invalid(format!(
            "{field} exceeds its encoded size bound or is empty"
        )));
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| Refusal::invalid(format!("{field} must be unpadded base64url")))?;
    if bytes.len() > max {
        return Err(Refusal::invalid(format!(
            "{field} exceeds {max} decoded bytes"
        )));
    }
    Ok(bytes)
}
