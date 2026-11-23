use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::Refusal;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SandboxLevel {
    Credentials,
    Standard,
    Strict,
}

impl SandboxLevel {
    fn label(self) -> &'static str {
        match self {
            Self::Credentials => "Credentials",
            Self::Standard => "Standard",
            Self::Strict => "Strict",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SandboxEscape {
    pub call_key: String,
    pub command: String,
    pub cwd: String,
    pub shell: String,
    pub escapes_level: SandboxLevel,
    pub background: bool,
    pub reason: String,
    pub expires_in_ms: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Filing {
    #[serde(rename = "kind")]
    _kind: String,
    sandbox_escape: SandboxEscape,
}

fn invalid(field: &str) -> Refusal {
    Refusal::invalid(format!("invalid sandbox_escape {field}"))
}

fn bidi(c: char) -> bool {
    matches!(c, '\u{061c}' | '\u{200e}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

impl SandboxEscape {
    pub(super) fn validate(&self) -> Result<(), Refusal> {
        if self.call_key.trim().is_empty() {
            return Err(invalid("call_key"));
        }
        if self.command.trim().is_empty() || self.command.chars().count() > 2_000 {
            return Err(invalid("command length"));
        }
        // Do not let nonprinting controls rearrange the command the person sees.
        if self
            .command
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            return Err(invalid("command control character"));
        }
        if self.command.chars().any(bidi) {
            return Err(invalid("command text direction"));
        }
        for (field, path) in [("cwd", &self.cwd), ("shell", &self.shell)] {
            if !path.starts_with('/') || path.contains('\0') {
                return Err(invalid(field));
            }
        }
        if self.reason.chars().count() > 160
            || self.reason.chars().any(|c| {
                matches!(
                    c,
                    '\n' | '\r' | '\x0b' | '\x0c' | '\u{0085}' | '\u{2028}' | '\u{2029}'
                )
            })
        {
            return Err(invalid("reason"));
        }
        if !(1..=600_000).contains(&self.expires_in_ms) {
            return Err(invalid("expires_in_ms"));
        }
        Ok(())
    }
}

pub(super) fn normalize(body: Value) -> Result<Value, Refusal> {
    let Filing { sandbox_escape, .. } =
        serde_json::from_value(body).map_err(|_| invalid("fields"))?;
    Ok(json!({
        "kind": "sandbox_escape",
        "title": "Run one command outside the sandbox?",
        "prompt": format!("Runs outside the {} sandbox, with full access to your logins and files.", sandbox_escape.escapes_level.label()),
        "options": [
            {"id": "run_once", "label": "Run once", "effect": "grant"},
            {"id": "decline", "label": "Don't run", "effect": "decline"}
        ],
        "default": "decline", "on_expiry": "deny", "urgency": "normal", "material_damage": false,
        "late_execution": "notify_only", "scope": {}, "target": {"kind": "sandbox_escape", "label": "Shell command"},
        "sandbox_escape": sandbox_escape
    }))
}
