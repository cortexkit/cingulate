use std::collections::BTreeMap;

use ck_cingulate::Stamp;
use serde_json::{json, Value};
use subc_client_rs::HandlerOutcome;

use crate::support::{app_body, Fixture};

pub const CORPUS: &str = "sealed-text-v1.2";

pub fn filer() -> Stamp {
    crate::support::worker::stamp("aft", subc_protocol::scope::ScopeKind::Head)
}

pub fn desktop() -> Stamp {
    use subc_client_rs::{RouteBindRequest, RouteHandle};
    use subc_protocol::{
        peer::{OperatorApp, RouteBindPeer},
        BindIdentity, Principal, RouteTarget,
    };
    Stamp::from_bind(
        &RouteBindRequest::new(
            RouteHandle::detached(1, 1),
            RouteTarget::ManagementSurface {
                module_id: "cingulate".into(),
            },
            BindIdentity::new("local-desktop", "untrusted-session"),
        )
        .with_principal(Principal::Direct)
        .with_peer(
            RouteBindPeer::new().with_operator_app(Some(OperatorApp::new("alfonso-desktop", 5))),
        ),
    )
}

pub fn request(public_key: &str) -> Value {
    let mut body = app_body();
    body["title"] = json!("Type a one-use sudo password");
    body["prompt"] = json!("Only AFT can open the sealed password. It will use it once.");
    body["inputs"] = json!([{
        "kind":"sealed_text", "id":"password", "label":"Sudo password", "max_bytes":64,
        "hpke":{"suite":cingulate_decode::SEALED_TEXT_SUITE,"public_key":public_key}
    }]);
    body
}

pub fn answer(input: Value) -> Value {
    json!({"elicitationId":"el_1", "choiceId":"allow_once", "expectedRound":0, "inputs":[input]})
}

async fn save(
    files: &mut BTreeMap<String, Vec<u8>>,
    fixture: &Fixture,
    who: &Stamp,
    name: &str,
    method: &str,
    params: Value,
) {
    let wire = serde_json::to_vec(&json!({"method":method,"params":params})).unwrap();
    let outcome = fixture.handler.handle_stamped(who, &wire).await;
    let reply = match outcome {
        HandlerOutcome::Response(bytes) => bytes,
        HandlerOutcome::Error { code, message } => {
            serde_json::to_vec(&json!({"code":code,"message":message})).unwrap()
        }
        other => panic!("{method}: {other:?}"),
    };
    files.insert(format!("{name}-request.json"), wire);
    files.insert(format!("{name}-reply.json"), reply);
}

/// Every wire reply is the untouched real handler output on a fresh scratch store.
pub async fn generate(public_key: &str, input: Value) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let fixture = Fixture::open(None).await;
    let phone = crate::phone::phone_stamp();
    save(
        &mut files,
        &fixture,
        &filer(),
        "filing",
        "consent.request",
        request(public_key),
    )
    .await;
    for (name, who, params) in [
        ("phone-card", phone.clone(), json!({"elicitationId":"el_1"})),
        ("desktop-card", desktop(), json!({"elicitationId":"el_1"})),
    ] {
        save(&mut files, &fixture, &who, name, "consent.get", params).await;
    }
    save(
        &mut files,
        &fixture,
        &phone,
        "approve",
        "consent.answer",
        answer(input.clone()),
    )
    .await;
    save(
        &mut files,
        &fixture,
        &phone,
        "approve-repeat",
        "consent.answer",
        answer(input.clone()),
    )
    .await;
    save(
        &mut files,
        &fixture,
        &filer(),
        "filer-before-release",
        "consent.await",
        json!({"elicitation_id":"el_1","timeout_ms":0}),
    )
    .await;
    save(
        &mut files,
        &fixture,
        &desktop(),
        "desktop-read-back",
        "consent.get",
        json!({"elicitationId":"el_1"}),
    )
    .await;
    save(
        &mut files,
        &fixture,
        &phone,
        "phone-read-back",
        "consent.get",
        json!({"elicitationId":"el_1"}),
    )
    .await;
    save(
        &mut files,
        &fixture,
        &filer(),
        "release",
        "consent.release_inputs",
        json!({"elicitation_id":"el_1"}),
    )
    .await;
    save(
        &mut files,
        &fixture,
        &filer(),
        "release-repeat",
        "consent.release_inputs",
        json!({"elicitation_id":"el_1"}),
    )
    .await;
    save(
        &mut files,
        &fixture,
        &filer(),
        "filer-after-release",
        "consent.get",
        json!({"elicitation_id":"el_1"}),
    )
    .await;
    let notices: Vec<String> = fixture
        .sql()
        .prepare("SELECT body_json FROM access_notices ORDER BY rowid")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    files.insert(
        "session-notices.json".into(),
        serde_json::to_vec(
            &notices
                .iter()
                .map(|s| serde_json::from_str::<Value>(s).unwrap())
                .collect::<Vec<_>>(),
        )
        .unwrap(),
    );
    files.insert(
        "pointer-notices.json".into(),
        serde_json::to_vec(&fixture.payloads()).unwrap(),
    );

    let decline = Fixture::open(None).await;
    decline.request(&filer(), request(public_key)).await;
    save(
        &mut files,
        &decline,
        &phone,
        "decline",
        "consent.answer",
        json!({"elicitationId":"el_1","choiceId":"deny"}),
    )
    .await;

    let mut bad_suite = request(public_key);
    bad_suite["inputs"][0]["hpke"]["suite"] = json!("wrong");
    let mut over_cap = request(public_key);
    over_cap["inputs"][0]["max_bytes"] = json!(4097);
    let mut two = request(public_key);
    let duplicate = two["inputs"][0].clone();
    two["inputs"].as_array_mut().unwrap().push(duplicate);
    let mut secret = request(public_key);
    secret["kind"] = json!("secret_use");
    for (name, body) in [
        ("bad-suite", bad_suite),
        ("max-bytes-out-of-range", over_cap),
        ("two-inputs", two),
        ("input-on-secret-use", secret),
    ] {
        let fixture = Fixture::open(None).await;
        save(
            &mut files,
            &fixture,
            &filer(),
            name,
            "consent.request",
            body,
        )
        .await;
    }
    let mut enc = input.clone();
    enc["enc"] = json!("AQ");
    // Keep the malformed-envelope fixture stable when the valid ciphertext changes.
    enc["ciphertext"] = json!("z2JJ6vGzd582r_Y5Nk1ujSSyNjGPHax8wemNJT19zgUPqY2JBvA");
    let mut long = input;
    long["ciphertext"] = json!(base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        [7u8; 81]
    ));
    for (name, params) in [
        ("wrong-enc-length", answer(enc)),
        ("ciphertext-too-long", answer(long)),
        (
            "missing-input-on-approve",
            json!({"elicitationId":"el_1","choiceId":"allow_once"}),
        ),
    ] {
        let fixture = Fixture::open(None).await;
        fixture.request(&filer(), request(public_key)).await;
        save(&mut files, &fixture, &phone, name, "consent.answer", params).await;
    }
    files
}
