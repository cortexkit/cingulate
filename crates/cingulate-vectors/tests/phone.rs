#[path = "support/phone.rs"]
mod phone;
mod support;

use std::{collections::BTreeSet, fs};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use support::*;

const FILES: [&str; 17] = [
    "answer-already-answered.json",
    "answer-different-choice.json",
    "answer-expired.json",
    "answer-not-found.json",
    "answer-success.json",
    "answer-withdrawn.json",
    "browser-get-pending.json",
    "flow-decision-get-pending.json",
    "flow-install-get-pending.json",
    "list-unchanged.json",
    "other-list-pending.json",
    "package-get-answered.json",
    "package-get-expired.json",
    "package-get-pending.json",
    "package-get-withdrawn.json",
    "package-list-pending.json",
    "README.md",
];

#[tokio::test]
async fn phone_served_vectors_regenerate_byte_for_byte() {
    let generated = phone::generate().await;
    assert_eq!(generated.len(), FILES.len() - 1);
    let expected: BTreeSet<_> = FILES
        .iter()
        .filter(|name| **name != "README.md")
        .map(|name| name.to_string())
        .collect();
    assert_eq!(generated.keys().cloned().collect::<BTreeSet<_>>(), expected);
    for (name, bytes) in generated {
        assert_eq!(
            fs::read(corpus(phone::CORPUS).join(&name)).unwrap(),
            bytes,
            "served vector differs: {name}"
        );
    }
}

#[test]
fn phone_sha256sums_cover_and_match_every_file() {
    let root = corpus(phone::CORPUS);
    let sums = fs::read(root.join("SHA256SUMS")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&sums)),
        "b9e0f4169345550e3c057ae90295bb011e9b1a3c1d2db6c3830feb5ae7f665e6"
    );
    let mut seen = BTreeSet::new();
    for line in std::str::from_utf8(&sums).unwrap().lines() {
        let (hash, name) = line.split_once("  ").unwrap();
        assert!(seen.insert(name.to_owned()), "duplicate hash entry: {name}");
        assert_eq!(
            format!("{:x}", Sha256::digest(fs::read(root.join(name)).unwrap())),
            hash,
            "SHA256SUMS: {name}"
        );
    }
    assert_eq!(seen, FILES.iter().map(|name| name.to_string()).collect());
    seen.insert("SHA256SUMS".into());
    let actual = fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, seen, "unexpected corpus file");
}

async fn phone_error(fixture: &Fixture, method: &str, params: Value, code: &str) {
    let refusal = fixture
        .call(&phone::phone_stamp(), method, params)
        .await
        .unwrap_err();
    assert_eq!(refusal["code"], code);
}

#[tokio::test]
async fn phone_get_and_answer_require_camel_case_not_aliases() {
    let fixture = Fixture::open(None).await;
    let id = fixture
        .request(&stamp("reserved:prefrontal-core"), phone::package_body())
        .await;
    for params in [
        json!({}),
        json!({"elicitation_id":id}),
        json!({"elicitationId":1}),
        json!({"elicitationId":null}),
    ] {
        phone_error(
            &fixture,
            "consent.get",
            params.clone(),
            "elicitation_invalid_request",
        )
        .await;
        phone_error(
            &fixture,
            "consent.answer",
            params,
            "elicitation_invalid_request",
        )
        .await;
    }
    for params in [
        json!({"elicitationId":id}),
        json!({"elicitationId":id,"choice_id":"allow"}),
        json!({"elicitationId":id,"choiceId":false}),
    ] {
        phone_error(
            &fixture,
            "consent.answer",
            params,
            "elicitation_invalid_request",
        )
        .await;
    }
    assert_eq!(
        fixture
            .ok(
                &phone::phone_stamp(),
                "consent.get",
                json!({"elicitationId":id,"elicitation_id":"wrong"})
            )
            .await["state"],
        "pending"
    );
    assert_eq!(
        fixture
            .ok(
                &phone::phone_stamp(),
                "consent.answer",
                json!({"elicitationId":id,"choiceId":"allow","choice_id":"deny"})
            )
            .await["ok"],
        true
    );
}

#[tokio::test]
async fn phone_answer_lifecycle_results_and_unknown_choice_are_distinct() {
    let fixture = Fixture::open(None).await;
    let id = fixture
        .request(&stamp("reserved:prefrontal-core"), phone::package_body())
        .await;
    phone_error(
        &fixture,
        "consent.answer",
        json!({"elicitationId":id,"choiceId":"not-offered"}),
        "elicitation_invalid_choice",
    )
    .await;
    assert_eq!(
        fixture
            .ok(
                &phone::phone_stamp(),
                "consent.get",
                json!({"elicitationId":id})
            )
            .await["state"],
        "pending"
    );
    let answer = fixture
        .ok(
            &phone::phone_stamp(),
            "consent.answer",
            json!({"elicitationId":id,"choiceId":"allow"}),
        )
        .await;
    assert_eq!(answer["ok"], true);
    assert_eq!(answer["alreadyAnswered"], false);
    assert_eq!(answer["elicitation"]["answeredChoiceId"], "allow");
    let repeat = fixture
        .ok(
            &phone::phone_stamp(),
            "consent.answer",
            json!({"elicitationId":id,"choiceId":"allow"}),
        )
        .await;
    assert_eq!(
        repeat,
        json!({"ok":true,"alreadyAnswered":true,"elicitation":answer["elicitation"]})
    );
    for choice in ["deny", "not-offered"] {
        assert_eq!(
            fixture
                .ok(
                    &phone::phone_stamp(),
                    "consent.answer",
                    json!({"elicitationId":id,"choiceId":choice})
                )
                .await,
            json!({"ok":false,"code":"conflict","elicitation":answer["elicitation"]})
        );
    }
    phone_error(
        &fixture,
        "consent.get",
        json!({"elicitationId":"unknown"}),
        "elicitation_not_found",
    )
    .await;
    assert_eq!(
        fixture
            .ok(
                &phone::phone_stamp(),
                "consent.answer",
                json!({"elicitationId":"unknown","choiceId":"allow"})
            )
            .await,
        json!({"ok":false,"code":"not_found"})
    );
}

#[tokio::test]
async fn phone_get_expired_withdrawn_acknowledged_and_retention_boundary() {
    let fixture = Fixture::open(None).await;
    let core = stamp("reserved:prefrontal-core");
    let mut request = phone::package_body();
    request["expires_in_ms"] = json!(60_000);
    let expired = fixture.request(&core, request.clone()).await;
    request["dedup_key"] = json!("withdraw-me");
    let withdrawn = fixture.request(&core, request).await;
    fixture.clock.set(ANSWERED);
    fixture
        .ok(
            &core,
            "consent.withdraw",
            json!({"elicitation_id":withdrawn}),
        )
        .await;
    fixture.clock.set(NOW + 60_000);
    for (id, state, at) in [
        (&expired, "expired", NOW + 60_000),
        (&withdrawn, "withdrawn", ANSWERED),
    ] {
        let card = fixture
            .ok(
                &phone::phone_stamp(),
                "consent.get",
                json!({"elicitationId":id}),
            )
            .await;
        assert_eq!(card["state"], state);
        assert_eq!(card["answeredAtMs"], at);
        assert!(card.get("answeredChoiceId").is_none());
        assert_eq!(
            fixture
                .ok(
                    &phone::phone_stamp(),
                    "consent.answer",
                    json!({"elicitationId":id,"choiceId":"not-offered"})
                )
                .await,
            json!({"ok":false,"code":"conflict","elicitation":card})
        );
    }
    let feed = fixture
        .ok(&core, "consent.answers", json!({"since":0}))
        .await;
    fixture
        .ok(&core, "consent.ack", json!({"cursor":feed["cursor"]}))
        .await;
    phone_error(
        &fixture,
        "consent.get",
        json!({"elicitationId":withdrawn}),
        "elicitation_not_found",
    )
    .await;
    assert_eq!(
        fixture
            .ok(
                &phone::phone_stamp(),
                "consent.answer",
                json!({"elicitationId":withdrawn,"choiceId":"allow"})
            )
            .await["code"],
        "conflict"
    );
    fixture
        .clock
        .set(NOW + 60_000 + cingulate_store::RETENTION_MS);
    phone_error(
        &fixture,
        "consent.get",
        json!({"elicitationId":expired}),
        "elicitation_not_found",
    )
    .await;
    assert_eq!(
        fixture
            .ok(
                &phone::phone_stamp(),
                "consent.answer",
                json!({"elicitationId":expired,"choiceId":"allow"})
            )
            .await,
        json!({"ok":false,"code":"not_found"})
    );
}

#[tokio::test]
async fn phone_list_continuation_validation_null_and_ignored_extra_fields() {
    let fixture = Fixture::open(None).await;
    let full = fixture
        .ok(&phone::phone_stamp(), "consent.list_pending", json!({}))
        .await;
    assert_eq!(
        fixture
            .ok(
                &phone::phone_stamp(),
                "consent.list_pending",
                json!({"continuation":null,"limit":1,"since":9})
            )
            .await,
        full
    );
    for continuation in [
        json!(false),
        json!({}),
        json!({"expiresAtMs":0,"elicitationId":"el_1"}),
        json!({"expires_at_ms":-1,"elicitation_id":"el_1"}),
        json!({"expires_at_ms":0,"elicitation_id":4}),
    ] {
        phone_error(
            &fixture,
            "consent.list_pending",
            json!({"continuation":continuation}),
            "elicitation_invalid_request",
        )
        .await;
    }
}

#[tokio::test]
async fn phone_identity_params_are_ignored_and_cannot_grant_route_authority() {
    let fixture = Fixture::open(None).await;
    let id = fixture
        .request(&stamp("reserved:prefrontal-core"), phone::package_body())
        .await;
    let phone = phone::phone_stamp();
    let listed = fixture.ok(&phone, "consent.list_pending", json!({})).await;
    let fetched = fixture
        .ok(&phone, "consent.get", json!({"elicitationId":id}))
        .await;
    fixture
        .ok(
            &phone,
            "consent.answer",
            json!({"elicitationId":id,"choiceId":"allow"}),
        )
        .await;
    let repeated = fixture
        .ok(
            &phone,
            "consent.answer",
            json!({"elicitationId":id,"choiceId":"allow"}),
        )
        .await;
    let after = fixture.ok(&phone, "consent.list_pending", json!({})).await;
    assert_ne!(listed["revision"], after["revision"]);
    assert_eq!(fetched["requesterLabel"], "reserved:prefrontal-core");
    for name in [
        "principal",
        "requester",
        "requester_id",
        "requesterId",
        "requester_agent_id",
        "requesterAgentId",
        "requesterLabel",
        "owner",
        "subject",
        "agent_id",
        "agentId",
        "head_agent_id",
        "headAgentId",
        "carried_agent_id",
        "carriedAgentId",
        "session",
        "session_id",
        "sessionId",
        "session_ref",
        "sessionRef",
        "parent_session_ref",
        "parentSessionRef",
        "bind_session",
        "bindSession",
        "scope",
        "scope_ref",
        "scopeRef",
        "scope_epoch",
        "scopeEpoch",
        "route",
        "bind",
        "identity",
        "harness",
        "operator_surface",
        "operatorSurface",
        "device_id",
        "deviceId",
        "device_fingerprint",
        "deviceFingerprint",
        "machine_id",
        "machineId",
        "console_machine_id",
        "consoleMachineId",
        "at_console",
        "atConsole",
    ] {
        let forged = json!({name:{"untrusted":"identity","harness":"fed-phone:forged"}});
        assert_eq!(
            fixture
                .ok(&phone, "consent.list_pending", forged.clone())
                .await,
            after,
            "{name}"
        );
        let mut get = forged.clone();
        get["elicitationId"] = json!(id);
        assert_eq!(
            fixture.ok(&phone, "consent.get", get.clone()).await,
            repeated["elicitation"],
            "{name}"
        );
        let mut answer = get.clone();
        answer["choiceId"] = json!("allow");
        assert_eq!(
            fixture.ok(&phone, "consent.answer", answer.clone()).await,
            repeated,
            "{name}"
        );
        for (method, params, code) in [
            (
                "consent.list_pending",
                forged,
                "elicitation_operator_required",
            ),
            ("consent.answer", answer, "elicitation_operator_required"),
            ("consent.get", get, "elicitation_requester_refused"),
        ] {
            assert_eq!(
                fixture
                    .call(&stamp("reserved:callosum"), method, params)
                    .await
                    .unwrap_err()["code"],
                code,
                "{name}"
            );
        }
    }
}
