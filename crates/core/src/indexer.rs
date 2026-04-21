//! SQLite indexing for parsed log entries.
//!
//! Schema and behavioural decisions are fixed in the project doc:
//!
//! - Schema verbatim from the "SQLite schema" section of the doc — the
//!   `log_entries` table plus three indexes on `level`, `tag`, `timestamp`.
//! - Deduplication via `blake3` hashing of the raw line into `raw_hash UNIQUE`
//!   (decisions log, 2026-04-19: "Hash-based deduplication on ingestion
//!   using blake3").
//! - Re-ingestion uses `INSERT OR IGNORE` so duplicate rows are silently
//!   dropped rather than erroring out.
//! - Batch size of 1000 rows per transaction for ingestion throughput
//!   (decisions log, 2026-04-19: "batch insert per 1000 lines").
//! - Hybrid storage: the four known fields are dedicated columns, all
//!   other keys are serialized into the `fields` TEXT column for
//!   `json_extract()`-based querying.
//! - Default DB path `~/.logdive/index.db`, overridable via `--db`.
//!
//! This module does not validate the semantic content of log lines — that
//! is the parser's job. Its sole responsibility is persistence.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

use crate::entry::LogEntry;

/// Maximum number of entries committed in a single transaction during
/// batched ingestion. Matches the decisions-log choice of 1000.
pub const BATCH_SIZE: usize = 1000;

/// Summary of a batch insert. The CLI surfaces these counts as
/// "lines ingested / lines skipped" in its progress output (milestone 6).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct IngestStats {
    /// Rows that were newly written.
    pub inserted: u64,
    /// Rows rejected because their `raw_hash` was already present.
    pub duplicates: u64,
}

impl IngestStats {
    /// Merge two stats values. The CLI's streaming ingest path
    /// accumulates per-flush stats using this.
    pub fn combine(self, other: IngestStats) -> IngestStats {
        IngestStats {
            inserted: self.inserted + other.inserted,
            duplicates: self.duplicates + other.duplicates,
        }
    }
}

/// Errors surfaced by the indexer layer. Milestone 5 consolidates these
/// into the crate-wide `LogdiveError` — for now this enum keeps the
/// module self-contained and lets us return meaningful variants without
/// resorting to `unwrap()` in non-test code.
#[derive(Debug, thiserror::Error)]
pub enum IndexerError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("could not resolve home directory: HOME environment variable is not set")]
    HomeNotSet,

    #[error("failed to serialize entry fields: {0}")]
    FieldsSerialize(#[from] serde_json::Error),
}

/// Module-local `Result` alias. Kept private to avoid shadowing the
/// eventual `logdive_core::Result` from milestone 5.
type Result<T> = std::result::Result<T, IndexerError>;

/// DDL for the `log_entries` table and its indexes.
///
/// Reproduced verbatim from the "SQLite schema" section of the project
/// doc, with `IF NOT EXISTS` added so [`init_schema`] is idempotent.
const SCHEMA_SQL: &str = "\
CREATE TABLE IF NOT EXISTS log_entries (
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
CREATE INDEX IF NOT EXISTS idx_timestamp ON log_entries(timestamp);
";

const INSERT_SQL: &str = "\
INSERT OR IGNORE INTO log_entries
    (timestamp, level, message, tag, fields, raw, raw_hash)
VALUES
    (?1, ?2, ?3, ?4, ?5, ?6, ?7)
";

/// Resolve the database path.
///
/// If `override_path` is provided, that path is returned verbatim.
/// Otherwise the default `$HOME/.logdive/index.db` is used, per the
/// decisions-log entry "Default index location".
pub fn db_path(override_path: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = override_path {
        return Ok(p.to_path_buf());
    }
    let home = std::env::var("HOME").map_err(|_| IndexerError::HomeNotSet)?;
    Ok(default_db_path_from_home(&home))
}

/// Pure helper: given a HOME directory string, return the default index
/// file path. Separated out so unit tests can exercise path construction
/// without touching process env vars.
fn default_db_path_from_home(home: &str) -> PathBuf {
    PathBuf::from(home).join(".logdive").join("index.db")
}

/// Open the SQLite database at `path`, creating the parent directory
/// and schema if necessary.
pub fn open(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let conn = Connection::open(path)?;
    init_schema(&conn)?;
    Ok(conn)
}

/// Apply the schema DDL. Idempotent — safe to call on every open.
pub fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(SCHEMA_SQL)?;
    Ok(())
}

/// Insert entries into the index.
///
/// Internally chunks `entries` into transactions of at most [`BATCH_SIZE`]
/// rows. Duplicate rows (same `raw_hash`) are silently skipped and counted
/// in [`IngestStats::duplicates`]. Entries with no timestamp receive the
/// current UTC time in RFC 3339 format, satisfying the schema's NOT NULL
/// constraint without losing the row.
pub fn insert_batch(conn: &mut Connection, entries: &[LogEntry]) -> Result<IngestStats> {
    let mut total = IngestStats::default();
    for chunk in entries.chunks(BATCH_SIZE) {
        total = total.combine(insert_one_transaction(conn, chunk)?);
    }
    Ok(total)
}

/// Execute a single transaction's worth of inserts. Private — callers
/// go through [`insert_batch`], which guarantees the BATCH_SIZE cap.
fn insert_one_transaction(conn: &mut Connection, entries: &[LogEntry]) -> Result<IngestStats> {
    if entries.is_empty() {
        return Ok(IngestStats::default());
    }

    let tx = conn.transaction()?;
    let mut stats = IngestStats::default();

    {
        let mut stmt = tx.prepare(INSERT_SQL)?;
        for entry in entries {
            let timestamp = entry
                .timestamp
                .clone()
                .unwrap_or_else(current_timestamp_rfc3339);

            let fields_json = if entry.fields.is_empty() {
                None
            } else {
                Some(serde_json::to_string(&entry.fields)?)
            };

            let raw_hash = blake3::hash(entry.raw.as_bytes()).to_hex().to_string();

            let affected = stmt.execute(params![
                timestamp,
                entry.level,
                entry.message,
                entry.tag,
                fields_json,
                entry.raw,
                raw_hash,
            ])?;

            if affected > 0 {
                stats.inserted += 1;
            } else {
                stats.duplicates += 1;
            }
        }
    }

    tx.commit()?;
    Ok(stats)
}

/// Current UTC time formatted as RFC 3339 with millisecond precision
/// and the `Z` zone designator. Matches the format emitted by common
/// structured-logging libraries and is understood by SQLite's datetime
/// functions for range comparison in the query executor (milestone 4).
fn current_timestamp_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use tempfile::tempdir;

    /// In-memory DB with schema already initialized. Cheap, isolated,
    /// never touches the filesystem.
    fn in_memory() -> Connection {
        let conn = Connection::open_in_memory().expect("open memory db");
        init_schema(&conn).expect("init schema");
        conn
    }

    /// Sample entry with a fixed timestamp and level. Unique `raw`
    /// strings yield unique `blake3` hashes → no accidental dedup.
    fn sample_entry(raw: &str, level: &str) -> LogEntry {
        let mut e = LogEntry::new(raw);
        e.timestamp = Some("2026-04-19T10:00:00Z".to_string());
        e.level = Some(level.to_string());
        e
    }

    #[test]
    fn default_db_path_uses_dot_logdive_under_home() {
        let p = default_db_path_from_home("/home/bob");
        assert_eq!(p, PathBuf::from("/home/bob/.logdive/index.db"));
    }

    #[test]
    fn db_path_returns_override_verbatim() {
        let custom = PathBuf::from("/tmp/foo/my.db");
        let resolved = db_path(Some(&custom)).expect("resolve");
        assert_eq!(resolved, custom);
    }

    #[test]
    fn init_schema_is_idempotent() {
        let conn = in_memory();
        init_schema(&conn).expect("second init_schema");
    }

    #[test]
    fn schema_creates_expected_indexes() {
        let conn = in_memory();
        let mut stmt = conn
            .prepare(
                "SELECT name FROM sqlite_master \
                 WHERE type='index' AND name LIKE 'idx_%' \
                 ORDER BY name",
            )
            .unwrap();
        let names: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        assert_eq!(names, vec!["idx_level", "idx_tag", "idx_timestamp"]);
    }

    #[test]
    fn insert_batch_writes_rows() {
        let mut conn = in_memory();
        let entries = vec![
            sample_entry(r#"{"level":"info"}"#, "info"),
            sample_entry(r#"{"level":"warn"}"#, "warn"),
            sample_entry(r#"{"level":"error"}"#, "error"),
        ];

        let stats = insert_batch(&mut conn, &entries).expect("insert");
        assert_eq!(stats.inserted, 3);
        assert_eq!(stats.duplicates, 0);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM log_entries", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 3);
    }

    #[test]
    fn insert_batch_deduplicates_on_reinsert() {
        let mut conn = in_memory();
        let entries = vec![
            sample_entry(r#"{"level":"info"}"#, "info"),
            sample_entry(r#"{"level":"warn"}"#, "warn"),
        ];

        let first = insert_batch(&mut conn, &entries).expect("first");
        assert_eq!(first.inserted, 2);
        assert_eq!(first.duplicates, 0);

        let second = insert_batch(&mut conn, &entries).expect("second");
        assert_eq!(second.inserted, 0);
        assert_eq!(second.duplicates, 2);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM log_entries", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn insert_batch_empty_slice_is_noop() {
        let mut conn = in_memory();
        let stats = insert_batch(&mut conn, &[]).expect("empty");
        assert_eq!(stats, IngestStats::default());

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM log_entries", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn fields_map_is_stored_as_json_blob() {
        let mut conn = in_memory();

        let mut e = sample_entry(r#"{"level":"info","service":"pay"}"#, "info");
        e.fields
            .insert("service".into(), Value::String("pay".into()));
        e.fields.insert("req_id".into(), json!(42));

        insert_batch(&mut conn, &[e]).expect("insert");

        let stored: String = conn
            .query_row("SELECT fields FROM log_entries", [], |r| r.get(0))
            .unwrap();
        let parsed: Value = serde_json::from_str(&stored).unwrap();
        assert_eq!(parsed["service"], json!("pay"));
        assert_eq!(parsed["req_id"], json!(42));
    }

    #[test]
    fn empty_fields_map_stored_as_null() {
        let mut conn = in_memory();
        let e = sample_entry(r#"{"level":"info"}"#, "info");
        insert_batch(&mut conn, &[e]).expect("insert");

        let stored: Option<String> = conn
            .query_row("SELECT fields FROM log_entries", [], |r| r.get(0))
            .unwrap();
        assert!(stored.is_none(), "expected NULL, got {:?}", stored);
    }

    #[test]
    fn missing_timestamp_is_filled_with_current_time() {
        let mut conn = in_memory();

        let mut e = LogEntry::new(r#"{"level":"info"}"#);
        e.level = Some("info".to_string());
        assert!(e.timestamp.is_none());

        insert_batch(&mut conn, &[e]).expect("insert");

        let stored: String = conn
            .query_row("SELECT timestamp FROM log_entries", [], |r| r.get(0))
            .unwrap();
        assert!(
            !stored.is_empty(),
            "timestamp must not be empty — schema is NOT NULL"
        );
        // Loose shape check for RFC 3339.
        assert!(stored.contains('T'), "should contain T: {stored}");
        assert!(stored.ends_with('Z'), "should end with Z: {stored}");
    }

    #[test]
    fn raw_hash_is_blake3_hex_of_raw() {
        let mut conn = in_memory();
        let e = sample_entry(r#"{"level":"info"}"#, "info");
        let expected = blake3::hash(e.raw.as_bytes()).to_hex().to_string();
        insert_batch(&mut conn, &[e]).expect("insert");

        let stored: String = conn
            .query_row("SELECT raw_hash FROM log_entries", [], |r| r.get(0))
            .unwrap();
        assert_eq!(stored, expected);
        assert_eq!(stored.len(), 64, "blake3 hex output is 64 chars");
    }

    #[test]
    fn open_creates_parent_directory() {
        let dir = tempdir().expect("tempdir");
        let db = dir.path().join("nested").join("path").join("index.db");
        assert!(!db.parent().unwrap().exists());

        let _conn = open(&db).expect("open");
        assert!(db.exists());
        assert!(db.parent().unwrap().exists());
    }

    #[test]
    fn insert_batch_chunks_into_transactions() {
        // Exercise BATCH_SIZE-spanning input. With BATCH_SIZE = 1000,
        // 2500 entries → 3 transactions (1000 + 1000 + 500).
        let mut conn = in_memory();
        let entries: Vec<LogEntry> = (0..2500)
            .map(|i| sample_entry(&format!(r#"{{"level":"info","seq":{i}}}"#), "info"))
            .collect();

        let stats = insert_batch(&mut conn, &entries).expect("insert");
        assert_eq!(stats.inserted, 2500);
        assert_eq!(stats.duplicates, 0);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM log_entries", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2500);
    }

    #[test]
    fn ingest_stats_combine_is_additive() {
        let a = IngestStats {
            inserted: 3,
            duplicates: 1,
        };
        let b = IngestStats {
            inserted: 7,
            duplicates: 2,
        };
        assert_eq!(
            a.combine(b),
            IngestStats {
                inserted: 10,
                duplicates: 3,
            }
        );
    }

    #[test]
    fn parser_to_indexer_pipeline() {
        // Integration check: parser output feeds the indexer without glue.
        let mut conn = in_memory();
        let lines = [
            r#"{"timestamp":"2026-04-19T10:00:00Z","level":"error","message":"boom","service":"pay"}"#,
            r#"{"timestamp":"2026-04-19T10:01:00Z","level":"info","request_id":123}"#,
            "not json",
            r#"{"timestamp":"2026-04-19T10:02:00Z","level":"warn"}"#,
        ];

        let entries: Vec<LogEntry> = lines
            .iter()
            .filter_map(|l| crate::parser::parse_line(l))
            .collect();
        assert_eq!(entries.len(), 3, "malformed line must be skipped");

        let stats = insert_batch(&mut conn, &entries).expect("insert");
        assert_eq!(stats.inserted, 3);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM log_entries", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 3);
    }
}
