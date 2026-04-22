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
use crate::error::{LogdiveError, Result};

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

/// Aggregate metadata about the contents of an index.
///
/// Produced by [`Indexer::stats`] and consumed by the CLI `stats` subcommand
/// (milestone 7) and the `GET /stats` HTTP endpoint (milestone 8). The shape
/// is intentionally minimal and structural; the CLI and HTTP layers format
/// it for human or machine consumption.
///
/// `tags` ordering: `None` (untagged rows) first, then non-null tag strings
/// in ascending alphabetical order. This ordering is produced directly by
/// SQLite (`ORDER BY tag` places NULL first in ascending order) and is not
/// re-sorted in Rust. The CLI renders the `None` slot as "(untagged)".
///
/// Marked `#[non_exhaustive]` so additional summary fields (e.g. distinct
/// level counts) can be added in later milestones without breaking the
/// public API.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Stats {
    /// Total number of rows currently in the `log_entries` table.
    pub entries: u64,
    /// Lexically smallest `timestamp` value in the index, or `None` on an
    /// empty database. Lexical ordering is correct for ISO-8601 timestamps;
    /// see the "live design decisions" section of the project handoff.
    pub min_timestamp: Option<String>,
    /// Lexically largest `timestamp` value in the index, or `None` on an
    /// empty database.
    pub max_timestamp: Option<String>,
    /// Distinct tag values observed across all rows. `None` represents rows
    /// with no tag (SQL NULL) and — when present — is always the first
    /// element; non-null tags follow in ascending alphabetical order.
    pub tags: Vec<Option<String>>,
}

/// Owning handle over a SQLite connection to a logdive index.
#[derive(Debug)]
pub struct Indexer {
    conn: Connection,
}

impl Indexer {
    /// Open (or create) a logdive index at `path`.
    ///
    /// Creates the parent directory if it does not already exist, opens the
    /// SQLite database, and runs idempotent schema migrations.
    pub fn open(path: &Path) -> Result<Self> {
        ensure_parent_dir(path)?;
        let conn = Connection::open(path)?;
        init_schema(&conn)?;
        Ok(Self { conn })
    }

    /// Open an in-memory index. Used by tests; also usable for one-shot
    /// scenarios that don't need persistence.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        init_schema(&conn)?;
        Ok(Self { conn })
    }

    /// Borrow the underlying connection.
    ///
    /// Exposed so the query executor can run reads without an extra
    /// abstraction layer. Read-only borrow keeps ingestion and querying
    /// from contending over `&mut`.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Insert a slice of entries into the index, chunking internally into
    /// transactions of [`BATCH_SIZE`] rows each.
    ///
    /// Returns aggregate stats across all chunks. Entry ordering within
    /// the index is not guaranteed.
    pub fn insert_batch(&mut self, entries: &[LogEntry]) -> Result<InsertStats> {
        let mut total = InsertStats::default();
        for chunk in entries.chunks(BATCH_SIZE) {
            let stats = insert_one_chunk(&mut self.conn, chunk)?;
            total.extend(stats);
        }
        Ok(total)
    }

    /// Read aggregate metadata about the index.
    ///
    /// Runs three read-only queries:
    /// 1. `COUNT(*)` for the row count,
    /// 2. `MIN(timestamp), MAX(timestamp)` for the time range,
    /// 3. `SELECT DISTINCT tag ... ORDER BY tag` for the tag list.
    ///
    /// On an empty database, returns `entries = 0`, both timestamp bounds
    /// as `None`, and an empty `tags` vector — not an error.
    pub fn stats(&self) -> Result<Stats> {
        // COUNT(*) is always non-negative; cast i64 → u64 is well-defined.
        let entries_i64: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM log_entries", [], |row| row.get(0))?;
        let entries = entries_i64 as u64;

        // Aggregates without GROUP BY always yield exactly one row; MIN/MAX
        // on an empty table return (NULL, NULL), which maps cleanly to
        // (None, None) via rusqlite's Option<T> FromSql impl.
        let (min_timestamp, max_timestamp): (Option<String>, Option<String>) =
            self.conn.query_row(
                "SELECT MIN(timestamp), MAX(timestamp) FROM log_entries",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;

        // SQLite's `ORDER BY tag` (default ascending) places NULLs first,
        // which is exactly the ordering contract advertised on `Stats.tags`.
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT tag FROM log_entries ORDER BY tag")?;
        let rows = stmt.query_map([], |row| row.get::<_, Option<String>>(0))?;
        let mut tags: Vec<Option<String>> = Vec::new();
        for row in rows {
            tags.push(row?);
        }

        Ok(Stats {
            entries,
            min_timestamp,
            max_timestamp,
            tags,
        })
    }
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

fn ensure_parent_dir(path: &Path) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() {
        // Relative filename with no directory component ("index.db").
        return Ok(());
    }
    std::fs::create_dir_all(parent).map_err(|io_err| LogdiveError::io_at(parent, io_err))
}

fn init_schema(conn: &Connection) -> Result<()> {
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
    )?;
    Ok(())
}

fn insert_one_chunk(conn: &mut Connection, entries: &[LogEntry]) -> Result<InsertStats> {
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
        idx.insert_batch(&[make_entry("2026-04-20T10:00:00Z", "info", "first")])
            .unwrap();

        let mut no_ts = LogEntry::new(r#"{"level":"warn"}"#);
        no_ts.level = Some("warn".to_string());

        let mixed = vec![
            make_entry("2026-04-20T10:00:00Z", "info", "first"),
            make_entry("2026-04-20T10:00:05Z", "error", "second"),
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
        let total = BATCH_SIZE + 337;
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

    #[test]
    fn io_error_variant_attaches_parent_path() {
        // If the parent directory cannot be created (e.g. because it lives
        // under a regular file), we should get LogdiveError::Io with the
        // offending path, not a SqliteFailure.
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, b"not a directory").unwrap();
        let bad_db = blocker.join("child").join("index.db");

        let err = Indexer::open(&bad_db).unwrap_err();
        match err {
            LogdiveError::Io { path, .. } => {
                assert!(path.starts_with(dir.path()));
            }
            other => panic!("expected Io variant, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------
    // stats()
    // -----------------------------------------------------------------

    #[test]
    fn stats_empty_database_returns_zeroed_values() {
        let idx = Indexer::open_in_memory().unwrap();
        let stats = idx.stats().unwrap();

        assert_eq!(stats.entries, 0);
        assert_eq!(stats.min_timestamp, None);
        assert_eq!(stats.max_timestamp, None);
        assert!(stats.tags.is_empty());
    }

    #[test]
    fn stats_counts_entries() {
        let mut idx = Indexer::open_in_memory().unwrap();
        let entries: Vec<_> = (0..5)
            .map(|i| make_entry("2026-04-20T10:00:00Z", "info", &format!("msg-{i}")))
            .collect();
        idx.insert_batch(&entries).unwrap();

        let stats = idx.stats().unwrap();
        assert_eq!(stats.entries, 5);
    }

    #[test]
    fn stats_timestamp_range_uses_lexical_min_and_max() {
        let mut idx = Indexer::open_in_memory().unwrap();
        // Insert intentionally out-of-order to confirm MIN/MAX, not insertion
        // order, drives the bounds.
        idx.insert_batch(&[
            make_entry("2026-04-22T15:30:00Z", "error", "second"),
            make_entry("2026-04-20T10:00:00Z", "info", "first"),
            make_entry("2026-04-21T12:00:00Z", "warn", "third"),
        ])
        .unwrap();

        let stats = idx.stats().unwrap();
        assert_eq!(stats.min_timestamp.as_deref(), Some("2026-04-20T10:00:00Z"));
        assert_eq!(stats.max_timestamp.as_deref(), Some("2026-04-22T15:30:00Z"));
    }

    #[test]
    fn stats_distinct_tags_place_untagged_first_then_alphabetical() {
        let mut idx = Indexer::open_in_memory().unwrap();

        // One untagged row.
        let untagged = make_entry("2026-04-20T10:00:00Z", "info", "untagged-msg");

        // Two distinct rows sharing tag "api" — must collapse via DISTINCT.
        let mut api1 = make_entry("2026-04-20T10:00:01Z", "info", "api-msg-1");
        api1.tag = Some("api".to_string());
        let mut api2 = make_entry("2026-04-20T10:00:02Z", "info", "api-msg-2");
        api2.tag = Some("api".to_string());

        // One row with tag "payments".
        let mut payments = make_entry("2026-04-20T10:00:03Z", "info", "payments-msg");
        payments.tag = Some("payments".to_string());

        idx.insert_batch(&[untagged, api1, api2, payments]).unwrap();

        let stats = idx.stats().unwrap();
        assert_eq!(stats.tags.len(), 3);
        // NULL comes first in SQLite's ascending sort.
        assert_eq!(stats.tags[0], None);
        assert_eq!(stats.tags[1], Some("api".to_string()));
        assert_eq!(stats.tags[2], Some("payments".to_string()));
    }

    #[test]
    fn stats_entries_count_respects_dedup() {
        let mut idx = Indexer::open_in_memory().unwrap();
        // Two batches of the same entry — second is deduplicated away.
        idx.insert_batch(&[make_entry("2026-04-20T10:00:00Z", "info", "dup")])
            .unwrap();
        idx.insert_batch(&[make_entry("2026-04-20T10:00:00Z", "info", "dup")])
            .unwrap();

        let stats = idx.stats().unwrap();
        assert_eq!(stats.entries, 1);
    }

    #[test]
    fn stats_entries_count_excludes_timestamp_less_entries() {
        let mut idx = Indexer::open_in_memory().unwrap();

        let mut no_ts = LogEntry::new(r#"{"level":"info"}"#);
        no_ts.level = Some("info".to_string());

        idx.insert_batch(&[make_entry("2026-04-20T10:00:00Z", "info", "present"), no_ts])
            .unwrap();

        let stats = idx.stats().unwrap();
        assert_eq!(stats.entries, 1);
    }
}
