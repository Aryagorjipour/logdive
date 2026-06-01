//! Concurrent CLI ingest tests.
//!
//! These tests spawn two `logdive ingest` processes against the same database
//! file simultaneously and verify that:
//!
//! - Neither process crashes or corrupts the database (SQLite's file-locking
//!   serializes concurrent writers; one process waits while the other commits).
//! - Deduplication is preserved: ingesting the same file from two processes
//!   results in each unique row being stored exactly once.

use std::process::Command;

use logdive_core::Indexer;

const LOGDIVE: &str = env!("CARGO_BIN_EXE_logdive");

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn write_json_log(path: &std::path::Path, n: usize) {
    use std::io::Write;
    let mut f = std::fs::File::create(path).expect("create log file");
    for i in 0..n {
        writeln!(
            f,
            r#"{{"timestamp":"2026-04-20T{:02}:{:02}:00Z","level":"info","message":"entry-{i}"}}"#,
            i / 60,
            i % 60,
        )
        .expect("write log line");
    }
}

fn ingest(db: &std::path::Path, log: &std::path::Path) -> std::process::Child {
    Command::new(LOGDIVE)
        .args([
            "--db",
            db.to_str().unwrap(),
            "ingest",
            "--file",
            log.to_str().unwrap(),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn logdive ingest")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn concurrent_ingest_same_file_no_data_corruption() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    let log = dir.path().join("app.log");

    // Create the DB schema first so both processes open an existing file.
    let _ = Indexer::open(&db).unwrap();

    // Write 60 unique JSON log entries.
    write_json_log(&log, 60);

    // Launch two ingest processes simultaneously, both reading the same file.
    let mut p1 = ingest(&db, &log);
    let mut p2 = ingest(&db, &log);

    let s1 = p1.wait().expect("wait p1");
    let s2 = p2.wait().expect("wait p2");

    assert!(s1.success(), "first ingest process must exit 0");
    assert!(s2.success(), "second ingest process must exit 0");

    // The DB must be intact and readable.
    let idx = Indexer::open_read_only(&db).expect("open_read_only after concurrent ingest");
    let stats = idx.stats().expect("stats after concurrent ingest");

    // Each of the 60 lines is unique, so dedup ensures exactly 60 rows.
    assert_eq!(
        stats.entries, 60,
        "60 unique rows must be present; got {}",
        stats.entries
    );
}

#[test]
fn concurrent_ingest_same_file_twice_dedup_wins() {
    // Both processes ingest the same 30-entry file. Whether one or both
    // processes complete, dedup must prevent any row appearing more than once.
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("dedup.db");
    let log = dir.path().join("dedup.log");

    let _ = Indexer::open(&db).unwrap();
    write_json_log(&log, 30);

    let mut p1 = ingest(&db, &log);
    let mut p2 = ingest(&db, &log);
    p1.wait().unwrap();
    p2.wait().unwrap();

    let idx = Indexer::open_read_only(&db).unwrap();
    let stats = idx.stats().unwrap();

    assert!(
        stats.entries <= 30,
        "dedup must prevent row count exceeding unique entry count; got {}",
        stats.entries
    );
    assert!(
        stats.entries > 0,
        "at least one process must have inserted rows; got {}",
        stats.entries
    );
}
