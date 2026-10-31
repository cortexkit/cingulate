#![forbid(unsafe_code)]
//! Shared, deterministic presence summaries and routing. No clock, storage or I/O
//! is accessed here: callers supply Unix epoch milliseconds and route-stamped devices.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// The class of a route-identified reporting device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    Phone,
    Computer,
}

/// A reporter's current device/app activity, not an authorization grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Active,
    Background,
    Idle,
    Locked,
}

/// A validated report stamped with the receiver's clock. Focus and inline kinds
/// are meaningful only while active; expiry is exclusive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub state: State,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub focus: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shows_inline: Vec<String>,
    pub reported_at_ms: i64,
    pub expires_at_ms: i64,
}

/// The most recent report for one device id. The service admits the reporter
/// and picks its device kind from the daemon's route; only a local app without
/// a verified stamp names itself, and only while the development switch is on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    pub device_id: String,
    pub device_kind: DeviceKind,
    #[serde(flatten)]
    pub report: Report,
}

/// Where fresh reports place the person. Active phone takes precedence over a
/// computer; idle/locked reports alone indicate away, and no reports mean unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Where {
    Phone,
    Computer,
    Away,
    Unknown,
}

/// An agent chat drawn by active apps, their union of inline item kinds, and
/// the newest receiver-stamped report time among those apps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Watching {
    pub agent_id: String,
    pub shows_inline: Vec<String>,
    pub seen_at_ms: i64,
}

/// Routing inputs projected from unexpired reports. Watching entries and their
/// inline kinds are sorted and de-duplicated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    #[serde(rename = "where")]
    pub location: Where,
    pub watching: Vec<Watching>,
}

impl Summary {
    /// Whether two summaries draw the same attention: true when location,
    /// watched agents and their inline kinds match, so a watch needs no new
    /// frame. Refresh timestamps alone don't count, but every emitted summary
    /// still carries them.
    pub fn same_attention(&self, other: &Self) -> bool {
        self.location == other.location
            && self.watching.len() == other.watching.len()
            && self
                .watching
                .iter()
                .zip(&other.watching)
                .all(|(a, b)| a.agent_id == b.agent_id && a.shows_inline == b.shows_inline)
    }
}

/// Summarize fresh devices at `now_ms` (Unix epoch milliseconds). Expired reports
/// and non-active focus are ignored, even if supplied by an external caller.
pub fn summarize<'a>(reports: impl IntoIterator<Item = &'a Device>, now_ms: i64) -> Summary {
    let mut phone = false;
    let mut computer = false;
    let mut fresh = false;
    let mut watching: BTreeMap<String, (BTreeSet<String>, i64)> = BTreeMap::new();
    for device in reports {
        let report = &device.report;
        if report.expires_at_ms <= now_ms {
            continue;
        }
        fresh = true;
        phone |= device.device_kind == DeviceKind::Phone && report.state == State::Active;
        computer |= device.device_kind == DeviceKind::Computer
            && matches!(report.state, State::Active | State::Background);
        if report.state == State::Active {
            for agent in &report.focus {
                let entry = watching
                    .entry(agent.clone())
                    .or_insert_with(|| (BTreeSet::new(), report.reported_at_ms));
                entry.0.extend(report.shows_inline.iter().cloned());
                entry.1 = entry.1.max(report.reported_at_ms);
            }
        }
    }
    Summary {
        location: if phone {
            Where::Phone
        } else if computer {
            Where::Computer
        } else if fresh {
            Where::Away
        } else {
            Where::Unknown
        },
        watching: watching
            .into_iter()
            .map(|(agent_id, (kinds, seen_at_ms))| Watching {
                agent_id,
                shows_inline: kinds.into_iter().collect(),
                seen_at_ms,
            })
            .collect(),
    }
}

/// Where to deliver an out-of-chat ask or notice. None means a recently watched
/// chat already draws that item kind inline; it is not permission to act.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Deliver {
    Desktop,
    Phone,
    None,
}

/// Decide delivery for `kind` and `agent_id` using a summary and Unix epoch
/// milliseconds. Inline suppression lasts at most 60 seconds since the newest
/// active report; otherwise computer presence routes to desktop and all other
/// locations (including unknown) route to phone.
pub fn route(summary: &Summary, kind: &str, agent_id: &str, now_ms: i64) -> Deliver {
    if summary.watching.iter().any(|entry| {
        entry.agent_id == agent_id
            && entry.shows_inline.iter().any(|value| value == kind)
            && now_ms.saturating_sub(entry.seen_at_ms) <= 60_000
    }) {
        Deliver::None
    } else if summary.location == Where::Computer {
        Deliver::Desktop
    } else {
        Deliver::Phone
    }
}

#[cfg(test)]
mod tests;
