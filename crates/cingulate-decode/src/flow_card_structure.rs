use serde::{Deserialize, Deserializer, Serialize};

use crate::{ConsentRequest, Refusal};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Comparison {
    pub against: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub note: Option<String>,
    pub rows: Vec<ComparisonRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ComparisonRow {
    pub label: String,
    pub change: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub before: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub after: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    pub rows: Vec<CapabilityRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CapabilityRow {
    pub label: String,
    pub value: String,
}

// A present optional member must have its declared type, not JSON null.
pub(crate) fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

impl ConsentRequest {
    pub(crate) fn validate_flow_card_structure(&self) -> Result<(), Refusal> {
        for (field, present) in [
            ("warnings", self.warnings.is_some()),
            ("comparison", self.comparison.is_some()),
            ("capabilities", self.capabilities.is_some()),
        ] {
            if present && !matches!(self.kind.as_str(), "flow_install" | "package") {
                return Err(Refusal::invalid(format!(
                    "{field} is only valid for flow_install or package cards"
                )));
            }
        }
        if let Some(warnings) = &self.warnings {
            count("warnings", warnings.len(), 4)?;
            for warning in warnings {
                text("warnings item", warning, 300, false)?;
            }
        }
        if let Some(comparison) = &self.comparison {
            text("comparison.against", &comparison.against, 120, false)?;
            if let Some(note) = &comparison.note {
                text("comparison.note", note, 1200, true)?;
            }
            count("comparison.rows", comparison.rows.len(), 24)?;
            for row in &comparison.rows {
                text("comparison.rows.label", &row.label, 64, false)?;
                if row.change.is_empty()
                    || row.change.len() > 32
                    || !row
                        .change
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b == b'_')
                {
                    return Err(Refusal::invalid(
                        "comparison.rows.change must be 1 to 32 lowercase ASCII letters or underscores",
                    ));
                }
                if let Some(before) = &row.before {
                    text("comparison.rows.before", before, 1200, true)?;
                }
                if let Some(after) = &row.after {
                    text("comparison.rows.after", after, 1200, true)?;
                }
                let valid = match row.change.as_str() {
                    "changed" => row.after.is_some(),
                    "added" => row.after.is_some() && row.before.is_none(),
                    "removed" => row.before.is_some() && row.after.is_none(),
                    _ => row.before.is_some() || row.after.is_some(),
                };
                if !valid {
                    return Err(Refusal::invalid(
                        "comparison.rows before and after do not match the change rule",
                    ));
                }
            }
        }
        if let Some(capabilities) = &self.capabilities {
            count("capabilities.rows", capabilities.rows.len(), 24)?;
            for row in &capabilities.rows {
                text("capabilities.rows.label", &row.label, 64, false)?;
                text("capabilities.rows.value", &row.value, 1200, true)?;
            }
        }
        Ok(())
    }
}

fn count(field: &str, len: usize, max: usize) -> Result<(), Refusal> {
    if len == 0 || len > max {
        return Err(Refusal::invalid(format!(
            "{field} must contain 1 to {max} entries"
        )));
    }
    Ok(())
}

fn text(field: &str, value: &str, max: usize, newline: bool) -> Result<(), Refusal> {
    if value.is_empty() || value.len() > max {
        return Err(Refusal::invalid(format!(
            "{field} must be nonempty and at most {max} UTF-8 bytes"
        )));
    }
    if value
        .chars()
        .any(|c| c.is_control() && !(newline && c == '\n'))
    {
        return Err(Refusal::invalid(format!(
            "{field} must not contain control characters{}",
            if newline { " except newline" } else { "" }
        )));
    }
    Ok(())
}
