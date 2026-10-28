use super::*;
use crate::identity::projects::{decode_project_reply, ProjectRuntime};
use cortexkit_project_identity::{
    EnumerateParams, EnumerateReply, Project, RefKind, ResolveProjectIdParams,
    ResolveProjectIdReply, Via,
};
use subc_protocol::scope::{ScopeKind, ScopeStamp};

#[derive(Default)]
struct FakeProjects {
    resolutions: StdMutex<VecDeque<Result<ResolveProjectIdReply, Failure>>>,
    enumerations: StdMutex<VecDeque<Result<EnumerateReply, Failure>>>,
    ids: StdMutex<Vec<String>>,
    enumerate_calls: AtomicUsize,
    hang_resolve: AtomicBool,
    hang_enumerate: AtomicBool,
}

#[async_trait]
impl ProjectRuntime for FakeProjects {
    async fn resolve_project_id(
        &self,
        params: ResolveProjectIdParams,
    ) -> Result<ResolveProjectIdReply, Failure> {
        self.ids.lock().unwrap().push(params.project_id.clone());
        if self.hang_resolve.load(Ordering::Relaxed) {
            std::future::pending::<()>().await;
        }
        self.resolutions
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| {
                Ok(ResolveProjectIdReply::new(
                    params.project_id,
                    Via::Current,
                    false,
                    1,
                    "registry".into(),
                ))
            })
    }

    async fn enumerate(&self, params: EnumerateParams) -> Result<EnumerateReply, Failure> {
        assert!(params.workspace_id.is_none());
        self.enumerate_calls.fetch_add(1, Ordering::Relaxed);
        if self.hang_enumerate.load(Ordering::Relaxed) {
            std::future::pending::<()>().await;
        }
        self.enumerations.lock().unwrap().pop_front().unwrap()
    }
}

fn registry(names: &[(&str, &str)]) -> Result<EnumerateReply, Failure> {
    Ok(EnumerateReply::new(
        vec![],
        names
            .iter()
            .map(|(id, name)| {
                Project::new(
                    (*id).into(),
                    (*name).into(),
                    vec!["/project/root".into()],
                    false,
                    RefKind::Local,
                    None,
                    None,
                    None,
                    None,
                )
            })
            .collect(),
        1,
        "registry".into(),
    ))
}

fn fixture() -> (Fixture, Arc<FakeProjects>) {
    let f = Fixture::new(vec![], vec![snapshot("one", &[("agent-alf", "Alfonso")])]);
    let projects = Arc::new(FakeProjects::default());
    *f.resolver.projects.runtime.lock().unwrap() = Some(projects.clone());
    (f, projects)
}

fn stamp(project_id: Option<&str>) -> Stamp {
    Stamp {
        principal: Some("reserved:plexus".into()),
        carried_agent_id: "agent-alf".into(),
        scope: Some(
            ScopeStamp::new(
                Principal::Reserved {
                    module_id: "prefrontal-core".into(),
                },
                "head",
                1,
                ScopeKind::Head,
            )
            .with_project_id(project_id.map(str::to_owned)),
        ),
        ..Default::default()
    }
}

fn request() -> ConsentRequest {
    ConsentRequest::decode(
        serde_json::from_str(include_str!(
            "../../../../../test-vectors/elicitation-request-v1/valid/app-grant-once.json"
        ))
        .unwrap(),
    )
    .unwrap()
}

async fn label(f: &Fixture, project_id: Option<&str>) -> String {
    f.handler
        .identity(&stamp(project_id), &request())
        .await
        .unwrap()
        .subject
        .label
}

#[tokio::test]
async fn stamped_project_name_is_stored_served_and_refreshed() {
    let (f, projects) = fixture();
    projects
        .enumerations
        .lock()
        .unwrap()
        .push_back(registry(&[("pj-one", "Cingulate")]));
    let identity = f
        .handler
        .identity(&stamp(Some("pj-one")), &request())
        .await
        .unwrap();
    assert_eq!(identity.subject.label, "Alfonso in project Cingulate");
    assert_eq!(
        identity.resolved_name.as_deref(),
        Some("Alfonso in project Cingulate")
    );
    assert_eq!(*projects.ids.lock().unwrap(), ["pj-one"]);

    let descriptor = cingulate_store::storage_descriptor(None, f._dir.path().to_str().unwrap());
    let (store, _) = ConsentStore::open(&descriptor, 0).unwrap();
    let mut request = request();
    request.dedup_key = Some("project-subject".into());
    request.expires_in_ms = Some(CACHE_MS * 3);
    let created = store.create(&identity, request.clone(), 0).unwrap();
    assert_eq!(created.record.subject.label, "Alfonso in project Cingulate");
    let id = created.record.elicitation_id;
    assert_eq!(
        store.get_phone(&id, 0).unwrap().unwrap().phone(None)["subject"]["label"],
        "Alfonso in project Cingulate"
    );

    f.clock.advance(CACHE_MS);
    projects
        .enumerations
        .lock()
        .unwrap()
        .push_back(registry(&[("pj-one", "Renamed")]));
    let identity = f
        .handler
        .identity(&stamp(Some("pj-one")), &request)
        .await
        .unwrap();
    let refreshed = store.create(&identity, request, CACHE_MS).unwrap();
    assert!(!refreshed.created);
    assert_eq!(refreshed.record.elicitation_id, id);
    drop(store);
    let (store, _) = ConsentStore::open(&descriptor, CACHE_MS).unwrap();
    let stored = store.get_phone(&id, CACHE_MS).unwrap().unwrap();
    assert_eq!(stored.subject.label, "Alfonso in project Renamed");
    assert_eq!(
        stored.phone(None)["subject"]["label"],
        "Alfonso in project Renamed"
    );
}

#[tokio::test]
async fn alias_uses_current_project_name() {
    let (f, projects) = fixture();
    projects
        .resolutions
        .lock()
        .unwrap()
        .push_back(Ok(ResolveProjectIdReply::new(
            "pj-current".into(),
            Via::Alias,
            false,
            1,
            "registry".into(),
        )));
    projects.enumerations.lock().unwrap().push_back(registry(&[
        ("pj-old", "Obsolete name"),
        ("pj-current", "Current name"),
    ]));
    assert_eq!(
        label(&f, Some("pj-old")).await,
        "Alfonso in project Current name"
    );
    assert_eq!(*projects.ids.lock().unwrap(), ["pj-old"]);
}

#[tokio::test]
async fn gone_project_shows_stamped_id_without_enumerating() {
    let (f, projects) = fixture();
    projects
        .resolutions
        .lock()
        .unwrap()
        .push_back(Ok(ResolveProjectIdReply::new(
            "pj-gone".into(),
            Via::Gone,
            true,
            1,
            "registry".into(),
        )));
    projects
        .enumerations
        .lock()
        .unwrap()
        .push_back(registry(&[("pj-gone", "Old name")]));
    assert_eq!(
        label(&f, Some("pj-gone")).await,
        "Alfonso in project pj-gone"
    );
    assert_eq!(projects.enumerate_calls.load(Ordering::Relaxed), 0);
}

#[tokio::test(start_paused = true)]
async fn project_resolution_timeout_shows_stamped_id() {
    let (f, projects) = fixture();
    projects.hang_resolve.store(true, Ordering::Relaxed);
    let start = tokio::time::Instant::now();
    assert_eq!(label(&f, Some("pj-one")).await, "Alfonso in project pj-one");
    assert_eq!(start.elapsed(), LOOKUP_TIMEOUT);
    assert_eq!(projects.enumerate_calls.load(Ordering::Relaxed), 0);
}

#[tokio::test(start_paused = true)]
async fn project_enumeration_timeout_shows_stamped_id() {
    let (f, projects) = fixture();
    projects.hang_enumerate.store(true, Ordering::Relaxed);
    let start = tokio::time::Instant::now();
    assert_eq!(label(&f, Some("pj-one")).await, "Alfonso in project pj-one");
    assert_eq!(start.elapsed(), LOOKUP_TIMEOUT);
}

#[tokio::test(start_paused = true)]
async fn project_and_agent_timeouts_share_the_same_deadline() {
    let (f, projects) = fixture();
    f.names.hang.store(true, Ordering::Relaxed);
    f.runtime.hang.store(true, Ordering::Relaxed);
    projects.hang_resolve.store(true, Ordering::Relaxed);
    let start = tokio::time::Instant::now();
    assert_eq!(
        label(&f, Some("pj-one")).await,
        "agent-alf in project pj-one"
    );
    assert_eq!(start.elapsed(), LOOKUP_TIMEOUT);
}

#[tokio::test]
async fn absent_project_preserves_subject_byte_for_byte() {
    let (f, projects) = fixture();
    let before = f.identity().await.unwrap();
    let without_project = f.handler.identity(&stamp(None), &request()).await.unwrap();
    assert_eq!(
        serde_json::to_vec(&without_project.subject).unwrap(),
        serde_json::to_vec(&before.subject).unwrap(),
    );
    assert_eq!(without_project.subject.label, "Alfonso");
    assert!(projects.ids.lock().unwrap().is_empty());
    assert_eq!(projects.enumerate_calls.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn filer_scope_cannot_name_or_override_project() {
    let (f, projects) = fixture();
    projects
        .enumerations
        .lock()
        .unwrap()
        .push_back(registry(&[("pj-one", "Trusted name")]));
    let mut forged = serde_json::to_value(request()).unwrap();
    forged["scope"] =
        json!({"project_id":"pj-forged", "projectId":"pj-forged", "name":"Forged name"});
    let forged = ConsentRequest::decode(forged).unwrap();
    assert_eq!(
        f.handler
            .identity(&stamp(Some("pj-one")), &forged)
            .await
            .unwrap()
            .subject
            .label,
        "Alfonso in project Trusted name",
    );
    assert_eq!(
        f.handler
            .identity(&stamp(None), &forged)
            .await
            .unwrap()
            .subject
            .label,
        "Alfonso"
    );
    assert_eq!(*projects.ids.lock().unwrap(), ["pj-one"]);
}

#[tokio::test]
async fn project_enumeration_is_shared_and_expires_after_one_minute() {
    let (f, projects) = fixture();
    projects.enumerations.lock().unwrap().extend([
        registry(&[("pj-one", "First"), ("pj-two", "Second")]),
        registry(&[("pj-one", "Renamed")]),
    ]);
    assert_eq!(label(&f, Some("pj-one")).await, "Alfonso in project First");
    assert_eq!(label(&f, Some("pj-two")).await, "Alfonso in project Second");
    assert_eq!(
        label(&f, Some("pj-missing")).await,
        "Alfonso in project pj-missing"
    );
    f.clock.advance(CACHE_MS - 1);
    assert_eq!(label(&f, Some("pj-one")).await, "Alfonso in project First");
    assert_eq!(projects.enumerate_calls.load(Ordering::Relaxed), 1);
    f.clock.advance(1);
    assert_eq!(
        label(&f, Some("pj-one")).await,
        "Alfonso in project Renamed"
    );
    assert_eq!(projects.enumerate_calls.load(Ordering::Relaxed), 2);
}

#[tokio::test]
async fn failed_or_unavailable_project_reads_show_stamped_id() {
    let (f, projects) = fixture();
    for failure in [
        Failure::Decode,
        Failure::Refused,
        Failure::Transport,
        Failure::Route,
    ] {
        projects.resolutions.lock().unwrap().push_back(Err(failure));
        assert_eq!(label(&f, Some("pj-one")).await, "Alfonso in project pj-one");
        projects
            .enumerations
            .lock()
            .unwrap()
            .push_back(Err(failure));
        assert_eq!(label(&f, Some("pj-one")).await, "Alfonso in project pj-one");
    }
    *f.resolver.projects.runtime.lock().unwrap() = None;
    assert_eq!(label(&f, Some("pj-one")).await, "Alfonso in project pj-one");
}

#[tokio::test]
async fn expired_project_cache_does_not_supply_stale_names_on_failure() {
    let (f, projects) = fixture();
    projects.enumerations.lock().unwrap().extend([
        registry(&[("pj-one", "Old name")]),
        Err(Failure::Unavailable),
        registry(&[("pj-one", "New name")]),
    ]);
    assert_eq!(
        label(&f, Some("pj-one")).await,
        "Alfonso in project Old name"
    );
    f.clock.advance(CACHE_MS);
    assert_eq!(label(&f, Some("pj-one")).await, "Alfonso in project pj-one");
    assert_eq!(
        label(&f, Some("pj-one")).await,
        "Alfonso in project New name"
    );
}

#[test]
fn project_reply_decoder_requires_typed_success_envelopes() {
    let reply: ResolveProjectIdReply = decode_project_reply(br#"{"result":{"projectId":"pj-current","via":"alias","gone":false,"generation":1,"incarnation":"registry"}}"#).unwrap();
    assert_eq!(reply.project_id, "pj-current");
    assert_eq!(reply.via, Via::Alias);
    assert!(matches!(
        decode_project_reply::<ResolveProjectIdReply>(
            br#"{"code":"unknown_method","message":"unavailable"}"#
        ),
        Err(Failure::Refused)
    ));
    assert!(matches!(
        decode_project_reply::<EnumerateReply>(
            br#"{"result":{"generation":1,"incarnation":"registry"}}"#
        ),
        Err(Failure::Decode)
    ));
}
