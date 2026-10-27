//! Vault approvals bound to one installed flow version, not to an agent scope.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{lowercase_hex, secret_use::canonical, Refusal, SecretUse, SecretUseAnswer};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FlowIdentity {
    pub flow_id: String,
    pub version: u64,
    pub code_hash: String,
    pub install_id: String,
}

impl FlowIdentity {
    fn validate(&self) -> Result<(), Refusal> {
        let printable =
            |s: &str| (1..=256).contains(&s.len()) && s.bytes().all(|b| (0x20..=0x7e).contains(&b));
        if !printable(&self.flow_id)
            || !printable(&self.install_id)
            || !(1..=(1 << 53) - 1).contains(&self.version)
            || !lowercase_hex(&self.code_hash, 64)
        {
            return Err(invalid_identity());
        }
        Ok(())
    }
}

fn invalid_identity() -> Refusal {
    Refusal::new("elicitation_invalid_flow_identity", "invalid flow_identity")
}

fn invalid() -> Refusal {
    Refusal::new(
        "elicitation_invalid_flow_secrets",
        "invalid flow secret statements",
    )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FlowStatement {
    v: String,
    request_id: String,
    member_id: String,
    flow: FlowIdentity,
    secret: crate::secret_use::StatementSecret,
    r#use: FlowUse,
    durations: Vec<String>,
    request_expires_at_ms: u64,
}

#[derive(Debug, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum FlowUse {
    ApiKey {
        host: String,
        path_prefix: String,
        location: String,
        methods: Vec<String>,
    },
    Login {
        origin: String,
    },
}

pub struct FlowInstallSecrets {
    pub identity: FlowIdentity,
    pub approvals: SecretUse,
}

impl FlowInstallSecrets {
    pub fn decode(body: &Map<String, Value>) -> Result<Option<Self>, Refusal> {
        let (identity, secrets) = match (body.get("flow_identity"), body.get("secrets")) {
            (None, None) => return Ok(None),
            (Some(identity), Some(secrets)) => (identity, secrets),
            _ => {
                return Err(Refusal::new(
                    "elicitation_flow_secrets_pair_required",
                    "flow_identity and secrets must be supplied together",
                ))
            }
        };
        let identity: FlowIdentity =
            serde_json::from_value(identity.clone()).map_err(|_| invalid_identity())?;
        identity.validate()?;
        let approvals = SecretUse {
            items: serde_json::from_value(secrets.clone()).map_err(|_| invalid())?,
        };
        let result = Self {
            identity,
            approvals,
        };
        result.expiries()?;
        Ok(Some(result))
    }

    fn expiries(&self) -> Result<Vec<i64>, Refusal> {
        if !(1..=16).contains(&self.approvals.items.len()) {
            return Err(invalid());
        }
        let mut seen = HashSet::new();
        self.approvals
            .items
            .iter()
            .map(|item| {
                let (statement, _): (FlowStatement, _) =
                    canonical(&item.statement_b64url).map_err(|_| invalid())?;
                if statement.v != "ck-secret-flow-statement/v1"
                    || !super::secret_use::ulid(&item.request_id)
                    || statement.request_id != item.request_id
                {
                    return Err(invalid());
                }
                if !seen.insert(&item.request_id) {
                    return Err(Refusal::new(
                        "elicitation_duplicate_request_id",
                        "duplicate secret request id",
                    ));
                }
                if statement.flow != self.identity {
                    return Err(Refusal::new(
                        "elicitation_flow_identity_mismatch",
                        "statement flow must equal flow_identity",
                    ));
                }
                if statement.durations != ["version"] {
                    return Err(Refusal::new(
                        "elicitation_invalid_flow_durations",
                        "flow durations must be exactly version",
                    ));
                }
                // The vault owns secret scope semantics. Decode the closed shape here
                // without rewriting any of the bytes the phone will display and sign.
                let _ = (&statement.member_id, &statement.secret);
                match &statement.r#use {
                    FlowUse::ApiKey {
                        host,
                        path_prefix,
                        location,
                        methods,
                    } => {
                        let _ = (host, path_prefix, location);
                        if methods.is_empty()
                            || methods.windows(2).any(|pair| pair[0] >= pair[1])
                            || methods.iter().any(|m| {
                                !(1..=16).contains(&m.len())
                                    || !m.bytes().all(|b| b.is_ascii_uppercase())
                            })
                        {
                            return Err(invalid());
                        }
                    }
                    FlowUse::Login { origin } => {
                        let _ = origin;
                    }
                }
                i64::try_from(statement.request_expires_at_ms).map_err(|_| invalid())
            })
            .collect()
    }

    pub fn expires_at_ms(&self) -> Result<i64, Refusal> {
        self.expiries()?.into_iter().min().ok_or_else(invalid)
    }

    pub fn answer(
        &self,
        choice: &str,
        device: Option<&str>,
        encoded: Option<&str>,
    ) -> Result<SecretUseAnswer, Refusal> {
        self.approvals.answer_items(
            choice,
            device,
            encoded,
            &vec![vec!["version".into()]; self.approvals.items.len()],
            true,
        )
    }
}
