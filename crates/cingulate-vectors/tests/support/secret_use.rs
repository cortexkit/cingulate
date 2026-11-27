#![allow(dead_code)]

use std::collections::BTreeMap;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ck_cingulate::{Stamp, UnavailableResolver};
use serde_json::{json, Value};
use subc_client_rs::{HandlerOutcome, RouteBindRequest, RouteHandle};
use subc_protocol::{
    scope::{ScopeAttributes, ScopeKind, ScopeStamp},
    BindIdentity, Principal, RouteTarget,
};

use crate::support::Fixture;

pub const CORPUS: &str = "secret-use-approval-v1";
pub const DEVICE: &str = "11111111111111111111111111111111";
pub const NOW: i64 = 1_700_000_000_000;

pub fn check_refusal_messages(pinned: &[u8], current: &[u8], expected: &[(&str, &str)]) {
    let historical: Value = serde_json::from_slice(pinned).unwrap();
    let mut live: Value = serde_json::from_slice(current).unwrap();
    let cases = live["cases"].as_array_mut().unwrap();
    for &(name, expected_message) in expected {
        let index = cases.iter().position(|case| case["name"] == name).unwrap();
        let message = &mut cases[index]["reply"]["error"]["message"];
        assert_eq!(*message, expected_message, "{name}");
        // Apps pin the published bytes. Compare the live request and reply
        // with only the diagnostic that we just checked restored.
        *message = historical["cases"][index]["reply"]["error"]["message"].clone();
    }
    assert_eq!(live, historical);
}

pub fn check_v1_filer_refusals(pinned: &[u8], current: &[u8]) {
    check_refusal_messages(
        pinned,
        current,
        &[(
            "non-canonical",
            "secret_use.items[0] statement bytes must equal their canonical JSON encoding",
        )],
    );
}

pub fn vector(case: &str) -> Value {
    let all: Value = serde_json::from_slice(include_bytes!(
        "../../../../test-vectors/secret-use-approval-v1/secret-approval-v1.json"
    ))
    .unwrap();
    all["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["case"] == case)
        .unwrap()
        .clone()
}

pub fn body(case: &str) -> Value {
    let items: Vec<_> = vector(case)["statements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| json!({"request_id":s["request_id"],"statement_b64url":s["statement_b64url"]}))
        .collect();
    json!({"kind":"secret_use","secret_use":{"items":items}})
}

pub fn filer(module: &str, agent: Option<&str>) -> Stamp {
    filer_with_scope(
        module,
        ScopeKind::Head,
        ScopeAttributes::new().with_agent_id(agent.map(str::to_owned)),
    )
}

pub fn filer_with_scope(module: &str, kind: ScopeKind, attributes: ScopeAttributes) -> Stamp {
    Stamp::from_bind(
        &RouteBindRequest::new(
            RouteHandle::detached(1, 1),
            RouteTarget::ManagementSurface {
                module_id: "cingulate".into(),
            },
            BindIdentity::new("sender", "ignored"),
        )
        .with_principal(Principal::Reserved {
            module_id: module.into(),
        })
        .with_scope(
            ScopeStamp::new(
                Principal::Reserved {
                    module_id: "prefrontal-core".into(),
                },
                "H",
                7,
                kind,
            )
            .with_attributes(attributes)
            .with_owner_authorized(true),
        ),
    )
}

pub fn phone(device: &str) -> Stamp {
    Stamp::from_bind(
        &RouteBindRequest::new(
            RouteHandle::detached(1, 1),
            RouteTarget::ManagementSurface {
                module_id: "cingulate".into(),
            },
            BindIdentity::new(format!("fed-phone:{device}"), "ignored"),
        )
        .with_principal(Principal::Reserved {
            module_id: "callosum".into(),
        }),
    )
}

pub fn answer(id: &str, case: &str) -> Value {
    json!({"elicitationId":id,"choiceId":"allow","clientFeatures":["secret-use-approval"],"envelope_b64url":vector(case)["envelope_b64url"]})
}

pub fn decoded(encoded: &Value) -> Value {
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(encoded.as_str().unwrap()).unwrap()).unwrap()
}

pub fn encoded(value: &Value) -> Value {
    json!(URL_SAFE_NO_PAD.encode(serde_jcs::to_vec(value).unwrap()))
}

pub async fn fixture() -> Fixture {
    let f = Fixture::open_with_resolver(
        None,
        cingulate_store::CHANGE_LOG_ROWS,
        std::sync::Arc::new(UnavailableResolver),
    )
    .await;
    f.clock.set(NOW);
    f
}

pub async fn generate() -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    for (case, name) in [
        ("approval", "single-phone-get.json"),
        ("batch-omitted", "batch-phone-get.json"),
    ] {
        let f = fixture().await;
        let id = f.request(&filer("aft", Some("agent-A")), body(case)).await;
        let request = serde_json::to_vec(&json!({"method":"consent.get","params":{"elicitationId":id,"clientFeatures":["secret-use-approval"]}})).unwrap();
        let HandlerOutcome::Response(bytes) =
            f.handler.handle_stamped(&phone(DEVICE), &request).await
        else {
            panic!("phone get failed")
        };
        files.insert(name.into(), bytes);
    }
    for (name, bytes) in filer_vectors().await {
        let mut pretty =
            serde_json::to_vec_pretty(&serde_json::from_slice::<Value>(&bytes).unwrap()).unwrap();
        pretty.push(b'\n');
        files.insert(name, pretty);
    }
    files
}

/// Filer-side request/await samples, filed as `reserved:aft` with the vault's
/// `approval` statement (`batch-omitted` for the partial card). Each file holds
/// the exact framed method, request params and handler reply envelope.
async fn filer_vectors() -> BTreeMap<String, Vec<u8>> {
    use std::time::Duration;
    use subc_client_rs::HandlerOutcome;

    async fn capture(fixture: &Fixture, stamp: &Stamp, method: &str, params: Value) -> Vec<u8> {
        let request = serde_json::to_vec(&json!({"method": method, "params": params})).unwrap();
        let outcome = tokio::time::timeout(
            Duration::from_secs(1),
            fixture.handler.handle_stamped(stamp, &request),
        )
        .await
        .expect("filer op exceeded 1 s");
        let reply = match outcome {
            HandlerOutcome::Response(bytes) => serde_json::from_slice::<Value>(&bytes).unwrap(),
            HandlerOutcome::Error { code, message } => {
                json!({"error": {"code": code, "message": message}})
            }
            other => panic!("{method}: {other:?}"),
        };
        // The file holds the exact params the filer sent and the exact reply
        // envelope the handler returned, so decoders can replay both sides.
        serde_json::to_vec(&json!({"method": method, "params": params, "reply": reply})).unwrap()
    }

    let mut files = BTreeMap::new();
    let sender = filer("aft", Some("agent-A"));

    // Fresh filing: the consent.request reply carries the new elicitation id.
    {
        let f = fixture().await;
        files.insert(
            "filer-request.json".into(),
            capture(&f, &sender, "consent.request", body("approval")).await,
        );
    }
    // Repeating the identical filing returns the same card without refreshing it.
    {
        let f = fixture().await;
        f.request(&sender, body("approval")).await;
        files.insert(
            "filer-request-repeat.json".into(),
            capture(&f, &sender, "consent.request", body("approval")).await,
        );
    }
    // Zero-timeout wait before any answer returns the pending filer record.
    {
        let f = fixture().await;
        let id = f.request(&sender, body("approval")).await;
        files.insert(
            "filer-await-pending.json".into(),
            capture(
                &f,
                &sender,
                "consent.await",
                json!({"elicitation_id": id, "timeout_ms": 0}),
            )
            .await,
        );
    }
    // Allow with the vector envelope records per-item decisions and the hash.
    {
        let f = fixture().await;
        let id = f.request(&sender, body("approval")).await;
        f.ok(&phone(DEVICE), "consent.answer", answer(&id, "approval"))
            .await;
        files.insert(
            "filer-await-allowed.json".into(),
            capture(
                &f,
                &sender,
                "consent.await",
                json!({"elicitation_id": id, "timeout_ms": 0}),
            )
            .await,
        );
    }
    // The batch card allows the enveloped item and declines the omitted one.
    {
        let f = fixture().await;
        let id = f.request(&sender, body("batch-omitted")).await;
        f.ok(
            &phone(DEVICE),
            "consent.answer",
            answer(&id, "batch-omitted"),
        )
        .await;
        files.insert(
            "filer-await-partial.json".into(),
            capture(
                &f,
                &sender,
                "consent.await",
                json!({"elicitation_id": id, "timeout_ms": 0}),
            )
            .await,
        );
    }
    // A whole decline records every item as declined without duration or hash.
    {
        let f = fixture().await;
        let id = f.request(&sender, body("approval")).await;
        f.ok(
            &phone(DEVICE),
            "consent.answer",
            json!({"elicitationId": id, "choiceId": "decline", "clientFeatures": ["secret-use-approval"]}),
        )
        .await;
        files.insert(
            "filer-await-declined.json".into(),
            capture(
                &f,
                &sender,
                "consent.await",
                json!({"elicitation_id": id, "timeout_ms": 0}),
            )
            .await,
        );
    }
    // Advancing the test clock past the earliest expiry settles the card.
    {
        let f = fixture().await;
        let id = f.request(&sender, body("approval")).await;
        let expiry = decoded(&body("approval")["secret_use"]["items"][0]["statement_b64url"])
            ["request_expires_at_ms"]
            .as_i64()
            .unwrap();
        f.clock.set(expiry);
        files.insert(
            "filer-await-expired.json".into(),
            capture(
                &f,
                &sender,
                "consent.await",
                json!({"elicitation_id": id, "timeout_ms": 0}),
            )
            .await,
        );
    }
    // Four filings the handler refuses without storing a card.
    {
        // A statement naming a different agent than the filing stamp.
        let mismatched = {
            let f = fixture().await;
            capture(
                &f,
                &filer("aft", Some("another-agent")),
                "consent.request",
                body("approval"),
            )
            .await
        };
        // Strict unpadded base64url over canonical JSON: pretty bytes fail.
        let non_canonical = {
            let f = fixture().await;
            let statement =
                decoded(&body("approval")["secret_use"]["items"][0]["statement_b64url"]);
            let mut request = body("approval");
            request["secret_use"]["items"][0]["statement_b64url"] =
                json!(URL_SAFE_NO_PAD.encode(serde_json::to_string_pretty(&statement).unwrap()));
            capture(&f, &sender, "consent.request", request).await
        };
        // Reusing a request id on a different card conflicts.
        let conflict = {
            let f = fixture().await;
            f.request(&sender, body("approval")).await;
            capture(&f, &sender, "consent.request", body("batch-omitted")).await
        };
        // Only aft and cerebellum may file secret_use; broca is refused.
        let broca = {
            let f = fixture().await;
            capture(
                &f,
                &filer("broca", Some("agent-A")),
                "consent.request",
                body("approval"),
            )
            .await
        };
        let cases = vec![
            ("agent-mismatch", mismatched),
            ("non-canonical", non_canonical),
            ("request-conflict", conflict),
            ("broca-refused", broca),
        ]
        .into_iter()
        .map(|(name, bytes)| {
            let value: Value = serde_json::from_slice(&bytes).unwrap();
            json!({"name": name, "method": value["method"], "params": value["params"], "reply": value["reply"]})
        })
        .collect::<Vec<_>>();
        files.insert(
            "filer-refusals.json".into(),
            serde_json::to_vec(&json!({"cases": cases})).unwrap(),
        );
    }
    files
}
