use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

pub(crate) fn scratch_root(prefix: &str) -> PathBuf {
    scratch_root_at(
        prefix,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    )
}

fn scratch_root_at(prefix: &str, timestamp: u128) -> PathBuf {
    // Clock readings can repeat across test threads; the counter keeps their stores separate.
    let root = std::env::temp_dir().join(format!(
        "{prefix}-{}-{timestamp}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed),
    ));
    std::fs::create_dir(&root).unwrap();
    root
}

#[test]
fn scratch_stores_are_isolated_when_clock_repeats() {
    use crate::{storage_descriptor, ConsentStore};

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let first = scratch_root_at("cingulate-scratch-isolation", timestamp);
    let second = scratch_root_at("cingulate-scratch-isolation", timestamp);
    assert_ne!(first, second, "equal clock readings must not share a root");

    let first_descriptor = storage_descriptor(None, first.to_str().unwrap());
    let second_descriptor = storage_descriptor(None, second.to_str().unwrap());
    let first_store = ConsentStore::open(&first_descriptor, 100).unwrap().0;
    let second_store = ConsentStore::open(&second_descriptor, 100).unwrap().0;
    second_store
        .transaction(|tx| {
            tx.execute_batch(
                "CREATE TABLE scratch_marker (id INTEGER PRIMARY KEY);
                              INSERT INTO scratch_marker VALUES (1)",
            )?;
            Ok(())
        })
        .unwrap();

    drop(first_store);
    std::fs::remove_dir_all(&first).unwrap();
    drop(second_store);
    let reopened = ConsentStore::open(&second_descriptor, 100).unwrap().0;
    reopened
        .read(|conn| {
            assert_eq!(
                conn.query_row("SELECT id FROM scratch_marker", [], |row| row
                    .get::<_, i64>(0))?,
                1
            );
            Ok(())
        })
        .unwrap();
    drop(reopened);
    std::fs::remove_dir_all(&second).unwrap();
}
