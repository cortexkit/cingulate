#[path = "support/access.rs"]
mod access;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs};
#[tokio::test]
async fn permission_served_vectors_regenerate_byte_for_byte() {
    let generated = access::generate().await;
    assert_eq!(generated.len(), 40);
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
        "7799d7074c24a5089a1f20db9ae00f85c2d77784ff4ab539d021d3c2042b9547"
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
    assert_eq!(listed.len(), 41);
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
