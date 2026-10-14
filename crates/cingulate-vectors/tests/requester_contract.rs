#[path = "support/phone.rs"]
#[allow(dead_code)]
mod phone;
#[path = "support/requester.rs"]
mod requester;
mod support;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs};
use support::*;

#[tokio::test]
async fn requester_served_vectors_regenerate_byte_for_byte() {
    let generated = requester::generate().await;
    assert_eq!(generated.len(), 44);
    for (name, bytes) in generated {
        assert_eq!(
            fs::read(corpus(requester::CORPUS).join(&name)).unwrap(),
            bytes,
            "served vector differs: {name}"
        );
    }
}

#[test]
fn requester_sha256sums_cover_and_match_every_file() {
    let root = corpus(requester::CORPUS);
    let sums = fs::read(root.join("SHA256SUMS")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&sums)),
        "e3ff6aab15312cac56f59e95a86b3c5cb42d9eb0eef3ecdd3bff638970ee8ffa"
    );
    let mut names = BTreeSet::new();
    for line in std::str::from_utf8(&sums).unwrap().lines() {
        let (hash, name) = line.split_once("  ").unwrap();
        assert!(names.insert(name.to_string()), "duplicate entry {name}");
        assert_eq!(
            format!("{:x}", Sha256::digest(fs::read(root.join(name)).unwrap())),
            hash,
            "SHA256SUMS: {name}"
        );
    }
    assert_eq!(names.len(), 45);
    assert!(names.contains("README.md"));
    names.insert("SHA256SUMS".into());
    let actual = fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, names);
}

#[tokio::test]
async fn basal_accepts_three_upstream_raw_bodies_unchanged() {
    for ((name, bytes), hash) in requester::DECISIONS.into_iter().zip([
        "d03a4ee9e1154ec382acbb55470adff30b0517a9b1208725d4ac8ebd83b897c6",
        "b98a31398e173a86b90e85d4a97b9c7bab10490c71cfb87f2f7285fa9a2fa0b7",
        "5d06530c266b5fa2a9d6a067e8dc3cd42ffb356dd14054ba9a28413b3da9a8fc",
    ]) {
        assert_eq!(
            format!("{:x}", Sha256::digest(bytes)),
            hash,
            "upstream body changed: {name}"
        );
        let fixture = Fixture::open(None).await;
        let body: Value = serde_json::from_slice(bytes).unwrap();
        let basal = stamp("reserved:basal");
        let id = fixture.request(&basal, body.clone()).await;
        let card = fixture
            .ok(&basal, "consent.get", json!({"elicitation_id":id}))
            .await;
        for (key, value) in body.as_object().unwrap() {
            assert_eq!(&card[key], value, "{name}.{key}");
        }
        assert_eq!(card["requester_label"], "reserved:basal");
        assert_eq!(card["subject"], json!({"label":"operator decision"}));
    }
}

#[tokio::test]
async fn flow_decision_body_validity_belongs_to_requester_not_envelope_decoder() {
    let fixture = Fixture::open(None).await;
    let basal = stamp("reserved:basal");
    for name in ["reconcile", "reenable", "grant_lost"] {
        let mut body = requester::decision_body(name);
        body["flow_decision"] =
            json!({"unknown_variant":true,"arbitrary_payload":[null,0,"verbatim"]});
        let id = fixture.request(&basal, body.clone()).await;
        let card = fixture
            .ok(&basal, "consent.get", json!({"elicitation_id":id}))
            .await;
        assert_eq!(card["flow_decision"], body["flow_decision"]);
    }
    for field in [
        "call_key",
        "executor",
        "module",
        "tool",
        "schema_pin",
        "args_jcs",
        "args_hash",
        "approval_scope",
        "requester_session",
        "agent_id",
        "subject",
        "principal",
        "device_id",
        "harness",
    ] {
        let mut body = requester::decision_body("grant_lost");
        body[field] = json!("unrecognized");
        assert_eq!(
            fixture
                .call(&basal, "consent.request", body)
                .await
                .unwrap_err()["code"],
            "elicitation_invalid_request",
            "{field}"
        );
    }
    for value in [json!(null), json!([]), json!("not an object")] {
        let mut body = requester::decision_body("grant_lost");
        body["flow_decision"] = value;
        assert_eq!(
            fixture
                .call(&basal, "consent.request", body)
                .await
                .unwrap_err()["code"],
            "elicitation_invalid_request"
        );
    }
}

#[tokio::test]
async fn basal_requests_need_no_phone_harness_but_remain_owner_bound() {
    use ck_cingulate::Stamp;
    use subc_client_rs::{RouteBindRequest, RouteHandle};
    use subc_protocol::{BindIdentity, Principal, RouteTarget};
    let bind = RouteBindRequest::new(
        RouteHandle::detached(1, 1),
        RouteTarget::ManagementSurface {
            module_id: "cingulate".into(),
        },
        BindIdentity::new("/scratch", "not-a-phone", "not-attested-session"),
    )
    .with_principal(Principal::Reserved {
        module_id: "basal".into(),
    });
    let basal = Stamp::from_bind(&bind);
    assert!(!basal.operator_surface);
    let fixture = Fixture::open(None).await;
    let id = fixture
        .request(&basal, requester::decision_body("reconcile"))
        .await;
    assert_eq!(
        fixture
            .ok(&basal, "consent.get", json!({"elicitation_id":id}))
            .await["state"],
        "pending"
    );
    assert_eq!(
        fixture
            .call(
                &stamp("reserved:basal"),
                "consent.get",
                json!({"elicitation_id":id})
            )
            .await
            .unwrap_err()["code"],
        "elicitation_not_owner"
    );
    for principal in [
        "reserved:prefrontal-core",
        "reserved:callosum",
        "reserved:unknown",
        "direct",
        "unverified",
    ] {
        assert_eq!(
            fixture
                .call(
                    &stamp(principal),
                    "consent.request",
                    requester::decision_body("reconcile")
                )
                .await
                .unwrap_err()["code"],
            "elicitation_requester_refused",
            "{principal}"
        );
    }
}

#[tokio::test]
async fn requester_cursor_is_global_and_ack_discards_owner_settlements_not_pending() {
    let fixture = Fixture::open(None).await;
    let basal = stamp("reserved:basal");
    let core = stamp("reserved:prefrontal-core");
    let answered = fixture
        .request(&basal, requester::decision_body("grant_lost"))
        .await;
    let withdrawn = fixture
        .request(&basal, requester::decision_body("reenable"))
        .await;
    let pending = fixture
        .request(&basal, requester::decision_body("reconcile"))
        .await;
    let package = fixture.request(&core, phone::package_body()).await;
    fixture
        .ok(
            &phone::phone_stamp(),
            "consent.answer",
            json!({"elicitationId":answered,"choiceId":"check_now"}),
        )
        .await;
    fixture
        .ok(
            &basal,
            "consent.withdraw",
            json!({"elicitation_id":withdrawn}),
        )
        .await;
    fixture
        .ok(
            &phone::phone_stamp(),
            "consent.answer",
            json!({"elicitationId":package,"choiceId":"allow"}),
        )
        .await;
    let feed = fixture
        .ok(&basal, "consent.answers", json!({"since":0}))
        .await;
    assert_eq!(feed["cursor"], 3);
    assert_eq!(feed["records"].as_array().unwrap().len(), 1);
    assert_eq!(feed["records"][0]["elicitation_id"], answered);
    assert!(feed.get("continuation").is_none());
    assert_eq!(
        fixture
            .ok(&basal, "consent.ack", json!({"cursor":feed["cursor"]}))
            .await,
        json!({"ok":true})
    );
    assert_eq!(
        fixture
            .ok(&basal, "consent.answers", json!({"since":0}))
            .await,
        json!({"records":[],"cursor":3})
    );
    for id in [answered, withdrawn] {
        assert_eq!(
            fixture
                .call(&basal, "consent.get", json!({"elicitation_id":id}))
                .await
                .unwrap_err()["code"],
            "elicitation_not_found"
        );
    }
    assert_eq!(
        fixture
            .ok(&basal, "consent.get", json!({"elicitation_id":pending}))
            .await["state"],
        "pending"
    );
    assert_eq!(
        fixture
            .ok(&core, "consent.answers", json!({"since":0}))
            .await["records"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        fixture
            .call(
                &basal,
                "consent.report_execution",
                json!({"elicitation_id":pending,"execution":"executed"})
            )
            .await
            .unwrap_err()["code"],
        "elicitation_invalid_execution"
    );
}

#[tokio::test]
async fn requester_v2_served_vectors_regenerate_byte_for_byte() {
    let generated = requester::generate_v2().await;
    assert_eq!(generated.len(), 54);
    for (name, bytes) in generated {
        assert_eq!(
            fs::read(corpus("consent-requester-v2").join(&name)).unwrap(),
            bytes,
            "served vector differs: {name}"
        );
    }
}

#[test]
fn requester_v2_sha256sums_cover_and_match_every_file() {
    let root = corpus("consent-requester-v2");
    let sums = fs::read(root.join("SHA256SUMS")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&sums)),
        "878bf8f22222af04095b937a12103e7ee9c10a314d0a0cce93bf7d435f23cb7f"
    );
    let mut names = BTreeSet::new();
    for line in std::str::from_utf8(&sums).unwrap().lines() {
        let (hash, name) = line.split_once("  ").unwrap();
        assert!(names.insert(name.to_string()), "duplicate entry {name}");
        assert_eq!(
            format!("{:x}", Sha256::digest(fs::read(root.join(name)).unwrap())),
            hash,
            "SHA256SUMS: {name}"
        );
    }
    assert_eq!(names.len(), 55);
    assert!(names.contains("README.md"));
    names.insert("SHA256SUMS".into());
    let actual = fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, names);
}
