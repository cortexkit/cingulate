#[path = "support/card_settled.rs"]
mod card_settled;

use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs};

#[test]
fn card_settled_v1_1_production_bytes_and_checksums() {
    let root = card_settled::corpus();
    let mut files = card_settled::generate();
    assert_eq!(
        files.len(),
        24,
        "two messages per filer, each with subject, headers and body"
    );
    for (index, filer) in ["aft", "plexus", "basal", "broca"].iter().enumerate() {
        for reader in [*filer, "prefrontal"] {
            let prefix = format!("{filer}.{reader}");
            assert_eq!(
                files[&format!("{prefix}.subject.txt")],
                format!("ck.box_example.event.cingulate.card_settled.v1.{reader}").as_bytes()
            );
            let body: serde_json::Value =
                serde_json::from_slice(&files[&format!("{prefix}.body.json")]).unwrap();
            assert_eq!(body.as_object().unwrap().len(), 3);
            assert_eq!(body["seq"], index + 1);
            assert_eq!(
                body["feed"],
                if reader == "prefrontal" {
                    "outcomes"
                } else {
                    "answers"
                }
            );
        }
    }
    files.insert(
        "README.md".into(),
        fs::read(root.join("README.md")).unwrap(),
    );
    let sums: String = files
        .iter()
        .map(|(name, bytes)| format!("{:x}  {name}\n", Sha256::digest(bytes)))
        .collect();
    files.insert("SHA256SUMS".into(), sums.into_bytes());
    let names: BTreeSet<_> = fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, files.keys().cloned().collect());
    for (name, bytes) in files {
        assert_eq!(fs::read(root.join(&name)).unwrap(), bytes, "{name}");
    }
}

#[test]
fn card_settled_v1_historical_bytes_and_checksums() {
    let root = card_settled::corpus().with_file_name("card-settled-v1");
    let sums = fs::read_to_string(root.join("SHA256SUMS")).unwrap();
    let mut count = 0;
    for line in sums.lines() {
        let (hash, name) = line.split_once("  ").unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(fs::read(root.join(name)).unwrap())),
            hash,
            "{name}"
        );
        count += 1;
    }
    assert_eq!(count, 25);
    let row = cingulate_store::CardSettled {
        elicitation_id: "el_000000000001".into(),
        reader: "prefrontal-core".into(),
        feed: "outcomes".into(),
        seq: 1,
    };
    assert!(
        ck_cingulate::card_settled::subject(card_settled::ACCOUNT, &row)
            .ends_with(".prefrontal-core")
    );
    assert_eq!(row.msg_id(), "el_000000000001:prefrontal-core:1");
}

#[test]
fn migrated_pending_prefrontal_pointer_uses_new_bus_subject_and_message_id() {
    use cingulate_store::{
        storage_descriptor, ConsentStore, StorageBackend, MIGRATIONS, MIGRATION_NAMESPACE,
    };
    let dir = tempfile::tempdir().unwrap();
    let descriptor = storage_descriptor(None, dir.path().to_str().unwrap());
    let old = cortexkit_store::open_sqlite(&descriptor).unwrap();
    // The legacy fixture is empty, so migration 14 has no audit rows to normalize.
    old.with_conn(|conn| {
        conn.create_scalar_function(
            "cingulate_migration14_normalize_audit",
            1,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8,
            |ctx| ctx.get::<String>(0),
        )
    })
    .unwrap();
    old.migrate(MIGRATION_NAMESPACE, &MIGRATIONS[..15]).unwrap();
    drop(old);
    let StorageBackend::Sqlite { path } = &descriptor.backend else {
        unreachable!()
    };
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute(
        "INSERT INTO card_settled_outbox VALUES ('pending','prefrontal-core','outcomes',9)",
        [],
    )
    .unwrap();
    drop(conn);
    let (store, _) = ConsentStore::open(&descriptor, 100).unwrap();
    let pending = store
        .card_settled_pending_for_reader("prefrontal", 64)
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(
        ck_cingulate::card_settled::subject(card_settled::ACCOUNT, &pending[0]),
        "ck.box_example.event.cingulate.card_settled.v1.prefrontal"
    );
    assert_eq!(pending[0].msg_id(), "pending:prefrontal:9");
}
