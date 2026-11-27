//! Vault approvals bound to one installed flow version, not to an agent scope.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{
    lowercase_hex, secret_scope,
    secret_use::{
        parse_statement, present_string, statement_version_ok, StatementRule, StatementValidation,
    },
    Refusal, SecretUse, SecretUseAnswer,
};

const USE_V2: &str = "ck-secret-use-statement/v2";
const FLOW_V2: &str = "ck-secret-flow-statement/v2";
const USE_V3: &str = "ck-secret-use-statement/v3";
const FLOW_V3: &str = "ck-secret-flow-statement/v3";

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
    .reason("statement_invalid")
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FlowStatement {
    v: String,
    request_id: String,
    member_id: String,
    /// The paired computer the flow runs on; present exactly for forwarded
    /// requests (v2, or forwarded v3).
    #[serde(default, deserialize_with = "present_string")]
    asking_member_id: Option<String>,
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
        // Signed legacy statements always emit this key, using "" for no prefix.
        path_prefix: String,
        location: String,
        methods: Vec<String>,
    },
    Login {
        origin: String,
    },
    ExtendedApiKey {
        rules: Vec<StatementRule>,
        location: String,
        methods: Vec<String>,
    },
    ExtendedLogin {
        origins: Vec<String>,
    },
}

impl FlowUse {
    fn shape_ok(&self, v3: bool, kind: &str) -> bool {
        match self {
            Self::ApiKey { .. } | Self::Login { .. } => !v3,
            Self::ExtendedApiKey { .. } => v3 && kind == "agent_api_key",
            Self::ExtendedLogin { .. } => v3 && kind == "site_login",
        }
    }

    fn validate_scope(&self) -> Result<(), &'static str> {
        match self {
            Self::ApiKey {
                host,
                path_prefix,
                location,
                ..
            } => secret_scope::flat_api_key(host, path_prefix, location),
            Self::ExtendedApiKey {
                rules, location, ..
            } => secret_scope::extended_api_key(rules, location),
            Self::ExtendedLogin { origins } => secret_scope::origins(origins),
            Self::Login { .. } => Ok(()),
        }
    }

    fn methods(&self) -> Option<&[String]> {
        match self {
            Self::ApiKey { methods, .. } | Self::ExtendedApiKey { methods, .. } => Some(methods),
            Self::Login { origin } => {
                let _ = origin;
                None
            }
            Self::ExtendedLogin { .. } => None,
        }
    }
}

pub struct FlowInstallSecrets {
    pub identity: FlowIdentity,
    pub approvals: SecretUse,
}

impl FlowInstallSecrets {
    pub fn decode(body: &Map<String, Value>) -> Result<Option<Self>, Refusal> {
        Self::decode_with(body, StatementValidation::Filing)
    }

    pub(super) fn decode_with(
        body: &Map<String, Value>,
        validation: StatementValidation,
    ) -> Result<Option<Self>, Refusal> {
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
            items: serde_json::from_value(secrets.clone())
                .map_err(|_| invalid().reason("request_shape"))?,
        };
        let result = Self {
            identity,
            approvals,
        };
        result.expiries(validation)?;
        Ok(Some(result))
    }

    fn expiries(&self, validation: StatementValidation) -> Result<Vec<i64>, Refusal> {
        if !(1..=16).contains(&self.approvals.items.len()) {
            return Err(invalid().reason("request_shape"));
        }
        let mut seen = HashSet::new();
        // The asking computer of the first statement; every later statement
        // must name the same one, or all must omit it.
        let mut asking: Option<Option<String>> = None;
        // One home vault holds the account's flow grants and agent secrets.
        let mut member: Option<String> = None;
        self.approvals
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let refusal = |problem: &str| match validation {
                    StatementValidation::Filing => Refusal::new(
                        "elicitation_invalid_flow_secrets",
                        format!("flow_install.secrets[{index}] statement {problem}"),
                    )
                    .reason("statement_invalid"),
                    StatementValidation::Retained => invalid(),
                };
                let (statement, _): (FlowStatement, _) =
                    parse_statement(&item.statement_b64url, invalid, refusal)?;
                if !statement_version_ok(
                    &statement.v,
                    "ck-secret-flow-statement",
                    statement.asking_member_id.as_deref(),
                    &statement.member_id,
                ) || !statement
                    .r#use
                    .shape_ok(statement.v == FLOW_V3, &statement.secret.kind)
                    // Extended scopes reserve the v3 tags even locally; every
                    // forwarded request also reserves the legacy v2 tags.
                    || (statement.v == FLOW_V3
                        && matches!(statement.flow.flow_id.as_str(), USE_V3 | FLOW_V3))
                    || (statement.asking_member_id.is_some()
                        && matches!(statement.flow.flow_id.as_str(), USE_V2 | FLOW_V2))
                {
                    return Err(invalid());
                }
                if *member.get_or_insert_with(|| statement.member_id.clone()) != statement.member_id
                {
                    return Err(refusal("member_id must match the first statement"));
                }
                if *asking.get_or_insert_with(|| statement.asking_member_id.clone())
                    != statement.asking_member_id
                {
                    return Err(refusal(
                        "asking_member_id must match the first statement, including when absent",
                    ));
                }
                if !super::secret_use::ulid(&item.request_id)
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
                if let Some(methods) = statement.r#use.methods() {
                    secret_scope::methods(methods)
                        .map_err(|problem| refusal(problem).reason("methods_invalid"))?;
                }
                if validation == StatementValidation::Filing {
                    statement.r#use.validate_scope().map_err(refusal)?;
                }
                i64::try_from(statement.request_expires_at_ms)
                    .map_err(|_| invalid().reason("expiry_invalid"))
            })
            .collect()
    }

    pub fn expires_at_ms(&self) -> Result<i64, Refusal> {
        self.expiries(StatementValidation::Filing)?
            .into_iter()
            .min()
            .ok_or_else(invalid)
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
