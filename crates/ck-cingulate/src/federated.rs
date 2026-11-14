//! Daemon-attested remote subjects. A remote claim never names a local agent.

use cingulate_decode::{ConsentRequest, Fact, Refusal};
use serde_json::{json, Value};
use subc_protocol::scope::{
    FederatedAttributes, FederatedPeerClass, ParentState, RemoteClaim, ScopeKind, ScopeStamp,
};

use crate::{invalid, Stamp};

pub(crate) fn facts(stamp: &Stamp) -> Result<Option<&FederatedAttributes>, Refusal> {
    let Some(scope) = &stamp.scope else {
        return Ok(None);
    };
    if scope.kind != ScopeKind::Federated {
        if scope.attributes.federated.is_some() {
            return Err(stamp.refuse());
        }
        return Ok(None);
    }
    if scope
        .parent_state
        .is_some_and(|state| state != ParentState::Linked)
    {
        return Err(Refusal::new(
            "elicitation_federated_unlinked",
            "federated scope parent is not linked",
        ));
    }
    let facts = scope
        .attributes
        .federated
        .as_ref()
        .ok_or_else(|| stamp.refuse())?;
    facts.validate().map_err(invalid)?;
    if facts.remote_claim.is_some() && facts.peer_class != FederatedPeerClass::Computer {
        return Err(stamp.refuse());
    }
    // Reject future claim variants until their identity and display rules exist.
    claim_identity(facts.remote_claim.as_ref())?;
    Ok(Some(facts))
}

/// Display-only attribution is deliberately absent from this canonical identity.
pub fn claim_identity(claim: Option<&RemoteClaim>) -> Result<Value, Refusal> {
    Ok(match claim {
        None => Value::Null,
        Some(RemoteClaim::Agent { agent_id, .. }) => json!(["agent", agent_id]),
        Some(RemoteClaim::Run { agent_id, .. }) => json!(["run", agent_id]),
        Some(RemoteClaim::Worker {
            owner_module_id,
            worker_ref,
            ..
        }) => {
            json!(["worker", owner_module_id, worker_ref])
        }
        Some(RemoteClaim::Flow {
            flow_id,
            flow_version,
            flow_install_id,
            flow_code_hash,
            ..
        }) => {
            json!([
                "flow",
                flow_id,
                flow_version,
                flow_install_id,
                flow_code_hash
            ])
        }
        Some(_) => return Err(invalid("unsupported remote claim").reason("remote_claim_invalid")),
    })
}

pub(crate) fn session(facts: &FederatedAttributes) -> Result<String, Refusal> {
    Ok(json!([
        "federated",
        facts.peer_fingerprint,
        facts.pairing_id,
        facts.peer_class,
        claim_identity(facts.remote_claim.as_ref())?
    ])
    .to_string())
}

// Keep the display part separate from identity: remote display names must
// not change who can collect a card or which grant a claim selects.
fn display(claim: Option<&RemoteClaim>) -> Result<(String, Option<String>), Refusal> {
    Ok(match claim {
        Some(RemoteClaim::Agent {
            agent_id,
            agent_name,
            ..
        }) => {
            let agent = named_id(agent_id, agent_name.as_deref());
            (format!("agent {agent}"), Some(agent))
        }
        Some(RemoteClaim::Worker {
            on_behalf_of: Some(agent),
            agent_name,
            ..
        }) => {
            let agent = named_id(agent, agent_name.as_deref());
            (
                format!("a worker for agent {agent}"),
                Some(format!("worker for {agent}")),
            )
        }
        Some(RemoteClaim::Worker { .. }) => ("one of its workers".into(), Some("worker".into())),
        Some(RemoteClaim::Flow {
            flow_id,
            flow_version,
            flow_name,
            ..
        }) => match plain_name(flow_name.as_deref()) {
            Some(name) => (
                format!("flow “{name}” ({flow_id}) (version {flow_version})"),
                Some(format!("flow “{name}” ({flow_id}) v{flow_version}")),
            ),
            None => (
                format!("flow “{flow_id}” (version {flow_version})"),
                Some(format!("flow {flow_id} v{flow_version}")),
            ),
        },
        Some(RemoteClaim::Run {
            agent_id,
            agent_name,
            ..
        }) => (
            format!(
                "a run for agent {}",
                named_id(agent_id, agent_name.as_deref())
            ),
            None,
        ),
        None => (String::new(), Some("any program".into())),
        Some(_) => return Err(invalid("unsupported remote claim").reason("remote_claim_invalid")),
    })
}

fn plain_name(name: Option<&str>) -> Option<&str> {
    name.filter(|name| {
        (1..=80).contains(&name.chars().count())
            && !name.chars().any(|ch| {
                ch.is_control()
                    || matches!(ch, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            })
    })
}

fn named_id(id: &str, name: Option<&str>) -> String {
    plain_name(name).map_or_else(|| id.to_owned(), |name| format!("“{name}” ({id})"))
}

/// Recheck names even on typed stamps: programmatically constructed claims can
/// bypass protocol decoding. Invalid display text must not enter stored cards.
pub(crate) fn display_facts(facts: &FederatedAttributes) -> FederatedAttributes {
    let mut facts = facts.clone();
    let name = match facts.remote_claim.as_mut() {
        Some(RemoteClaim::Agent { agent_name, .. })
        | Some(RemoteClaim::Run { agent_name, .. })
        | Some(RemoteClaim::Worker { agent_name, .. }) => Some(agent_name),
        Some(RemoteClaim::Flow { flow_name, .. }) => Some(flow_name),
        _ => None,
    };
    if let Some(name) = name {
        if plain_name(name.as_deref()).is_none() {
            *name = None;
        }
    }
    facts
}

/// The permissions label is display only and never a matching credential.
/// Runs have no standing-grant label.
pub fn holder_label(facts: &FederatedAttributes, selector: &str) -> Option<String> {
    if matches!(facts.remote_claim, Some(RemoteClaim::Run { .. })) {
        return None;
    }
    let suffix = match selector {
        "all_claims" => "everything on it".to_owned(),
        "claimless" if facts.remote_claim.is_none() => "any program".to_owned(),
        "exact_claim" if facts.remote_claim.is_some() => {
            display(facts.remote_claim.as_ref()).ok()?.1?
        }
        _ => return None,
    };
    Some(
        format!("{} · {suffix}", facts.peer_name)
            .chars()
            .take(80)
            .collect(),
    )
}

/// Compare a holder's selector and identity only. Providers must additionally
/// check base-scope liveness and serialize grant application with revocation.
/// Missing/unknown selectors and run claims never select standing authority.
pub fn holder_selects(holder: &Value, scope: Option<&ScopeStamp>) -> bool {
    let Some(scope) = scope.filter(|scope| scope.kind == ScopeKind::Federated) else {
        return false;
    };
    let stamp = Stamp {
        scope: Some(scope.clone()),
        ..Stamp::default()
    };
    let Ok(Some(facts)) = facts(&stamp) else {
        return false;
    };
    if holder["kind"] != "federated"
        || holder["peer_fingerprint"] != facts.peer_fingerprint
        || holder["pairing_id"] != facts.pairing_id
        || holder["peer_class"] != json!(facts.peer_class)
        || matches!(facts.remote_claim, Some(RemoteClaim::Run { .. }))
    {
        return false;
    }
    match holder["selector"].as_str() {
        Some("claimless") => holder.get("claim").is_none() && facts.remote_claim.is_none(),
        Some("all_claims") => holder.get("claim").is_none(),
        Some("exact_claim") => {
            let Some(remote) = facts.remote_claim.as_ref() else {
                return false;
            };
            let Some(claim) = holder
                .get("claim")
                .and_then(|claim| serde_json::from_value::<RemoteClaim>(claim.clone()).ok())
            else {
                return false;
            };
            claim.validate().is_ok()
                && !matches!(claim, RemoteClaim::Run { .. })
                && claim_identity(Some(&claim)).ok() == claim_identity(Some(remote)).ok()
        }
        _ => false,
    }
}

pub(crate) fn prepare(
    facts: &FederatedAttributes,
    request: &mut ConsentRequest,
) -> Result<(), Refusal> {
    if request.kind != "consent" {
        return Err(invalid("federated requesters may raise only consent cards")
            .reason("federated_kind_invalid"));
    }
    let (subject, _) = display(facts.remote_claim.as_ref())?;
    let name = &facts.peer_name;
    let action = request.prompt.trim();
    request.prompt = if facts.remote_claim.is_none() {
        format!("A program on “{name}” (no agent named) wants to {action}")
    } else {
        format!("“{name}” says {subject} wants to {action}")
    };
    request.facts.insert(
        0,
        Fact {
            label: "Paired computer".into(),
            value: format!("“{name}” is paired with this computer."),
        },
    );
    if matches!(facts.remote_claim, Some(RemoteClaim::Run { .. })) {
        request.prompt.push_str(" (this time only)");
        if request
            .options
            .iter()
            .filter(|option| option.effect == "grant")
            .count()
            != 1
        {
            return Err(invalid("a run card requires exactly one grant choice")
                .reason("grant_choices_invalid"));
        }
        request.single_use = Some(true);
        request.duration = None;
        let grant = request
            .options
            .iter_mut()
            .find(|option| option.effect == "grant")
            .expect("one grant choice");
        grant.label = "Allow this time".into();
        grant.detail = None;
    }
    Ok(())
}
