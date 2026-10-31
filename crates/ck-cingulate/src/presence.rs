//! Memory-only presence: nothing touches consent's store or its lock. Presence
//! watches share the module's watch slots with consent watches.

use cingulate_decode::Refusal;
use cingulate_presence::{summarize, Device, DeviceKind, Report, State, Summary};
use serde::Deserialize;
use serde_json::{json, Value};
use subc_client_rs::HandlerOutcome;

use crate::{Handler, Reply, Stamp, WatchSink};

fn invalid(field: &'static str) -> Refusal {
    Refusal::new("invalid_params", json!({"field": field}).to_string())
}

fn code(value: &str, separator: u8) -> bool {
    let bytes = value.as_bytes();
    (1..=32).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == separator)
}

fn list(
    params: &Value,
    field: &'static str,
    min: usize,
    max: usize,
    valid: impl Fn(&str) -> bool,
) -> Result<Vec<String>, Refusal> {
    let Some(value) = params.get(field) else {
        return Ok(vec![]);
    };
    let values = value.as_array().ok_or_else(|| invalid(field))?;
    if !(min..=max).contains(&values.len()) {
        return Err(invalid(field));
    }
    let mut result = Vec::new();
    for value in values {
        let value = value
            .as_str()
            .filter(|value| valid(value))
            .ok_or_else(|| invalid(field))?;
        if result.iter().any(|old| old == value) {
            return Err(invalid(field));
        }
        result.push(value.to_owned());
    }
    Ok(result)
}

impl Stamp {
    fn presence_local(&self) -> bool {
        self.label() != "reserved:callosum" && (self.operator_app.is_some() || self.local_operator)
    }

    fn presence_reader(&self) -> Result<(), Refusal> {
        if self.label() == "reserved:prefrontal-core" || self.presence_local() {
            Ok(())
        } else {
            Err(Refusal::new(
                "presence_read_not_allowed",
                "presence reader is not allowed",
            ))
        }
    }

    fn presence_device(&self, params: &Value) -> Result<(String, DeviceKind), Refusal> {
        if self.label() == "reserved:callosum" {
            if let Some(fingerprint) = self.harness.strip_prefix("fed-phone:") {
                if cingulate_decode::lowercase_hex(fingerprint, 32) {
                    return Ok((format!("phone:{fingerprint}"), DeviceKind::Phone));
                }
            }
        } else if self.presence_local() {
            let app = self
                .operator_app
                .as_deref()
                .or_else(|| params.get("app").and_then(Value::as_str))
                .filter(|app| code(app, b'-'))
                .ok_or_else(|| invalid("app"))?;
            return Ok((format!("computer:{app}"), DeviceKind::Computer));
        }
        Err(Refusal::new(
            "presence_device_not_allowed",
            "presence device is not allowed",
        ))
    }
}

impl Handler {
    pub(crate) fn presence_dispatch(
        &self,
        stamp: &Stamp,
        method: &str,
        params: &Value,
    ) -> Option<Reply> {
        Some(match method {
            "presence.report" => self.presence_report(stamp, params),
            "presence.get" => stamp.presence_reader().map(|()| {
                let (devices, summary, _) = self.presence_snapshot();
                json!({"devices": devices, "where": summary.location, "watching": summary.watching})
            }),
            "presence.watch" => stamp
                .presence_reader()
                .and_then(|()| Err(invalid("stream"))),
            _ => return None,
        })
    }

    fn presence_report(&self, stamp: &Stamp, params: &Value) -> Reply {
        let (device_id, device_kind) = stamp.presence_device(params)?;
        let state: State =
            serde_json::from_value(params.get("state").cloned().unwrap_or(Value::Null))
                .map_err(|_| invalid("state"))?;
        let mut focus = list(params, "focus", 1, 16, |id| {
            subc_protocol::scope::validate_on_behalf_of_agent(id).is_ok()
        })?;
        let mut shows_inline = list(params, "shows_inline", 0, 8, |kind| code(kind, b'_'))?;
        let ttl = params
            .get("ttl_ms")
            .and_then(Value::as_i64)
            .filter(|ttl| (1..=120_000).contains(ttl))
            .ok_or_else(|| invalid("ttl_ms"))?;
        if state != State::Active {
            focus.clear();
            shows_inline.clear();
        }
        let now = self.inner.options.clock.now_ms();
        let expires_at_ms = now.saturating_add(ttl);
        let device = Device {
            device_id: device_id.clone(),
            device_kind,
            report: Report {
                state,
                focus,
                shows_inline,
                reported_at_ms: now,
                expires_at_ms,
            },
        };
        {
            let mut devices = self.inner.presence.lock().expect("presence lock");
            devices.retain(|_, device| device.report.expires_at_ms > now);
            devices.insert(device_id.clone(), device);
        }
        self.inner.presence_changes.send_replace(());
        Ok(json!({"device_id": device_id, "expires_at_ms": expires_at_ms}))
    }

    fn presence_snapshot(&self) -> (Vec<Device>, Summary, Option<i64>) {
        let now = self.inner.options.clock.now_ms();
        let devices: Vec<_> = {
            let mut devices = self.inner.presence.lock().expect("presence lock");
            devices.retain(|_, device| device.report.expires_at_ms > now);
            devices.values().cloned().collect()
        };
        let summary = summarize(&devices, now);
        let expiry = devices
            .iter()
            .map(|device| device.report.expires_at_ms)
            .min();
        (devices, summary, expiry)
    }

    /// Serve a memory-only presence stream through the same sink as consent watches.
    /// No frame or report is written to the consent store or outbox.
    pub async fn handle_presence_watch_stamped(
        &self,
        stamp: &Stamp,
        body: &[u8],
        sink: &dyn WatchSink,
    ) -> HandlerOutcome {
        match self.presence_watch(stamp, body, sink).await {
            Ok(()) => HandlerOutcome::Streamed,
            Err(error) => HandlerOutcome::Error {
                code: error.code.into(),
                message: error.message,
            },
        }
    }

    async fn presence_watch(
        &self,
        stamp: &Stamp,
        body: &[u8],
        sink: &dyn WatchSink,
    ) -> Result<(), Refusal> {
        stamp.presence_reader()?;
        #[derive(Deserialize)]
        struct Request {
            method: String,
        }
        if body.len() > 1_310_720 {
            return Err(invalid("request"));
        }
        let request: Request = serde_json::from_slice(body).map_err(|_| invalid("request"))?;
        if request.method != "presence.watch" {
            return Err(invalid("method"));
        }
        let mut stopping = self.inner.stopping.subscribe();
        if *stopping.borrow() {
            return Err(Refusal::new("presence_unavailable", "module is stopping"));
        }
        let _slot = self
            .inner
            .watch_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Refusal::new("watch_capacity", "too many watches"))?;
        tokio::select! {
            biased;
            _ = sink.ended() => {},
            _ = stopping.wait_for(|value| *value) => {},
            _ = self.presence_stream(sink) => {},
        }
        Ok(())
    }

    async fn presence_stream(&self, sink: &dyn WatchSink) {
        // Subscribe before reading so a concurrent report cannot be lost.
        let mut changes = self.inner.presence_changes.subscribe();
        changes.borrow_and_update();
        let (_, mut sent, _) = self.presence_snapshot();
        if sink
            .emit(serde_json::to_vec(&sent).expect("summary serialization"))
            .await
            .is_err()
        {
            return;
        }
        let mut deadline = None;
        loop {
            changes.borrow_and_update();
            let (_, current, expiry) = self.presence_snapshot();
            let now = self.inner.options.clock.now_ms();
            if sent.same_attention(&current) {
                deadline = None;
            } else if deadline.is_none() {
                deadline = Some(now.saturating_add(250));
            }
            if deadline.is_some_and(|deadline| now >= deadline) {
                if sink
                    .emit(serde_json::to_vec(&current).expect("summary serialization"))
                    .await
                    .is_err()
                {
                    return;
                }
                sent = current;
                deadline = None;
                continue;
            }
            // Only the nearest expiry and a pending coalescing window need a timer.
            let wake_at = expiry.into_iter().chain(deadline).min();
            tokio::select! {
                _ = changes.changed() => {},
                _ = async {
                    match wake_at {
                        Some(at) => self.inner.options.clock.sleep_until(at).await,
                        None => std::future::pending::<()>().await,
                    }
                } => {},
            }
        }
    }
}

#[cfg(test)]
mod tests;
