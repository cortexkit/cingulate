//! Tool asks have no lifetime or caller-supplied identity. The fixed answer set
//! prevents a requester from offering a grant as the decline default.
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Refusal;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AccessRequest {
    ToolPermission {
        call_key: String,
        tool: String,
        operation: String,
        paths: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        command: Option<String>,
        reason: String,
        args_hash: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        schema_pin: Option<String>,
        pattern: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        head_policy: Option<String>,
    },
    MacosPermission {
        folder: String,
        program: String,
        steps: String,
    },
}

impl AccessRequest {
    pub fn decode(value: Value) -> Result<Self, Refusal> {
        let request: Self = serde_json::from_value(value)
            .map_err(|e| Refusal::new("elicitation_invalid_request", e.to_string()))?;
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), Refusal> {
        let invalid = |message| Refusal::new("elicitation_invalid_request", message);
        match self {
            Self::ToolPermission {
                call_key,
                tool,
                operation,
                paths,
                command,
                reason,
                args_hash,
                schema_pin,
                pattern,
                head_policy,
            } => {
                if tool.chars().count() > 64
                    || [call_key, tool, args_hash, pattern]
                        .iter()
                        .any(|s| s.trim().is_empty())
                    || schema_pin.as_ref().is_some_and(|s| s.trim().is_empty())
                {
                    return Err(invalid(
                        "call_key, tool, args_hash, pattern and any schema_pin must be nonblank",
                    ));
                }
                if !matches!(operation.as_str(), "read" | "write" | "exec") {
                    return Err(invalid("operation must be read, write or exec"));
                }
                if paths.len() > 16
                    || paths
                        .iter()
                        .any(|p| !p.starts_with('/') || p.contains('\0'))
                {
                    return Err(invalid("paths must contain at most 16 absolute paths"));
                }
                if (operation == "exec") != command.is_some()
                    || command
                        .as_ref()
                        .is_some_and(|c| c.trim().is_empty() || c.chars().count() > 600)
                {
                    return Err(invalid(
                        "command is required for exec only and limited to 600 characters",
                    ));
                }
                validate_note(reason, false)?;
                if head_policy
                    .as_ref()
                    .is_some_and(|p| !matches!(p.as_str(), "allow" | "not_held"))
                {
                    return Err(invalid("head_policy must be allow or not_held"));
                }
            }
            Self::MacosPermission {
                folder,
                program,
                steps,
            } => {
                if !folder.starts_with('/')
                    || folder.contains('\0')
                    || program.trim().is_empty()
                    || steps.trim().is_empty()
                {
                    return Err(invalid(
                        "folder must be absolute; program and steps must be nonblank",
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn call_key(&self) -> Option<&str> {
        match self {
            Self::ToolPermission { call_key, .. } => Some(call_key),
            _ => None,
        }
    }
    pub fn head_policy(&self) -> Option<&str> {
        match self {
            Self::ToolPermission { head_policy, .. } => head_policy.as_deref(),
            _ => None,
        }
    }
    pub fn informational(&self) -> bool {
        matches!(self, Self::MacosPermission { .. })
    }
}

pub fn validate_note(note: &str, optional: bool) -> Result<(), Refusal> {
    if (!optional && note.trim().is_empty())
        || note.chars().count() > 160
        || note.contains(['\r', '\n', '\0'])
    {
        Err(Refusal::new(
            "elicitation_invalid_request",
            "reason or note must be one line, at most 160 characters",
        ))
    } else {
        Ok(())
    }
}
