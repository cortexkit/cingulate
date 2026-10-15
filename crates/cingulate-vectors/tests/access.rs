#[path = "support/access.rs"]
mod access;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs};
#[tokio::test]
async fn permission_served_vectors_regenerate_byte_for_byte() {
    let generated = access::generate().await;
    assert_eq!(generated.len(), 52);
    for (name, bytes) in generated {
        assert_eq!(
            fs::read(access::root().join(&name)).unwrap(),
            bytes,
            "served bytes: {name}"
        );
    }
}
#[test]
fn permission_manifest_pins_and_covers_exact_bytes() {
    let root = access::root();
    let sums = fs::read(root.join("SHA256SUMS")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&sums)),
        "3ea60d982ebc5c0893ab6fb72795f7905b5386e5cfd1fbc5e1175a644cd69df5"
    );
    let mut listed = BTreeSet::new();
    for line in std::str::from_utf8(&sums).unwrap().lines() {
        let (hash, name) = line.split_once("  ").unwrap();
        assert!(listed.insert(name.to_owned()));
        assert_eq!(
            format!("{:x}", Sha256::digest(fs::read(root.join(name)).unwrap())),
            hash,
            "{name}"
        );
    }
    let actual: BTreeSet<_> = fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|name| name != "SHA256SUMS")
        .collect();
    assert_eq!(actual, listed);
    assert_eq!(listed.len(), 53);
}

fn served(files: &std::collections::BTreeMap<String, Vec<u8>>, name: &str) -> Value {
    serde_json::from_slice(&files[&format!("access-escalated-phone-{name}-reply.json")]).unwrap()
}

fn decode_phone_card(card: &Value) -> Result<(), String> {
    for field in ["prompt", "state", "kind"] {
        if card[field].as_str().is_none_or(str::is_empty) {
            return Err(format!("phone card requires {field}"));
        }
    }
    if card["subject"]["label"].as_str().is_none_or(str::is_empty) {
        return Err("phone card requires subject.label".into());
    }
    let options = card["options"]
        .as_array()
        .ok_or("phone card requires options")?;
    if options.is_empty()
        || options.iter().any(|option| {
            ["id", "label"]
                .iter()
                .any(|key| option[key].as_str().is_none_or(str::is_empty))
        })
    {
        return Err("phone card requires labeled options".into());
    }
    Ok(())
}

#[tokio::test]
async fn access_escalated_phone_decodes_and_allow_once_settles() {
    let files = access::generate().await;
    let get = served(&files, "get");
    let card = &get["result"];
    let list = served(&files, "list");
    let listed = list["result"]["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["elicitationId"] == card["elicitationId"])
        .unwrap();
    for card in [card, listed] {
        decode_phone_card(card).expect("escalated tool permission must decode on shipped phones");
        assert_eq!(card["prompt"], "An AFT worker wants to read 1 path: /x/y");
        assert_eq!(card["title"], "Worker permission");
        // Shipped phones keep only cards whose state is exactly "pending".
        assert_eq!(card["state"], "pending");
        assert_eq!(card["subject"]["label"], "agent");
        assert_eq!(card["requesterLabel"], "reserved:aft");
        assert_eq!(card["requesterName"], "AFT");
        assert_eq!(
            card["options"],
            serde_json::json!([
                {"id":"allow_once","label":"Allow once","effect":"grant"},
                {"id":"allow_scope","label":"Allow for this task","effect":"grant"},
                {"id":"deny","label":"Don't allow","effect":"decline"}
            ])
        );
        assert_eq!(
            card["facts"],
            serde_json::json!([
                {"label":"Operation","value":"read"},
                {"label":"Paths","value":"/x/y"},
                {"label":"Reason","value":"Inspect the requested path"},
                {"label":"Pattern","value":"/x/y"},
                {"label":"Lead agent's note","value":"Please approve this read"}
            ])
        );
        assert_eq!(card["default"], "deny");
        assert_eq!(card["createdAtMs"], 1000);
    }
    let answer = served(&files, "answer");
    assert_eq!(answer["result"]["ok"], true);
    assert_eq!(answer["result"]["elicitation"]["state"], "settled");
    assert_eq!(answer["result"]["elicitation"]["outcome"], "allow_once");
    assert_eq!(answer["result"]["elicitation"]["requesterName"], "AFT");
    let readback = served(&files, "readback");
    assert_eq!(readback["result"]["outcome"], "allow_once");
    assert_eq!(readback["result"]["answered_by"], "user");
    for key in ["prompt", "title", "subject", "facts"] {
        assert!(
            readback["result"].get(key).is_none(),
            "phone-only field {key}"
        );
    }
}

#[tokio::test]
async fn access_escalated_phone_places_card_under_head_agent() {
    let files = access::generate().await;
    let get = served(&files, "get");
    let list = served(&files, "list");
    let listed = list["result"]["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["elicitationId"] == get["result"]["elicitationId"])
        .unwrap();
    for card in [&get["result"], listed] {
        assert_eq!(card["subject"]["headAgentId"], "agent");
        assert_eq!(card["subject"]["agentId"], "agent");
        assert_eq!(card["subject"]["viaWorker"], true);
        assert!(card["asker"].get("agent_id").is_none());
    }
}

struct NamedResolver;
#[async_trait::async_trait]
impl ck_cingulate::Resolver for NamedResolver {
    async fn resolve(&self, agent_id: &str) -> Result<Option<ck_cingulate::ResolvedAgent>, String> {
        assert_eq!(agent_id, "agent");
        Ok(Some(ck_cingulate::ResolvedAgent {
            name: Some("ALF".into()),
            retired: false,
        }))
    }
}

#[tokio::test]
async fn access_phone_subject_uses_generic_consent_name_resolver() {
    let files = access::generate_with_resolver(std::sync::Arc::new(NamedResolver)).await;
    assert_eq!(served(&files, "get")["result"]["subject"]["label"], "ALF");
    assert_eq!(
        served(&files, "answer")["result"]["elicitation"]["subject"]["label"],
        "ALF"
    );
    let list = served(&files, "list");
    for card in list["result"]["records"].as_array().unwrap() {
        assert_eq!(card["subject"]["label"], "ALF");
    }
}
#[test]
fn pinned_scope_grant_names_the_worker_and_hidden_leg_has_no_card() {
    let root = access::root();
    let get = |name| serde_json::from_slice::<Value>(&fs::read(root.join(name)).unwrap()).unwrap();
    let grant = get("phone-settled-reply.json");
    assert_eq!(
        grant["result"]["elicitation"]["asker"]["scope_ref"],
        "worker"
    );
    assert_eq!(grant["result"]["elicitation"]["outcome"], "allow_scope");
    let hidden = get("phone-head-leg-reply.json");
    assert_eq!(hidden["result"]["code"], "access_wrong_leg");
    assert!(hidden["result"].get("elicitation").is_none());
}
