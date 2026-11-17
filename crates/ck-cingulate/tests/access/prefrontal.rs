use super::*;

fn owned_stamp(module: &str, owner: &str, kind: ScopeKind) -> Stamp {
    let mut stamp = stamp(module, kind);
    let scope = stamp.scope.as_mut().unwrap();
    scope.owner = principal(owner);
    if let Some(parent) = &mut scope.parent {
        parent.owner = principal(owner);
    }
    stamp
}

#[tokio::test]
async fn prefrontal_aliases_ask_relay_stored_escalation_and_withdraw() {
    for owner in ["prefrontal", "prefrontal-core"] {
        for relay in ["prefrontal", "prefrontal-core"] {
            let f = Fixture::new(true).await;
            let asker = owned_stamp("aft", owner, ScopeKind::Worker);
            let id = f.ask(&asker, request(Some("allow"))).await;
            assert_eq!(
                f.audit(&id)["identity"]["scope_owner"],
                format!("reserved:{owner}")
            );
            let relayer = Stamp::from_principal(Some(&principal(relay)));
            assert_eq!(
                f.ok(
                    &relayer,
                    "consent.answer",
                    json!({
                        "elicitation_id":id,"answer":"escalate","answered_by":head()
                    })
                )
                .await["ok"],
                true
            );
            let other = if relay == "prefrontal" {
                "prefrontal-core"
            } else {
                "prefrontal"
            };
            let other = Stamp::from_principal(Some(&principal(other)));
            assert_eq!(
                f.ok(
                    &other,
                    "consent.answer",
                    json!({
                        "elicitation_id":id,"answer":"deny","answered_by":head()
                    })
                )
                .await["ok"],
                true
            );
            assert_eq!(f.get(&id).await["outcome"], "deny");

            let mut req = request(Some("allow"));
            req["call_key"] = json!("withdraw");
            let id = f.ask(&asker, req).await;
            f.ok(&relayer, "consent.withdraw", json!({"elicitation_id":id}))
                .await;
            assert_eq!(f.get(&id).await["outcome"], "access_withdrawn");
        }
    }
}

#[tokio::test]
async fn prefrontal_aliases_admit_requesters_outcomes_and_worker_scope_owners() {
    for module in ["prefrontal", "prefrontal-core"] {
        let f = Fixture::new(true).await;
        let requester = owned_stamp(module, module, ScopeKind::Worker);
        // A flow install on a worker derives its subject from the supervising head.
        let vector: Value = serde_json::from_str(include_str!(
            "../../../../test-vectors/flow-install-secrets-v2/filing.json"
        ))
        .unwrap();
        let id = f.ask(&requester, vector["params"].clone()).await;
        let notice: Value = serde_json::from_str(&f.card_notices(&id)[0].2).unwrap();
        assert_eq!(notice["fields"]["subject"], "agent-head");
        let mut package = consent_card();
        package["kind"] = json!("package");
        package["package"] = json!({"package":"example","version":1,"code_hash":"a".repeat(64),"script":"private script","manifest_json":"{}"});
        f.ask(&requester, package).await;
        let relayer = Stamp::from_principal(Some(&principal(module)));
        f.ok(&relayer, "consent.outcomes", json!({"since":0})).await;
    }
    let f = Fixture::new(true).await;
    assert!(f
        .call(
            &Stamp::from_principal(Some(&principal("aft"))),
            "consent.outcomes",
            json!({"since":0})
        )
        .await
        .is_err());
}
