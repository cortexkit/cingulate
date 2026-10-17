//! Engram backup catalog.
//!
//! Engram, the fleet backup module, backs up a module's data folder only when
//! that folder holds an `engram-catalog.json` naming what to capture. This
//! module therefore writes its catalog from the store descriptor on every
//! start, so a fresh machine needs no hand step before its first backup.
//!
//! The catalog says `whole-db`: Engram takes a SQLite online-backup snapshot
//! against the live database, so the `-wal` and `-shm` sidecars and the
//! single-writer lease are already covered by that snapshot and are
//! deliberately not entries of their own.

use std::{
    fs::OpenOptions,
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use cingulate_store::{StorageBackend, StorageDescriptor};
use serde::Serialize;

/// Catalog file name Engram looks for inside a module's data folder.
pub const CATALOG_FILE: &str = "engram-catalog.json";

/// What one start did with the catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogWrite {
    /// The file was missing or differed and was rewritten.
    Written,
    /// The file already held exactly these bytes; it was left untouched so its
    /// modification time does not churn on every start.
    Unchanged,
}

/// The catalog is a typed document, serialised in field order, so the same
/// content always produces the same bytes on every start and every machine.
#[derive(Serialize)]
struct Catalog {
    schema_version: u8,
    module_id: &'static str,
    entries: [Entry; 1],
}

#[derive(Serialize)]
struct Entry {
    entry_id: &'static str,
    class: &'static str,
    mechanism: &'static str,
    path: &'static str,
    writer_interaction: &'static str,
}

/// `path` is relative to the module's data folder, which is also where this
/// catalog lives: the folder that holds `store.db`.
const STORE_ENTRY: Entry = Entry {
    entry_id: "cingulate/store",
    class: "portable",
    mechanism: "whole-db",
    path: "store.db",
    writer_interaction: "backup-api-live",
};

/// The exact bytes Engram should find. Deterministic by construction.
fn catalog_bytes() -> Vec<u8> {
    let catalog = Catalog {
        schema_version: 1,
        module_id: crate::MODULE_ID,
        entries: [STORE_ENTRY],
    };
    let mut bytes = serde_json::to_vec_pretty(&catalog)
        .expect("the backup catalog is a fixed document that always serialises");
    bytes.push(b'\n');
    bytes
}

/// The folder holding both `store.db` and the catalog. It comes from the
/// descriptor's own SQLite path rather than a hard-coded home so the two files
/// can never land in different folders, whichever supervisor supplied the
/// store. A non-SQLite backend has no SQLite file to describe, so it has no
/// catalog to write.
fn data_folder(descriptor: &StorageDescriptor) -> Option<PathBuf> {
    match &descriptor.backend {
        StorageBackend::Sqlite { path } => Path::new(path).parent().map(Path::to_path_buf),
        _ => None,
    }
}

/// Write the catalog for this start, logging but never failing.
///
/// A module that refused to serve because a backup descriptor could not be
/// written would be worse than a missed backup, so the failure is a warning
/// that names the path and the error and startup goes on.
pub fn bootstrap_catalog(descriptor: &StorageDescriptor) {
    let Some(folder) = data_folder(descriptor) else {
        return;
    };
    if let Err(error) = write_catalog(&folder) {
        tracing::warn!(
            path = %folder.join(CATALOG_FILE).display(),
            %error,
            "engram backup catalog not written; Engram will skip this module's store"
        );
    }
}

/// Write `CATALOG_FILE` inside `data_dir` unless it already holds exactly the
/// expected bytes. The folder is created owner-only and the file is written
/// atomically, so a reader never sees a partial catalog.
pub fn write_catalog(data_dir: &Path) -> io::Result<CatalogWrite> {
    let path = data_dir.join(CATALOG_FILE);
    let bytes = catalog_bytes();
    if std::fs::read(&path).is_ok_and(|existing| existing == bytes) {
        return Ok(CatalogWrite::Unchanged);
    }
    // The store folder is owner-only; creating it here, before the store opens,
    // must not leave a listable folder behind for the files inside it.
    cortexkit_lease::create_private_dir(data_dir)?;
    write_atomic(&path, &bytes)?;
    Ok(CatalogWrite::Written)
}

/// Same-folder temp file, fsync, then rename over the target: an interrupted
/// start leaves either the old catalog or the new one, never a truncated one.
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let folder = path
        .parent()
        .ok_or_else(|| invalid_input("catalog path has no parent folder"))?;
    let temp = folder.join(temp_name());
    // `create_new` plus owner-only mode from creation: the content is never
    // readable by another account, not even for the instant before the rename.
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options.open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, path)?;
        sync_folder(folder);
        Ok(())
    })();
    if result.is_err() {
        // Best effort: a leftover temp file must not turn one bad start into a
        // per-start growing pile of them.
        let _ = std::fs::remove_file(&temp);
    }
    result
}

fn temp_name() -> String {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(".{CATALOG_FILE}.{}.{stamp}.tmp", std::process::id())
}

fn invalid_input(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

/// Make the rename durable across a power cut. A failure here cannot lose the
/// catalog, because the renamed file is already in place, so it is ignored.
#[cfg(unix)]
fn sync_folder(folder: &Path) {
    if let Ok(handle) = std::fs::File::open(folder) {
        let _ = handle.sync_all();
    }
}

#[cfg(not(unix))]
fn sync_folder(_folder: &Path) {}
