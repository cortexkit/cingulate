//! Retained consent answers whose stamped session ended receive an execution backstop
//! only if no report exists. Scope lookups never hold the store lock.
use super::*;

const BATCH_SIZE: usize = 16;
const PASS_BUDGET: Duration = Duration::from_secs(1);
const PROBE_BUDGET: Duration = Duration::from_millis(250);

#[derive(Default)]
pub(super) struct Maintenance {
    after: Option<String>,
    retry_at_ms: i64,
    attempts: u32,
}

impl Handler {
    pub(super) async fn execution_maintenance(&self) -> Result<(), Refusal> {
        // Concurrent manual ticks must not bypass the retry delay or restart a page.
        let Ok(mut state) = self.inner.execution_maintenance.try_lock() else {
            return Ok(());
        };
        let now = self.inner.options.clock.now_ms();
        if now < state.retry_at_ms {
            return Ok(());
        }
        let page = self
            .with_store(|store| store.execution_candidates(state.after.as_deref(), BATCH_SIZE, now))
            .await?;
        if page.after.is_none() {
            state.after = None;
            return Ok(());
        }
        let deadline = tokio::time::Instant::now() + PASS_BUDGET;
        let mut unavailable = false;
        let mut interrupted = false;
        for record in page.records {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                interrupted = true;
                break;
            }
            let life = tokio::time::timeout(
                remaining.min(PROBE_BUDGET),
                self.access_runtime()
                    .describe_asker(record.asker.as_ref().expect("candidate asker")),
            )
            .await;
            match life {
                Ok(Ok(ScopeLife::Gone)) => {
                    self.with_store(|store| {
                        store.execution_session_ended(
                            &record.elicitation_id,
                            self.inner.options.clock.now_ms(),
                        )
                    })
                    .await?;
                }
                Ok(Ok(ScopeLife::Live(_))) => {}
                Ok(Ok(ScopeLife::Unknown)) | Ok(Err(_)) | Err(_) => unavailable = true,
            }
            state.after = Some(record.elicitation_id);
        }
        if !interrupted {
            state.after = page.after;
        }
        // Use the notice outbox's bounded exponential backoff for uncertain probes.
        if unavailable {
            state.retry_at_ms =
                now.saturating_add(cingulate_store::maintenance_backoff_ms(state.attempts));
            state.attempts = state.attempts.saturating_add(1);
        } else {
            state.attempts = 0;
            state.retry_at_ms = now;
        }
        Ok(())
    }
}
