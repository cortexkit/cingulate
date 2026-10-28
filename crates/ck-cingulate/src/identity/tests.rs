use super::*;
use cingulate_decode::AccessRequest;
use cingulate_store::{AccessIdentity, AccessRecord};
use cortexkit_agent_identity::{AgentGone, CoreAgentResolution, GoneReason};
use std::{
    collections::VecDeque,
    sync::atomic::{AtomicBool, AtomicI64, AtomicUsize},
};

mod projects;

#[derive(Default)]
struct TestClock(AtomicI64);
impl TestClock {
    fn advance(&self, ms: i64) {
        self.0.fetch_add(ms, Ordering::Relaxed);
    }
}
#[async_trait]
impl Clock for TestClock {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::Relaxed)
    }
    async fn sleep_until(&self, _: i64) {
        std::future::pending().await
    }
}

#[derive(Default)]
struct FakeRuntime {
    replies: StdMutex<VecDeque<Result<AgentStatus, Failure>>>,
    calls: AtomicUsize,
    hang: AtomicBool,
}
#[async_trait]
impl AgentRuntime for FakeRuntime {
    async fn fleet_identity(
        &self,
        _: AgentFleetIdentityParams,
    ) -> Result<AgentFleetIdentityReply, Failure> {
        Err(Failure::Unavailable)
    }

    async fn resolve(&self, params: AgentResolveParams) -> Result<AgentResolveReply, Failure> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        if self.hang.load(Ordering::Relaxed) {
            std::future::pending::<()>().await;
        }
        let status = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Ok(AgentStatus::Live))?;
        let (gone, merged_into) = match status {
            AgentStatus::Retired => (
                Some(AgentGone::new(GoneReason::Deleted, 1, Presence::Missing)),
                None,
            ),
            AgentStatus::Merged => (
                Some(AgentGone::new(
                    GoneReason::Merged,
                    1,
                    Presence::Present("survivor".into()),
                )),
                Some("survivor".into()),
            ),
            _ => (None, None),
        };
        Ok(AgentResolveReply::new(
            CoreAgentResolution::new(params.agent_id, status, merged_into, gone, 1),
            "incarnation".into(),
        ))
    }
}

struct SampleRuntime {
    replies: StdMutex<VecDeque<Value>>,
    requests: StdMutex<Vec<Value>>,
    resolve_calls: AtomicUsize,
}

#[async_trait]
impl AgentRuntime for SampleRuntime {
    async fn resolve(&self, params: AgentResolveParams) -> Result<AgentResolveReply, Failure> {
        self.resolve_calls.fetch_add(1, Ordering::Relaxed);
        Ok(AgentResolveReply::new(
            CoreAgentResolution::new(params.agent_id, AgentStatus::Live, None, None, 1),
            "incarnation".into(),
        ))
    }

    async fn fleet_identity(
        &self,
        params: AgentFleetIdentityParams,
    ) -> Result<AgentFleetIdentityReply, Failure> {
        self.requests
            .lock()
            .unwrap()
            .push(serde_json::to_value(Request::new("agent.fleet_identity", params)).unwrap());
        let reply = self.replies.lock().unwrap().pop_front().unwrap();
        decode_reply(&serde_json::to_vec(&reply).unwrap())
    }
}

#[derive(Default)]
struct FakeNames {
    replies: StdMutex<VecDeque<Result<RosterReply, Failure>>>,
    tokens: StdMutex<Vec<Option<String>>>,
    hang: AtomicBool,
}
#[async_trait]
impl NameSource for FakeNames {
    async fn refresh(&self, token: Option<&str>) -> Result<RosterReply, Failure> {
        self.tokens.lock().unwrap().push(token.map(str::to_owned));
        if self.hang.load(Ordering::Relaxed) {
            std::future::pending::<()>().await;
        }
        self.replies
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Ok(RosterReply::Unchanged))
    }
}

fn snapshot(token: &str, names: &[(&str, &str)]) -> Result<RosterReply, Failure> {
    Ok(RosterReply::Snapshot {
        token: token.into(),
        names: names
            .iter()
            .map(|(id, name)| ((*id).into(), (*name).into()))
            .collect(),
    })
}

struct Fixture {
    resolver: Arc<EntorhinalResolver>,
    clock: Arc<TestClock>,
    runtime: Arc<FakeRuntime>,
    names: Arc<FakeNames>,
    handler: Handler,
    _dir: tempfile::TempDir,
}
impl Fixture {
    fn new(
        statuses: Vec<Result<AgentStatus, Failure>>,
        snapshots: Vec<Result<RosterReply, Failure>>,
    ) -> Self {
        let clock = Arc::new(TestClock::default());
        let runtime = Arc::new(FakeRuntime {
            replies: StdMutex::new(statuses.into()),
            ..Default::default()
        });
        let names = Arc::new(FakeNames {
            replies: StdMutex::new(snapshots.into()),
            ..Default::default()
        });
        let mut resolver = EntorhinalResolver::new(clock.clone());
        *resolver.runtime.lock().unwrap() = Some(runtime.clone());
        resolver.names = names.clone();
        let resolver = Arc::new(resolver);
        let dir = tempfile::tempdir().unwrap();
        let handler = Handler::new(Options {
            resolver: resolver.clone(),
            clock: clock.clone(),
            sweep_timer: false,
            data_home: dir.path().to_str().unwrap().into(),
            ..Options::default()
        });
        Self {
            resolver,
            clock,
            runtime,
            names,
            handler,
            _dir: dir,
        }
    }

    async fn identity(&self) -> Result<RequestIdentity, Refusal> {
        let request = ConsentRequest::decode(
            serde_json::from_str(include_str!(
                "../../../../test-vectors/elicitation-request-v1/valid/app-grant-once.json"
            ))
            .unwrap(),
        )
        .unwrap();
        self.handler
            .identity(
                &Stamp {
                    principal: Some("reserved:plexus".into()),
                    carried_agent_id: "agent-alf".into(),
                    ..Default::default()
                },
                &request,
            )
            .await
    }

    async fn assert_labels(&self, expected: &str) {
        assert_eq!(self.identity().await.unwrap().subject.label, expected);
        assert_eq!(
            self.handler.access_phone_label(&permission_record()).await,
            expected
        );
    }
}

fn permission_record() -> AccessRecord {
    AccessRecord {
        elicitation_id: "acc_test".into(),
        request: AccessRequest::decode(json!({
            "kind":"tool_permission","call_key":"test","tool":"test",
            "operation":"exec","paths":[],"command":"cmd",
            "reason":"Test permission","args_hash":"hash",
            "pattern":"test","head_policy":"allow"
        }))
        .unwrap(),
        identity: AccessIdentity {
            principal: "reserved:aft".into(),
            scope_owner: Some("reserved:prefrontal-core".into()),
            asker: ScopeIdentity {
                scope_ref: "worker".into(),
                scope_epoch: 1,
                agent_id: None,
            },
            scope_kind: "worker".into(),
            harness: "test".into(),
            head: Some(ScopeIdentity {
                scope_ref: "head".into(),
                scope_epoch: 2,
                agent_id: Some("agent-alf".into()),
            }),
        },
        created_at_ms: 1,
        state: "pending:head".into(),
        surfaces: vec![],
        attempts: vec![],
        escalation_note: None,
        outcome: None,
        note: None,
        answered_by: None,
        withdrawn_by: None,
        settled_at_ms: None,
        unclaimed: false,
    }
}

#[tokio::test]
async fn roster_name_reaches_subject_and_permission_labels() {
    let f = Fixture::new(vec![], vec![snapshot("one", &[("agent-alf", "Alfonso")])]);
    f.assert_labels("Alfonso").await;
    assert_eq!(f.runtime.calls.load(Ordering::Relaxed), 0);
    assert_eq!(*f.names.tokens.lock().unwrap(), vec![None]);
}

#[tokio::test]
async fn live_and_unknown_status_fall_back_to_id_without_refusing() {
    for status in [AgentStatus::Live, AgentStatus::Unknown] {
        let f = Fixture::new(vec![Ok(status.clone()), Ok(status)], vec![]);
        f.assert_labels("agent-alf").await;
    }
}

#[tokio::test]
async fn retired_and_merged_status_refuse_consent_retired() {
    for status in [AgentStatus::Retired, AgentStatus::Merged] {
        let f = Fixture::new(vec![Ok(status)], vec![]);
        assert!(
            f.resolver
                .resolve("agent-alf")
                .await
                .unwrap()
                .unwrap()
                .retired
        );
        assert_eq!(f.identity().await.unwrap_err().code, "consent_retired");
        assert_eq!(f.runtime.calls.load(Ordering::Relaxed), 1);
    }
}

#[tokio::test(start_paused = true)]
async fn timeout_falls_back_to_id() {
    let f = Fixture::new(vec![], vec![]);
    f.runtime.hang.store(true, Ordering::Relaxed);
    let start = tokio::time::Instant::now();
    f.assert_labels("agent-alf").await;
    assert_eq!(start.elapsed(), Duration::from_millis(1_000));
    assert!(f.resolver.cache.lock().unwrap().is_empty());
    f.runtime.hang.store(false, Ordering::Relaxed);
    f.assert_labels("agent-alf").await;
    assert_eq!(f.runtime.calls.load(Ordering::Relaxed), 3);
}

#[tokio::test]
async fn runtime_errors_fall_back_to_id() {
    for kind in [
        Failure::Route,
        Failure::Transport,
        Failure::Refused,
        Failure::Decode,
    ] {
        let f = Fixture::new(vec![Err(kind), Err(kind)], vec![]);
        f.assert_labels("agent-alf").await;
    }
}

#[tokio::test]
async fn failures_are_not_cached() {
    for first in [
        Err(Failure::Route),
        Err(Failure::Transport),
        Err(Failure::Refused),
        Err(Failure::Decode),
        Ok(AgentStatus::Unknown),
        Ok(AgentStatus::Other("future".into())),
    ] {
        let f = Fixture::new(vec![first, Ok(AgentStatus::Retired)], vec![]);
        assert!(f.resolver.resolve("agent-alf").await.unwrap().is_none());
        assert_eq!(f.identity().await.unwrap_err().code, "consent_retired");
        assert_eq!(f.runtime.calls.load(Ordering::Relaxed), 2);
    }
}

#[tokio::test]
async fn positive_and_retired_cache_expires_at_sixty_seconds() {
    for status in [AgentStatus::Live, AgentStatus::Retired, AgentStatus::Merged] {
        let f = Fixture::new(vec![Ok(status)], vec![]);
        let first = f.resolver.resolve("agent-alf").await.unwrap().unwrap();
        f.clock.advance(59_999);
        let second = f.resolver.resolve("agent-alf").await.unwrap().unwrap();
        assert_eq!(first.retired, second.retired);
        assert_eq!(f.runtime.calls.load(Ordering::Relaxed), 1);
        f.clock.advance(1);
        assert!(
            !f.resolver
                .resolve("agent-alf")
                .await
                .unwrap()
                .unwrap()
                .retired
        );
        assert_eq!(f.runtime.calls.load(Ordering::Relaxed), 2);
    }
}

#[tokio::test]
async fn result_cache_is_bounded_and_keyed_by_agent_id() {
    let f = Fixture::new(vec![], vec![]);
    for index in 0..=CACHE_CAPACITY {
        f.resolver
            .resolve(&format!("agent-{index}"))
            .await
            .unwrap()
            .unwrap();
        f.clock.advance(1);
    }
    {
        let cache = f.resolver.cache.lock().unwrap();
        assert_eq!(cache.len(), 1_024);
        assert!(!cache.contains_key("agent-0"));
        assert!(cache.contains_key("agent-1024"));
    }
    f.resolver.resolve("agent-1024").await.unwrap().unwrap();
    assert_eq!(f.runtime.calls.load(Ordering::Relaxed), 1_025);
}

#[tokio::test]
async fn roster_hit_and_expiry_refresh_with_token() {
    let f = Fixture::new(
        vec![],
        vec![
            snapshot("one", &[("agent-alf", "Alfonso"), ("other", "Other")]),
            Ok(RosterReply::Unchanged),
            snapshot("two", &[("agent-alf", "New name")]),
        ],
    );
    f.assert_labels("Alfonso").await;
    assert_eq!(f.resolver.name("other").await.unwrap().unwrap().0, "Other");
    assert_eq!(*f.names.tokens.lock().unwrap(), vec![None]);
    f.clock.advance(60_000);
    f.assert_labels("Alfonso").await;
    assert_eq!(
        *f.names.tokens.lock().unwrap(),
        vec![None, Some("one".into())]
    );
    f.clock.advance(60_000);
    f.assert_labels("New name").await;
    assert_eq!(f.resolver.roster.lock().await.token.as_deref(), Some("two"));
    assert!(!f.resolver.roster.lock().await.names.contains_key("other"));
    assert_eq!(f.runtime.calls.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn roster_miss_refreshes_once_before_status_lookup() {
    let f = Fixture::new(
        vec![Ok(AgentStatus::Unknown)],
        vec![
            snapshot("one", &[("other", "Other")]),
            snapshot("two", &[("agent-alf", "Alfonso")]),
            Ok(RosterReply::Unchanged),
        ],
    );
    assert_eq!(f.resolver.name("other").await.unwrap().unwrap().0, "Other");
    f.assert_labels("Alfonso").await;
    assert_eq!(f.names.tokens.lock().unwrap().len(), 2);
    assert!(f.resolver.resolve("missing").await.unwrap().is_none());
    assert_eq!(
        *f.names.tokens.lock().unwrap(),
        vec![None, Some("one".into()), Some("two".into())]
    );
    assert_eq!(f.runtime.calls.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn cold_roster_miss_does_not_refresh_twice() {
    let f = Fixture::new(vec![Ok(AgentStatus::Unknown)], vec![snapshot("one", &[])]);
    assert!(f.resolver.resolve("agent-alf").await.unwrap().is_none());
    assert_eq!(*f.names.tokens.lock().unwrap(), vec![None]);
    assert_eq!(f.runtime.calls.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn roster_errors_are_not_cached() {
    let f = Fixture::new(
        vec![],
        vec![
            Err(Failure::Transport),
            snapshot("one", &[("agent-alf", "Alfonso")]),
        ],
    );
    assert_eq!(f.identity().await.unwrap().subject.label, "agent-alf");
    f.assert_labels("Alfonso").await;
    assert_eq!(*f.names.tokens.lock().unwrap(), vec![None, None]);
}

#[tokio::test]
async fn roster_failures_still_refuse_retired_agents() {
    for kind in [Failure::Refused, Failure::Decode, Failure::Transport] {
        let f = Fixture::new(vec![Ok(AgentStatus::Retired)], vec![Err(kind)]);
        assert_eq!(f.identity().await.unwrap_err().code, "consent_retired");
        assert_eq!(f.runtime.calls.load(Ordering::Relaxed), 1);
        assert!(f.resolver.warnings.lock().unwrap().contains_key(&kind));
    }
}

#[tokio::test]
async fn roster_failures_leave_live_agents_unnamed_and_retryable() {
    for kind in [Failure::Refused, Failure::Decode, Failure::Transport] {
        let f = Fixture::new(
            vec![Ok(AgentStatus::Live), Ok(AgentStatus::Live)],
            vec![Err(kind), Err(kind)],
        );
        f.assert_labels("agent-alf").await;
        assert_eq!(f.runtime.calls.load(Ordering::Relaxed), 2);
        assert!(f.resolver.cache.lock().unwrap().is_empty());
        assert!(f.resolver.roster.lock().await.names.is_empty());
    }
}

#[tokio::test(start_paused = true)]
async fn roster_timeout_leaves_time_to_refuse_retired_agents() {
    let f = Fixture::new(vec![Ok(AgentStatus::Retired)], vec![]);
    f.names.hang.store(true, Ordering::Relaxed);
    let start = tokio::time::Instant::now();
    assert_eq!(f.identity().await.unwrap_err().code, "consent_retired");
    assert_eq!(start.elapsed(), Duration::from_millis(250));
    assert_eq!(f.runtime.calls.load(Ordering::Relaxed), 1);
}

#[tokio::test(start_paused = true)]
async fn stalled_name_and_status_reads_share_the_overall_deadline() {
    let f = Fixture::new(vec![], vec![]);
    f.names.hang.store(true, Ordering::Relaxed);
    f.runtime.hang.store(true, Ordering::Relaxed);
    let start = tokio::time::Instant::now();
    assert_eq!(f.identity().await.unwrap().subject.label, "agent-alf");
    assert_eq!(start.elapsed(), Duration::from_millis(500));
    assert_eq!(f.runtime.calls.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn roster_derived_results_expire_with_the_roster() {
    let f = Fixture::new(
        vec![Ok(AgentStatus::Retired)],
        vec![
            snapshot("one", &[("first", "First"), ("agent-alf", "Alfonso")]),
            snapshot("two", &[]),
        ],
    );
    assert_eq!(
        f.resolver
            .resolve("first")
            .await
            .unwrap()
            .unwrap()
            .name
            .as_deref(),
        Some("First")
    );
    f.clock.advance(59_000);
    assert_eq!(f.identity().await.unwrap().subject.label, "Alfonso");
    assert_eq!(f.runtime.calls.load(Ordering::Relaxed), 0);
    f.clock.advance(1_000);
    assert_eq!(f.identity().await.unwrap_err().code, "consent_retired");
    assert_eq!(f.runtime.calls.load(Ordering::Relaxed), 1);
    assert_eq!(
        *f.names.tokens.lock().unwrap(),
        vec![None, Some("one".into())]
    );
}

#[tokio::test]
async fn warnings_are_throttled_per_kind_for_one_minute() {
    let f = Fixture::new(
        vec![
            Err(Failure::Route),
            Err(Failure::Route),
            Err(Failure::Transport),
            Err(Failure::Route),
        ],
        vec![],
    );
    f.resolver.resolve("agent-alf").await.unwrap();
    f.clock.advance(59_999);
    f.resolver.resolve("agent-alf").await.unwrap();
    f.resolver.resolve("other").await.unwrap();
    {
        let warnings = f.resolver.warnings.lock().unwrap();
        assert_eq!(warnings[&Failure::Route], 0);
        assert_eq!(warnings[&Failure::Transport], 59_999);
    }
    f.clock.advance(1);
    f.resolver.resolve("agent-alf").await.unwrap();
    assert_eq!(f.resolver.warnings.lock().unwrap()[&Failure::Route], 60_000);
}

#[tokio::test]
async fn fleet_sample_unchanged_preserves_display_names() {
    // Byte-for-byte owner capture published with cortexkit-agent-identity 0.1.1:
    // test-vectors/agent-identity-reads-v1/fleet-identity.json.
    let samples: Vec<Value> =
        serde_json::from_str(include_str!("../../tests/fixtures/fleet-identity.json")).unwrap();
    let runtime = Arc::new(SampleRuntime {
        replies: StdMutex::new(
            samples
                .iter()
                .map(|sample| sample["reply"].clone())
                .collect(),
        ),
        requests: StdMutex::new(vec![]),
        resolve_calls: AtomicUsize::new(0),
    });
    let clock = Arc::new(TestClock::default());
    let resolver = Arc::new(EntorhinalResolver::new(clock.clone()));
    *resolver.runtime.lock().unwrap() = Some(runtime.clone());
    let dir = tempfile::tempdir().unwrap();
    let handler = Handler::new(Options {
        resolver: resolver.clone(),
        clock: clock.clone(),
        sweep_timer: false,
        data_home: dir.path().to_str().unwrap().into(),
        ..Default::default()
    });
    let request = ConsentRequest::decode(
        serde_json::from_str(include_str!(
            "../../../../test-vectors/elicitation-request-v1/valid/app-grant-once.json"
        ))
        .unwrap(),
    )
    .unwrap();
    let stamp = Stamp {
        principal: Some("reserved:plexus".into()),
        carried_agent_id: "agent_0000000000000001".into(),
        ..Default::default()
    };
    assert_eq!(
        handler
            .identity(&stamp, &request)
            .await
            .unwrap()
            .subject
            .label,
        "Ada"
    );
    let mut record = permission_record();
    record.identity.head.as_mut().unwrap().agent_id = Some("agent_0000000000000002".into());
    assert_eq!(handler.access_phone_label(&record).await, "Lin");
    clock.advance(60_000);
    assert_eq!(
        handler
            .identity(&stamp, &request)
            .await
            .unwrap()
            .subject
            .label,
        "Ada"
    );
    assert_eq!(handler.access_phone_label(&record).await, "Lin");
    {
        let requests = runtime.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0]["method"], samples[0]["op"]);
        assert_eq!(requests[0]["params"], samples[0]["request"]);
        assert_eq!(requests[1]["params"], samples[1]["request"]);
    }
    assert_eq!(runtime.resolve_calls.load(Ordering::Relaxed), 0);
    clock.advance(60_000);
    assert_eq!(
        handler
            .identity(&stamp, &request)
            .await
            .unwrap()
            .subject
            .label,
        "agent_0000000000000001"
    );
    assert_eq!(runtime.resolve_calls.load(Ordering::Relaxed), 1);
}
