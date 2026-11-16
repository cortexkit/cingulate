//! Vault-built manifest statements. The vault verifies phone signatures;
//! Cingulate preserves the statement and records the phone's decision binding.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{lowercase_hex, secret_use::canonical, Refusal};

fn invalid(field: &str) -> Refusal {
    Refusal::invalid(format!("invalid manifest_binding.{field}")).reason("statement_invalid")
}

fn invalid_envelope(field: &str) -> Refusal {
    invalid(field).reason("envelope_invalid")
}

/// Read-only card facts derived from the exact canonical statement bytes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ManifestBinding {
    pub statement_b64url: String,
    pub statement_sha256: String,
    pub agent_id: String,
    pub app_slug: String,
    pub repo: String,
    pub installation_permissions: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestBindingStatement {
    pub v: String,
    pub proposal_id: String,
    pub agent_id: String,
    pub app_slug: String,
    pub repo: String,
    pub installation_permissions: BTreeMap<String, String>,
    pub manifest_version: u64,
    pub manifest_sha256: String,
    pub prior_version: u64,
    pub prior_sha256: String,
    pub expires_at_ms: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    v: String,
    proposal_id: String,
    statement_sha256: String,
    decision: String,
    device_fingerprint: String,
    issued_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManifestBindingAnswer {
    pub decision: String,
    pub statement_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub envelope_sha256: Option<String>,
}

fn statement(encoded: &str) -> Result<(ManifestBindingStatement, Vec<u8>), Refusal> {
    let (value, bytes): (Value, _) = canonical(encoded).map_err(|_| invalid("statement_b64url"))?;
    let fields = [
        "v",
        "proposal_id",
        "agent_id",
        "app_slug",
        "repo",
        "installation_permissions",
        "manifest_version",
        "manifest_sha256",
        "prior_version",
        "prior_sha256",
        "expires_at_ms",
    ];
    let object = value
        .as_object()
        .ok_or_else(|| invalid("statement_b64url"))?;
    for field in fields {
        let field_value = object.get(field).ok_or_else(|| invalid(field))?;
        let valid = match field {
            "installation_permissions" => field_value.as_object().is_some_and(|permissions| {
                permissions.values().all(|permission| {
                    matches!(permission.as_str(), Some("read" | "write" | "admin"))
                })
            }),
            "manifest_version" | "prior_version" => field_value.as_u64().is_some(),
            "expires_at_ms" => field_value.as_i64().is_some(),
            _ => field_value.as_str().is_some(),
        };
        if !valid {
            return Err(invalid(field));
        }
    }
    if let Some(extra) = object.keys().find(|key| !fields.contains(&key.as_str())) {
        return Err(invalid(extra));
    }
    let statement: ManifestBindingStatement =
        serde_json::from_value(value).map_err(|_| invalid("statement_b64url"))?;
    if statement.v != "ck-manifest-binding-statement/v1" {
        return Err(invalid("v"));
    }
    if !crate::secret_use::ulid(&statement.proposal_id) {
        return Err(invalid("proposal_id"));
    }
    if !(1..=128).contains(&statement.agent_id.len())
        || !statement
            .agent_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err(invalid("agent_id"));
    }
    if !(1..=100).contains(&statement.app_slug.len())
        || !statement
            .app_slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(invalid("app_slug"));
    }
    let parts: Vec<_> = statement.repo.split('/').collect();
    if parts.len() != 2
        || parts.iter().any(|part| {
            !(1..=100).contains(&part.len())
                || matches!(*part, "." | "..")
                || !part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
    {
        return Err(invalid("repo"));
    }
    if statement.prior_version.checked_add(1) != Some(statement.manifest_version) {
        return Err(invalid("manifest_version"));
    }
    for (field, hash) in [
        ("manifest_sha256", &statement.manifest_sha256),
        ("prior_sha256", &statement.prior_sha256),
    ] {
        if !lowercase_hex(hash, 64) {
            return Err(invalid(field));
        }
    }
    Ok((statement, bytes))
}

impl ManifestBinding {
    pub fn from_statement(statement_b64url: String) -> Result<Self, Refusal> {
        let (statement, bytes) = statement(&statement_b64url)?;
        Ok(Self {
            statement_b64url,
            statement_sha256: format!("{:x}", Sha256::digest(bytes)),
            agent_id: statement.agent_id,
            app_slug: statement.app_slug,
            repo: statement.repo,
            installation_permissions: statement.installation_permissions,
        })
    }

    pub fn statement(&self) -> Result<ManifestBindingStatement, Refusal> {
        // Stored facts must remain a projection of the bytes the phone signs.
        if Self::from_statement(self.statement_b64url.clone())? != *self {
            return Err(invalid("statement_sha256"));
        }
        Ok(statement(&self.statement_b64url)?.0)
    }

    pub fn expires_at_ms(&self) -> Result<i64, Refusal> {
        Ok(self.statement()?.expires_at_ms)
    }

    pub fn answer(
        &self,
        choice: &str,
        device: Option<&str>,
        encoded: Option<&str>,
    ) -> Result<ManifestBindingAnswer, Refusal> {
        let statement = self.statement()?;
        let envelope_sha256 = match (choice, encoded) {
            ("decline", None) => None,
            ("allow", Some(encoded)) => {
                let (envelope, bytes): (Envelope, _) =
                    canonical(encoded).map_err(|_| invalid_envelope("envelope_b64url"))?;
                if envelope.v != "ck-manifest-binding-envelope/v1" {
                    return Err(invalid_envelope("v"));
                }
                if envelope.proposal_id != statement.proposal_id {
                    return Err(invalid_envelope("proposal_id"));
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
                // As for secret_use, the vault owns signature and issued-at checks.
                let _ = envelope.issued_at_ms;
                Some(format!("{:x}", Sha256::digest(bytes)))
            }
            _ => return Err(invalid_envelope("decision")),
        };
        Ok(ManifestBindingAnswer {
            decision: choice.into(),
            statement_sha256: self.statement_sha256.clone(),
            envelope_sha256,
        })
    }
}

pub(super) fn normalize(body: Value) -> Result<Value, Refusal> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Filing {
        kind: String,
        manifest_binding: RawStatement,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct RawStatement {
        statement_b64url: String,
    }
    let filing: Filing = serde_json::from_value(body)
        .map_err(|_| Refusal::invalid("manifest_binding filing has invalid fields"))?;
    let binding = ManifestBinding::from_statement(filing.manifest_binding.statement_b64url)?;
    Ok(json!({
        "kind": filing.kind, "manifest_binding": binding,
        "title": "Repository binding approval", "prompt": "Review the vault's repository binding.",
        "options": [{"id":"allow", "label":"Allow", "effect":"grant"}, {"id":"decline", "label":"Decline", "effect":"decline"}],
        "default":"decline", "on_expiry":"deny", "urgency":"normal", "material_damage":false,
        "late_execution":"notify_only", "scope":{}, "target":{"kind":"manifest_binding", "label":"Repository binding"}
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

    fn sample(name: &str) -> Value {
        let samples: Value = serde_json::from_slice(include_bytes!(
            "../../../test-vectors/manifest-binding-v1/vault-samples.json"
        ))
        .unwrap();
        samples["samples"][name].clone()
    }

    fn encoded(value: &Value) -> String {
        URL_SAFE_NO_PAD.encode(serde_jcs::to_vec(value).unwrap())
    }

    fn defect(field: &str, value: Value) {
        let mut statement = sample("statement")["statement"].clone();
        statement[field] = value;
        let refusal = ManifestBinding::from_statement(encoded(&statement)).unwrap_err();
        assert_eq!(refusal.code, "elicitation_invalid_request");
        assert!(refusal.message.contains(field), "{refusal}");
    }

    #[test]
    fn noncanonical_statement_is_refused() {
        let statement = sample("statement")["statement"].clone();
        let text = serde_json::to_string_pretty(&statement).unwrap();
        assert!(
            ManifestBinding::from_statement(URL_SAFE_NO_PAD.encode(text))
                .unwrap_err()
                .message
                .contains("statement_b64url")
        );
    }

    #[test]
    fn statement_extra_field_is_refused() {
        defect("extra", json!(false));
    }

    #[test]
    fn statement_missing_field_is_refused() {
        let original = sample("statement")["statement"].clone();
        for field in original.as_object().unwrap().keys() {
            let mut statement = original.clone();
            statement.as_object_mut().unwrap().remove(field);
            assert!(ManifestBinding::from_statement(encoded(&statement))
                .unwrap_err()
                .message
                .contains(field));
        }
    }

    #[test]
    fn statement_wrong_version_domain_is_refused() {
        defect("v", json!("ck-secret-use-statement/v1"));
    }

    #[test]
    fn statement_bad_permission_is_refused() {
        defect("installation_permissions", json!({"contents":"owner"}));
    }

    #[test]
    fn statement_non_successor_version_is_refused() {
        defect("manifest_version", json!(30));
    }

    #[test]
    fn statement_uppercase_digests_are_refused() {
        for field in ["manifest_sha256", "prior_sha256"] {
            defect(field, json!("A".repeat(64)));
        }
    }

    #[test]
    fn statement_identity_and_single_repo_grammars_match_the_vault() {
        defect("agent_id", json!(""));
        defect("app_slug", json!("Not A Slug"));
        for repo in ["owner/repo/other", "owner/a,other/b", "owner/..", "owner/"] {
            defect("repo", json!(repo));
        }
    }

    #[test]
    fn statement_bytes_and_digest_are_preserved() {
        let source = sample("statement");
        let binding =
            ManifestBinding::from_statement(source["statement_b64url"].as_str().unwrap().into())
                .unwrap();
        assert_eq!(binding.statement_sha256, source["statement_sha256"]);
        assert_eq!(
            URL_SAFE_NO_PAD.decode(binding.statement_b64url).unwrap(),
            source["statement_utf8"].as_str().unwrap().as_bytes()
        );
    }

    #[test]
    fn answer_binds_the_exact_vault_envelope_and_decline_has_no_envelope() {
        let binding = ManifestBinding::from_statement(
            sample("statement")["statement_b64url"]
                .as_str()
                .unwrap()
                .into(),
        )
        .unwrap();
        let envelope = sample("envelope");
        let device = envelope["envelope"]["device_fingerprint"].as_str().unwrap();
        let encoded = envelope["envelope_b64url"].as_str().unwrap();
        let answer = binding
            .answer("allow", Some(device), Some(encoded))
            .unwrap();
        assert_eq!(
            answer.envelope_sha256,
            Some(format!(
                "{:x}",
                Sha256::digest(envelope["envelope_utf8"].as_str().unwrap().as_bytes())
            ))
        );
        assert!(binding.answer("allow", None, Some(encoded)).is_err());
        assert!(binding
            .answer("decline", Some(device), Some(encoded))
            .is_err());
        let decline = binding.answer("decline", Some(device), None).unwrap();
        assert_eq!(decline.statement_sha256, binding.statement_sha256);
        assert_eq!(decline.envelope_sha256, None);
    }
}
