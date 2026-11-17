use super::*;
use cingulate_store::CardSettled;

#[test]
fn prefrontal_reader_migration_preserves_publication_counts_and_remaps_pending() {
    let scratch = Scratch::new();
    let old = cortexkit_store::open_sqlite(&scratch.descriptor).unwrap();
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
    old.migrate(MIGRATION_NAMESPACE, &cingulate_store::MIGRATIONS[..15])
        .unwrap();
    drop(old);
    let conn = scratch.sql();
    conn.execute_batch(
        "INSERT INTO card_settled_outbox VALUES ('published','prefrontal-core','outcomes',1);
         INSERT INTO card_settled_outbox VALUES ('pending','prefrontal-core','outcomes',2);
         INSERT INTO card_settled_outbox VALUES ('other','aft','answers',2);",
    )
    .unwrap();
    // Version 15 records acknowledged publication by deleting its pointer and
    // incrementing the durable counter, not by retaining a history row.
    let published = CardSettled {
        elicitation_id: "published".into(),
        reader: "prefrontal-core".into(),
        feed: "outcomes".into(),
        seq: 1,
    };
    let published_id = published.msg_id();
    let published_body = published.body();
    conn.execute_batch(
        "BEGIN;
         DELETE FROM card_settled_outbox WHERE elicitation_id='published';
         UPDATE card_settled_counters SET published=7,dropped=3 WHERE reader='prefrontal-core';
         COMMIT;",
    )
    .unwrap();
    drop(conn);

    let (store, _) = scratch.open(100);
    let pending = store
        .card_settled_pending_for_reader("prefrontal", 64)
        .unwrap();
    assert_eq!(
        pending,
        vec![CardSettled {
            elicitation_id: "pending".into(),
            reader: "prefrontal".into(),
            feed: "outcomes".into(),
            seq: 2
        }]
    );
    assert_eq!(pending[0].msg_id(), "pending:prefrontal:2");
    assert!(store
        .card_settled_pending_for_reader("prefrontal-core", 64)
        .unwrap()
        .is_empty());
    assert_eq!(
        store
            .card_settled_pending_for_reader("aft", 64)
            .unwrap()
            .len(),
        1
    );
    let counts = store.card_settled_counts().unwrap()[4];
    assert_eq!(
        (counts.pending, counts.published, counts.dropped),
        (1, 7, 3)
    );
    store.card_settled_published(&pending[0]).unwrap();
    let counts = store.card_settled_counts().unwrap()[4];
    assert_eq!(
        (counts.pending, counts.published, counts.dropped),
        (0, 8, 3)
    );
    assert_eq!(published_id, "published:prefrontal-core:1");
    assert_eq!(
        published_body,
        br#"{"elicitation_id":"published","feed":"outcomes","seq":1}"#
    );
    assert!(store
        .card_settled_pending(64)
        .unwrap()
        .iter()
        .all(|row| row.elicitation_id != "published"));
    let conn = scratch.sql();
    assert!(conn
        .execute(
            "INSERT INTO card_settled_outbox VALUES ('rollback','prefrontal-core','outcomes',3)",
            []
        )
        .is_ok());
    assert!(conn
        .execute(
            "INSERT INTO card_settled_outbox VALUES ('invalid','unknown','outcomes',4)",
            []
        )
        .is_err());
    drop(conn);
    drop(store);
    let (store, _) = scratch.open(101);
    assert_eq!(store.card_settled_counts().unwrap()[4].published, 8);
}
