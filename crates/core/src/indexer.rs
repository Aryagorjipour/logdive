//! SQLite-backed index for ingested log entries.
//!
//! This module owns the persistent storage side of logdive: schema creation,
//! row-level deduplication via `blake3`, and batched inserts of 1000 rows per
//! transaction (per the decisions log entry dated 2026-04-19). The schema is
//! reproduced verbatim from the project doc's "SQLite schema" section with
//! `IF NOT EXISTS` added so opening an existing database is idempotent.
//!
//! `Indexer` is an owning handle over a `rusqlite::Connection`. It can be
//! constructed against a filesystem path via [`Indexer::open`] or against an
//! in-memory database via [`Indexer::open_in_memory`] — the latter is used
//! by the unit tests below and will also serve ad-hoc one-shot scenarios.
//!
//! # Timestamp NOT NULL policy
//!
//! The schema declares `timestamp TEXT NOT NULL`, but the parser produces
//! `LogEntry::timestamp = None` for lines that omit the key. Rather than
//! fabricating a fallback (which would falsely anchor those rows to
//! ingestion time and confuse `last Nh` queries), the indexer *skips* such
//! rows and reports them in [`InsertStats::skipped_no_timestamp`]. This
//! mirrors the parser's "graceful skip" philosophy — bad data is counted
//! and dropped, never manufactured.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

use crate::entry::LogEntry;

/// Size of a single insert transaction, per the decisions log
/// (2026-04-19: "batch insert per 1000 lines").
pub const BATCH_SIZE: usize = 1000;

const DEFAULT_DB_FILENAME: &str = "index.db";
const LOGDIVE_HOME_DIRNAME: &str = ".logdive";

/// Resolve the path to the index database.
///
/// When `override_path` is `Some`, it is used verbatim — this is what the
/// CLI's `--db` flag wires into. Otherwise the default `~/.logdive/index.db`
/// is returned per the "Default index location" decision in the project doc.
///
/// Purely functional: does not touch the filesystem.
pub fn db_path(override_path: Option<&Path>) -> PathBuf {
    if let Some(p) = override_path {
        return p.to_path_buf();
    }
    // POSIX-centric: logdive's Phase 4 release targets are Linux and macOS,
    // both of which expose HOME. Fall back to CWD if it is unset (containers,
    // stripped CI environments) rather than panicking.
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(LOGDIVE_HOME_DIRNAME)
        .join(DEFAULT_DB_FILENAME)
}

/// Outcome of an insert batch, surfaced to the CLI for progress output
/// ("lines ingested / lines skipped per second", per milestone 6).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct InsertStats {
    /// Rows newly added to the index.
    pub inserted: usize,
    /// Rows rejected by `INSERT OR IGNORE` because their `raw_hash` already
    /// existed — the dedup path per the decisions log.
    pub deduplicated: usize,
    /// Rows rejected because they had no `timestamp`. See module docs.
    pub skipped_no_timestamp: usize,
}

impl InsertStats {
    fn extend(&mut self, other: InsertStats) {
        self.inserted += other.inserted;
        self.deduplicated += other.deduplicated;
        self.skipped_no_timestamp += other.skipped_no_timestamp;
    }
}

/// Owning handle over a SQLite connection to a logdive index.
pub struct Indexer {
    conn: Connection,
}

impl Indexer {
    /// Open (or create) a logdive index at `path`.
    ///
    /// Creates the parent directory if it does not already exist, opens the
    /// SQLite database, and runs idempotent schema migrations.
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        ensure_parent_dir(path)?;
        let conn = Connection::open(path)?;
        init_schema(&conn)?;
        Ok(Self { conn })
    }

    /// Open an in-memory index. Used by tests; also usable for one-shot
    /// scenarios that don't need persistence.
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        init_schema(&conn)?;
        Ok(Self { conn })
    }

    /// Borrow the underlying connection.
    ///
    /// Exposed so milestone 4's query executor can run reads without an
    /// extra abstraction layer. Read-only borrow keeps ingestion and
    /// querying from contending over `&mut`.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Insert a slice of entries into the index, chunking internally into
    /// transactions of [`BATCH_SIZE`] rows each.
    ///
    /// Returns aggregate stats across all chunks. Entry ordering within
    /// the index is not guaranteed.
    pub fn insert_batch(&mut self, entries: &[LogEntry]) -> rusqlite::Result<InsertStats> {
        let mut total = InsertStats::default();
        for chunk in entries.chunks(BATCH_SIZE) {
            let stats = insert_one_chunk(&mut self.conn, chunk)?;
            total.extend(stats);
        }
        Ok(total)
    }
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

fn ensure_parent_dir(path: &Path) -> rusqlite::Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() {
        // Relative filename with no directory component ("index.db").
        return Ok(());
    }
    std::fs::create_dir_all(parent).map_err(|io_err| {
        // Milestone 5 will replace this with a proper LogdiveError variant.
        // For now, surface as a SqliteFailure with the semantically closest
        // result code (can't open) plus the OS error in the message.
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CANTOPEN),
            Some(format!(
                "failed to create directory {}: {io_err}",
                parent.display()
            )),
        )
    })
}

fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    // Schema taken verbatim from the project doc's "SQLite schema" section,
    // with `IF NOT EXISTS` added on every statement so open() is idempotent.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS log_entries (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp   TEXT NOT NULL,
            level       TEXT,
            message     TEXT,
            tag         TEXT,
            fields      TEXT,
            raw         TEXT NOT NULL,
            raw_hash    TEXT NOT NULL UNIQUE,
            ingested_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_level     ON log_entries(level);
        CREATE INDEX IF NOT EXISTS idx_tag       ON log_entries(tag);
        CREATE INDEX IF NOT EXISTS idx_timestamp ON log_entries(timestamp);",
    )
}

fn insert_one_chunk(conn: &mut Connection, entries: &[LogEntry]) -> rusqlite::Result<InsertStats> {
    let tx = conn.transaction()?;
    let mut stats = InsertStats::default();

    {
        let mut stmt = tx.prepare(
            "INSERT OR IGNORE INTO log_entries
             (timestamp, level, message, tag, fields, raw, raw_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;

        for entry in entries {
            // NOT NULL enforcement — see module-level docs.
            let Some(ref ts) = entry.timestamp else {
                stats.skipped_no_timestamp += 1;
                continue;
            };

            // Serializing a `Map<String, Value>` via serde_json is infallible:
            // every `Value` variant has a defined JSON representation.
            let fields_json = serde_json::to_string(&entry.fields)
                .expect("serializing serde_json::Map<String, Value> is infallible");
            let raw_hash = blake3::hash(entry.raw.as_bytes()).to_hex().to_string();

            let changes = stmt.execute(params![
                ts,
                entry.level,
                entry.message,
                entry.tag,
                fields_json,
                entry.raw,
                raw_hash,
            ])?;

            if changes == 0 {
                stats.deduplicated += 1;
            } else {
                stats.inserted += 1;
            }
        }
    }

    tx.commit()?;
    Ok(stats)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Build a LogEntry whose `raw` is unique per input tuple, guaranteeing
    /// a distinct `raw_hash` across calls (critical for the chunking test
    /// where we insert thousands of entries).
    fn make_entry(ts: &str, level: &str, message: &str) -> LogEntry {
        let raw = format!(r#"{{"timestamp":"{ts}","level":"{level}","message":"{message}"}}"#);
        let mut e = LogEntry::new(raw);
        e.timestamp = Some(ts.to_string());
        e.level = Some(level.to_string());
        e.message = Some(message.to_string());
        e
    }

    #[test]
    fn open_in_memory_creates_table_and_three_indexes() {
        let idx = Indexer::open_in_memory().expect("open in-memory");
        let table_count: i64 = idx
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master \
                 WHERE type='table' AND name='log_entries'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 1);

        let index_count: i64 = idx
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master \
                 WHERE type='index' AND name IN ('idx_level','idx_tag','idx_timestamp')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(index_count, 3);
    }

    #[test]
    fn insert_batch_adds_rows_and_reports_stats() {
        let mut idx = Indexer::open_in_memory().unwrap();
        let entries = vec![
            make_entry("2026-04-20T10:00:00Z", "info", "one"),
            make_entry("2026-04-20T10:00:01Z", "error", "two"),
        ];
        let stats = idx.insert_batch(&entries).unwrap();

        assert_eq!(stats.inserted, 2);
        assert_eq!(stats.deduplicated, 0);
        assert_eq!(stats.skipped_no_timestamp, 0);

        let count: i64 = idx
            .connection()
            .query_row("SELECT COUNT(*) FROM log_entries", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn reinsert_is_deduplicated_by_raw_hash() {
        let mut idx = Indexer::open_in_memory().unwrap();
        let entries = vec![make_entry("2026-04-20T10:00:00Z", "info", "hello")];

        let first = idx.insert_batch(&entries).unwrap();
        assert_eq!(first.inserted, 1);
        assert_eq!(first.deduplicated, 0);

        let second = idx.insert_batch(&entries).unwrap();
        assert_eq!(second.inserted, 0);
        assert_eq!(second.deduplicated, 1);

        let count: i64 = idx
            .connection()
            .query_row("SELECT COUNT(*) FROM log_entries", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn entries_without_timestamp_are_skipped_not_fabricated() {
        let mut idx = Indexer::open_in_memory().unwrap();
        let mut no_ts = LogEntry::new(r#"{"level":"info"}"#);
        no_ts.level = Some("info".to_string());
        // timestamp intentionally left as None.

        let stats = idx.insert_batch(&[no_ts]).unwrap();
        assert_eq!(stats.inserted, 0);
        assert_eq!(stats.skipped_no_timestamp, 1);

        let count: i64 = idx
            .connection()
            .query_row("SELECT COUNT(*) FROM log_entries", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn mixed_batch_counts_each_outcome_category() {
        let mut idx = Indexer::open_in_memory().unwrap();
        // Prime the index with one row so the re-insert in the mixed batch
        // exercises the dedup path.
        idx.insert_batch(&[make_entry("2026-04-20T10:00:00Z", "info", "first")])
            .unwrap();

        let mut no_ts = LogEntry::new(r#"{"level":"warn"}"#);
        no_ts.level = Some("warn".to_string());

        let mixed = vec![
            // Same raw as the primed row → deduplicated.
            make_entry("2026-04-20T10:00:00Z", "info", "first"),
            // Fresh row → inserted.
            make_entry("2026-04-20T10:00:05Z", "error", "second"),
            // No timestamp → skipped.
            no_ts,
        ];
        let stats = idx.insert_batch(&mixed).unwrap();
        assert_eq!(stats.inserted, 1);
        assert_eq!(stats.deduplicated, 1);
        assert_eq!(stats.skipped_no_timestamp, 1);
    }

    #[test]
    fn fields_are_stored_as_json_queryable_via_json_extract() {
        let mut idx = Indexer::open_in_memory().unwrap();
        let mut e = make_entry("2026-04-20T10:00:00Z", "info", "hi");
        e.fields.insert("service".to_string(), json!("payments"));
        e.fields.insert("req_id".to_string(), json!(42));
        idx.insert_batch(&[e]).unwrap();

        let service: String = idx
            .connection()
            .query_row(
                "SELECT json_extract(fields, '$.service') FROM log_entries",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(service, "payments");

        let req_id: i64 = idx
            .connection()
            .query_row(
                "SELECT json_extract(fields, '$.req_id') FROM log_entries",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(req_id, 42);
    }

    #[test]
    fn empty_fields_round_trip_as_empty_json_object_not_null() {
        let mut idx = Indexer::open_in_memory().unwrap();
        idx.insert_batch(&[make_entry("2026-04-20T10:00:00Z", "info", "x")])
            .unwrap();

        let stored: String = idx
            .connection()
            .query_row("SELECT fields FROM log_entries", [], |row| row.get(0))
            .unwrap();
        assert_eq!(stored, "{}");
    }

    #[test]
    fn raw_hash_is_a_64_char_hex_blake3_digest() {
        let mut idx = Indexer::open_in_memory().unwrap();
        idx.insert_batch(&[make_entry("2026-04-20T10:00:00Z", "info", "hash me")])
            .unwrap();

        let stored_hash: String = idx
            .connection()
            .query_row("SELECT raw_hash FROM log_entries", [], |row| row.get(0))
            .unwrap();
        assert_eq!(stored_hash.len(), 64);
        assert!(stored_hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn chunking_handles_batches_larger_than_batch_size() {
        let mut idx = Indexer::open_in_memory().unwrap();
        let total = BATCH_SIZE + 337; // Forces two transactions.
        let entries: Vec<_> = (0..total)
            .map(|i| make_entry("2026-04-20T10:00:00Z", "info", &format!("message-{i}")))
            .collect();

        let stats = idx.insert_batch(&entries).unwrap();
        assert_eq!(stats.inserted, total);
        assert_eq!(stats.deduplicated, 0);

        let count: i64 = idx
            .connection()
            .query_row("SELECT COUNT(*) FROM log_entries", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, total as i64);
    }

    #[test]
    fn db_path_returns_override_verbatim() {
        let p = Path::new("/tmp/logdive-test/override.db");
        assert_eq!(
            db_path(Some(p)),
            PathBuf::from("/tmp/logdive-test/override.db")
        );
    }

    #[test]
    fn db_path_default_ends_with_standard_location() {
        let default = db_path(None);
        assert!(default.ends_with(".logdive/index.db"));
    }

    #[test]
    fn open_creates_parent_directory_and_is_idempotent_across_opens() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("sub").join("dir").join("index.db");

        {
            let mut idx = Indexer::open(&db).expect("first open");
            idx.insert_batch(&[make_entry("2026-04-20T10:00:00Z", "info", "persist me")])
                .unwrap();
        }

        {
            let idx = Indexer::open(&db).expect("second open");
            let count: i64 = idx
                .connection()
                .query_row("SELECT COUNT(*) FROM log_entries", [], |row| row.get(0))
                .unwrap();
            assert_eq!(count, 1);
        }
    }
}
