use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashSet;

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
    // Older stored cards have no apply summary. Filing requires it, and those
    // historical cards remain readable but cannot offer approval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apply: Option<ConfigApply>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affected_projects: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_changes: Option<Vec<ConfigPolicyChange>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_manifests: Option<Vec<ConfigLaunchManifest>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub other_sections: Option<Map<String, Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ConfigApply {
    pub operation: ConfigOperation,
    pub rollback_to: Option<String>,
    pub modules: Vec<ConfigModuleApply>,
    pub review: Option<ConfigReview>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollback_programs: Option<Vec<ConfigRollbackProgram>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConfigOperation {
    Change,
    Rollback,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConfigTransition {
    Live,
    ModuleRestart,
    RouteRebind,
    NextStart,
    DaemonRestart,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ConfigReview {
    pub supervisor_reconcile: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConfigPolicyKind {
    Declaration,
    SubcJsonc,
    ReviewedProgram,
    LayerFile,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ConfigModuleApply {
    pub module_id: String,
    pub transition: ConfigTransition,
    pub whole_generation: bool,
    pub waits_for_next_start: bool,
    pub waits_for_daemon_restart: bool,
    pub deferred_live_fields: Vec<String>,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ConfigPolicyChange {
    pub kind: ConfigPolicyKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
    pub old: Value,
    pub new: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ConfigLaunchManifest {
    pub module_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    pub programs: Vec<ConfigProgramChange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program_changed_since_target: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program_changed_since_active: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ConfigProgramChange {
    pub input: String,
    pub old: Value,
    pub new: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ConfigRollbackProgram {
    pub module_id: String,
    pub input: String,
    pub approved_then: Value,
    pub on_disk_now: Value,
}

fn list_bound(len: usize, min: usize, max: usize, field: &str) -> Result<(), Refusal> {
    if !(min..=max).contains(&len) {
        return Err(Refusal::invalid(format!(
            "{field} must contain {min}..={max} rows"
        )));
    }
    Ok(())
}

fn json_bound(value: &Value, field: &str) -> Result<(), Refusal> {
    if serde_json::to_vec(value).expect("JSON").len() > 4096 {
        return Err(Refusal::invalid(format!("{field} exceeds 4096 bytes")));
    }
    Ok(())
}

fn unique<'a>(values: impl Iterator<Item = &'a str>, field: &str) -> Result<(), Refusal> {
    let mut seen = HashSet::new();
    for value in values {
        bounded(value, 128, field)?;
        if !seen.insert(value) {
            return Err(Refusal::invalid(format!("{field} must be unique")));
        }
    }
    Ok(())
}

fn typed_field<T: serde::de::DeserializeOwned>(value: &Value, field: &str) -> Result<(), Refusal> {
    serde_json::from_value::<T>(value.clone())
        .map(|_| ())
        .map_err(|error| Refusal::invalid(format!("{field}: {error}")))
}

fn layer_file_value(value: &Value, field: &str) -> Result<(), Refusal> {
    let valid = value.as_object().is_some_and(|object| {
        (object.len() == 1 && object.get("absent") == Some(&Value::Bool(true)))
            || (object.len() == 2
                && object
                    .get("sha256")
                    .and_then(Value::as_str)
                    .is_some_and(|hash| {
                        hash.len() == 64
                            && hash
                                .bytes()
                                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    })
                && object.get("bytes").and_then(Value::as_u64).is_some())
    });
    if !valid {
        return Err(Refusal::invalid(format!("{field} must be {{absent: true}} or a closed {{sha256: 64 lowercase hex, bytes: non-negative integer}} object")));
    }
    Ok(())
}

fn validate_sections(plan: &ConfigPlan) -> Result<(), Refusal> {
    let apply = plan
        .apply
        .as_ref()
        .ok_or_else(|| Refusal::invalid("config_plan.apply is required"))?;
    match (apply.operation, &apply.rollback_to) {
        (ConfigOperation::Change, None) => {},
        (ConfigOperation::Rollback, Some(revision)) if !revision.is_empty() && revision.bytes().all(|b| b.is_ascii_digit()) => {},
        _ => return Err(Refusal::invalid("config_plan.apply.rollback_to must be a nonempty decimal string exactly for rollback, otherwise null")),
    }
    if let Some(programs) = &apply.rollback_programs {
        if apply.operation != ConfigOperation::Rollback {
            return Err(Refusal::invalid(
                "config_plan.apply.rollback_programs is only allowed for rollback",
            ));
        }
        list_bound(programs.len(), 0, 64, "config_plan.apply.rollback_programs")?;
        for row in programs {
            bounded(
                &row.module_id,
                128,
                "config_plan.apply.rollback_programs.module_id",
            )?;
            bounded(
                &row.input,
                1024,
                "config_plan.apply.rollback_programs.input",
            )?;
            json_bound(
                &row.approved_then,
                "config_plan.apply.rollback_programs.approved_then",
            )?;
            json_bound(
                &row.on_disk_now,
                "config_plan.apply.rollback_programs.on_disk_now",
            )?;
        }
    }
    if apply
        .review
        .as_ref()
        .is_some_and(|review| !review.supervisor_reconcile)
    {
        return Err(Refusal::invalid(
            "config_plan.apply.review.supervisor_reconcile must be true",
        ));
    }
    list_bound(apply.modules.len(), 0, 64, "config_plan.apply.modules")?;
    unique(
        apply.modules.iter().map(|row| row.module_id.as_str()),
        "config_plan.apply.modules.module_id",
    )?;
    for row in &apply.modules {
        list_bound(
            row.deferred_live_fields.len(),
            0,
            64,
            "config_plan.apply.modules.deferred_live_fields",
        )?;
        for path in &row.deferred_live_fields {
            bounded(path, 1024, "config_plan.apply.modules.deferred_live_fields")?;
        }
        bounded(
            &row.explanation,
            512,
            "config_plan.apply.modules.explanation",
        )?;
        if row
            .explanation
            .chars()
            .any(|character| character.is_control() && character != '\n')
        {
            return Err(Refusal::invalid("config_plan.apply.modules.explanation must be plain text without control characters except newline"));
        }
    }
    if let Some(projects) = &plan.affected_projects {
        list_bound(projects.len(), 0, 64, "config_plan.affected_projects")?;
        unique(
            projects.iter().map(String::as_str),
            "config_plan.affected_projects",
        )?;
    }
    if let Some(rows) = &plan.policy_changes {
        list_bound(rows.len(), 0, 64, "config_plan.policy_changes")?;
        for row in rows {
            match (&row.module_id, row.kind) {
                (None, ConfigPolicyKind::SubcJsonc) => {},
                (Some(id), ConfigPolicyKind::Declaration | ConfigPolicyKind::ReviewedProgram | ConfigPolicyKind::LayerFile) => bounded(id, 128, "config_plan.policy_changes.module_id")?,
                _ => return Err(Refusal::invalid("config_plan.policy_changes.module_id is required except for subc_jsonc, where it must be absent")),
            }
            json_bound(&row.old, "config_plan.policy_changes.old")?;
            json_bound(&row.new, "config_plan.policy_changes.new")?;
            if row.kind == ConfigPolicyKind::LayerFile {
                let layer = row.layer.as_ref().ok_or_else(|| {
                    Refusal::invalid("config_plan.policy_changes.layer is required for layer_file")
                })?;
                bounded(layer, 64, "config_plan.policy_changes.layer")?;
                if row.reason.is_some() {
                    return Err(Refusal::invalid(
                        "config_plan.policy_changes.reason must be absent for layer_file",
                    ));
                }
                layer_file_value(&row.old, "config_plan.policy_changes.old")?;
                layer_file_value(&row.new, "config_plan.policy_changes.new")?;
            } else if row.layer.is_some() {
                return Err(Refusal::invalid(
                    "config_plan.policy_changes.layer is only allowed for layer_file",
                ));
            }
            if row.reason.as_ref().is_some_and(|reason| reason.len() > 512) {
                return Err(Refusal::invalid(
                    "config_plan.policy_changes.reason exceeds 512 bytes",
                ));
            }
        }
    }
    if let Some(manifests) = &plan.launch_manifests {
        list_bound(manifests.len(), 0, 64, "config_plan.launch_manifests")?;
        unique(
            manifests.iter().map(|row| row.module_id.as_str()),
            "config_plan.launch_manifests.module_id",
        )?;
        for manifest in manifests {
            if let Some(origin) = &manifest.origin {
                bounded(origin, 64, "config_plan.launch_manifests.origin")?;
            }
            list_bound(
                manifest.programs.len(),
                0,
                16,
                "config_plan.launch_manifests.programs",
            )?;
            if manifest.programs.is_empty()
                && manifest.program_changed_since_target != Some(true)
                && manifest.program_changed_since_active != Some(true)
            {
                return Err(Refusal::invalid("config_plan.launch_manifests entry needs at least one program or program-change flag"));
            }
            for row in &manifest.programs {
                bounded(
                    &row.input,
                    1024,
                    "config_plan.launch_manifests.programs.input",
                )?;
                json_bound(&row.old, "config_plan.launch_manifests.programs.old")?;
                json_bound(&row.new, "config_plan.launch_manifests.programs.new")?;
            }
        }
    }
    if plan.changes.is_empty()
        && plan.policy_changes.as_ref().is_none_or(Vec::is_empty)
        && plan.launch_manifests.as_ref().is_none_or(Vec::is_empty)
    {
        return Err(Refusal::invalid("config_plan must contain at least one row across changes, policy_changes and launch_manifests"));
    }
    Ok(())
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
    let plan = &body["config_plan"];
    if !plan.get("apply").is_some_and(Value::is_object) {
        return Err(Refusal::invalid("config_plan.apply must be an object"));
    }
    for field in ["rollback_to", "review"] {
        if plan["apply"].get(field).is_none() {
            return Err(Refusal::invalid(format!(
                "config_plan.apply.{field} is required"
            )));
        }
    }
    typed_field::<ConfigOperation>(&plan["apply"]["operation"], "config_plan.apply.operation")?;
    typed_field::<Option<String>>(
        &plan["apply"]["rollback_to"],
        "config_plan.apply.rollback_to",
    )?;
    if plan["apply"]["review"].is_object() {
        typed_field::<bool>(
            &plan["apply"]["review"]["supervisor_reconcile"],
            "config_plan.apply.review.supervisor_reconcile",
        )?;
    }
    typed_field::<Option<ConfigReview>>(&plan["apply"]["review"], "config_plan.apply.review")?;
    if plan["apply"]
        .get("rollback_programs")
        .is_some_and(|v| !v.is_array())
    {
        return Err(Refusal::invalid(
            "config_plan.apply.rollback_programs must be an array",
        ));
    }
    for field in ["affected_projects", "policy_changes", "launch_manifests"] {
        if plan.get(field).is_some_and(|v| !v.is_array()) {
            return Err(Refusal::invalid(format!(
                "config_plan.{field} must be an array"
            )));
        }
    }
    if let Some(rows) = plan["policy_changes"].as_array() {
        for row in rows {
            typed_field::<ConfigPolicyKind>(&row["kind"], "config_plan.policy_changes.kind")?;
            for field in ["module_id", "layer", "reason"] {
                if row.get(field).is_some_and(|v| !v.is_string()) {
                    return Err(Refusal::invalid(format!(
                        "config_plan.policy_changes.{field} must be a string"
                    )));
                }
            }
        }
    }
    if let Some(rows) = plan["launch_manifests"].as_array() {
        for row in rows {
            if row.get("origin").is_some_and(|v| !v.is_string()) {
                return Err(Refusal::invalid(
                    "config_plan.launch_manifests.origin must be a string",
                ));
            }
            for flag in [
                "program_changed_since_target",
                "program_changed_since_active",
            ] {
                if row.get(flag).is_some_and(|v| v != &Value::Bool(true)) {
                    return Err(Refusal::invalid(format!(
                        "config_plan.launch_manifests.{flag} must be true when present"
                    )));
                }
            }
        }
    }
    if let Some(rows) = plan["apply"]["modules"].as_array() {
        for row in rows {
            typed_field::<ConfigTransition>(
                &row["transition"],
                "config_plan.apply.modules.transition",
            )?;
            for field in [
                "whole_generation",
                "waits_for_next_start",
                "waits_for_daemon_restart",
            ] {
                if !row.get(field).is_some_and(Value::is_boolean) {
                    return Err(Refusal::invalid(format!(
                        "config_plan.apply.modules.{field} must be a boolean"
                    )));
                }
            }
        }
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
    list_bound(plan.changes.len(), 0, 64, "config_plan.changes")?;
    validate_sections(plan)?;
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
