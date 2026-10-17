//! The Engram backup catalog is written by the module's own startup.
//!
//! Engram backs up a module only if its data folder holds `engram-catalog.json`
//! naming the store, so these tests pin the bytes, the mode, the no-churn
//! behaviour on the second start, replacement of a stale file, and that an
//! unwritable location costs a warning rather than the module's health.

use std::{
    fs::{self, FileTimes},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};

use cingulate_store::storage_descriptor;
use ck_cingulate::{Handler, Options};
use subc_protocol::{session::HealthStatus, ModuleHelloAckBody};
use tempfile::TempDir;

/// The exact catalog bytes, written out here rather than taken from the writer
/// so a change to the document cannot pass by agreeing with itself.
const EXPECTED: &str = "{\n  \"schema_version\": 1,\n  \"module_id\": \"cingulate\",\n  \"entries\": [\n    {\n      \"entry_id\": \"cingulate/store\",\n      \"class\": \"portable\",\n      \"mechanism\": \"whole-db\",\n      \"path\": \"store.db\",\n      \"writer_interaction\": \"backup-api-live\"\n    }\n  ]\n}\n";

struct Start {
    data_home: TempDir,
}

impl Start {
    fn new() -> Self {
        Self {
            data_home: TempDir::new().expect("scratch data home"),
        }
    }

    fn data_home(&self) -> &str {
        self.data_home.path().to_str().expect("scratch path")
    }

    /// The module's data folder, exactly as the store lays it out.
    fn folder(&self) -> PathBuf {
        self.data_home.path().join("cortexkit/ck-cingulate")
    }

    fn catalog(&self) -> PathBuf {
        self.folder().join("engram-catalog.json")
    }

    /// One module start: an empty handler, then the daemon's hello with the
    /// store descriptor this data home resolves to.
    async fn start(&self) -> Handler {
        let descriptor = storage_descriptor(None, self.data_home());
        let handler = Handler::new(Options {
            sweep_timer: false,
            data_home: self.data_home().to_owned(),
            ..Options::default()
        });
        handler
            .hello_ack(&ModuleHelloAckBody {
                negotiated_ver: 1,
                subc_ops: vec![],
                subc_capabilities: vec![],
                storage: Some(serde_json::to_value(descriptor).expect("descriptor")),
                machine_id: None,
            })
            .await;
        handler
    }
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .expect("catalog metadata")
        .permissions()
        .mode()
        & 0o777
}

#[tokio::test]
async fn startup_on_an_empty_data_folder_creates_the_catalog() {
    let start = Start::new();
    assert!(!start.folder().exists(), "the data folder starts absent");
    let handler = start.start().await;
    assert_eq!(handler.health_report().status, HealthStatus::Ok);
    let bytes = fs::read(start.catalog()).expect("startup writes the catalog");
    assert_eq!(String::from_utf8(bytes).expect("utf8 catalog"), EXPECTED);
    assert_eq!(mode(&start.catalog()), 0o600);
}

#[tokio::test]
async fn second_start_leaves_bytes_and_mtime_unchanged() {
    let start = Start::new();
    let first = start.start().await;
    let bytes = fs::read(start.catalog()).expect("first start writes the catalog");
    let earlier = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    let file = fs::OpenOptions::new()
        .write(true)
        .open(start.catalog())
        .expect("catalog opens");
    file.set_times(FileTimes::new().set_modified(earlier))
        .expect("pin an old modification time");
    drop(file);
    // Release the single-writer lease so the second start is a real start, not
    // a lease retry, and give the clock room to move past the pinned instant.
    drop(first);

    let second = start.start().await;
    assert_eq!(second.health_report().status, HealthStatus::Ok);
    assert_eq!(
        fs::read(start.catalog()).expect("catalog survives the second start"),
        bytes
    );
    assert_eq!(
        fs::metadata(start.catalog())
            .expect("catalog metadata")
            .modified()
            .expect("modification time"),
        earlier,
        "an identical catalog must not be rewritten, or every start would churn its mtime"
    );
}

#[tokio::test]
async fn stale_catalog_is_replaced_with_the_expected_bytes() {
    let start = Start::new();
    fs::create_dir_all(start.folder()).expect("scratch data folder");
    fs::write(
        start.catalog(),
        b"{\"schema_version\":1,\"module_id\":\"cingulate\",\"entries\":[]}",
    )
    .expect("stale catalog");

    start.start().await;

    let bytes = fs::read(start.catalog()).expect("catalog rewritten");
    assert_eq!(String::from_utf8(bytes).expect("utf8 catalog"), EXPECTED);
    assert_eq!(mode(&start.catalog()), 0o600);
}

#[derive(Clone)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .expect("capture lock")
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// An unwritable catalog location must be a warning that names the path and the
/// error, and the module must still serve: a missed backup never stops consent.
#[tokio::test]
async fn unwritable_catalog_location_logs_a_warning_and_startup_continues() {
    let start = Start::new();
    // A directory where the file belongs: the rename can never land, so the
    // catalog write fails while the store itself stays perfectly writable.
    fs::create_dir_all(start.catalog()).expect("blocking directory");
    let captured = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::fmt()
        .with_writer({
            let captured = captured.clone();
            move || Capture(captured.clone())
        })
        .with_ansi(false)
        .without_time()
        .with_max_level(tracing::Level::WARN)
        .finish();

    let handler = {
        let _capture = tracing::subscriber::set_default(subscriber);
        start.start().await
    };

    assert_eq!(handler.health_report().status, HealthStatus::Ok);
    assert!(
        start.catalog().is_dir(),
        "the blocking directory is untouched"
    );
    let logged =
        String::from_utf8(captured.lock().expect("capture lock").clone()).expect("utf8 log output");
    assert!(
        logged.contains("engram backup catalog not written"),
        "a failed catalog write must warn, got: {logged}"
    );
    assert!(
        logged.contains("engram-catalog.json"),
        "the warning must name the path, got: {logged}"
    );
    assert!(
        logged.contains("os error"),
        "the warning must carry the error, got: {logged}"
    );
}
