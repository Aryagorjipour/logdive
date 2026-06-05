//! Cross-format ingestion and deduplication tests.
//!
//! These tests verify that the dedup contract (blake3 hash of the raw line)
//! behaves correctly across all three supported formats and in mixed-format
//! batches. Key invariants:
//!
//! - The same raw line ingested twice is deduplicated regardless of format.
//! - Semantically equivalent content in different formats has different raw
//!   bytes and therefore different hashes — two separate rows result.
//! - Plain-text entries require an explicit timestamp to survive the indexer's
//!   `timestamp NOT NULL` constraint; the test sets one directly.

use logdive_core::{
    Indexer,
    parsers::{json, logfmt, plain},
};

// ---------------------------------------------------------------------------
// JSON dedup
// ---------------------------------------------------------------------------

#[test]
fn same_json_line_ingested_twice_is_deduplicated() {
    let mut idx = Indexer::open_in_memory().unwrap();

    let line = r#"{"timestamp":"2026-04-20T10:00:00Z","level":"info","message":"hello"}"#;
    let entry = json::parse_line(line).expect("fixture must parse");

    idx.insert_batch(std::slice::from_ref(&entry)).unwrap();
    let stats = idx.insert_batch(std::slice::from_ref(&entry)).unwrap();

    assert_eq!(stats.deduplicated, 1, "second insert must be a dedup hit");
    assert_eq!(stats.inserted, 0);

    let count: i64 = idx
        .connection()
        .query_row("SELECT COUNT(*) FROM log_entries", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1, "table must contain exactly one row");
}

// ---------------------------------------------------------------------------
// Logfmt dedup
// ---------------------------------------------------------------------------

#[test]
fn same_logfmt_line_ingested_twice_is_deduplicated() {
    let mut idx = Indexer::open_in_memory().unwrap();

    let line = "timestamp=2026-04-20T10:00:00Z level=info message=hello";
    let entry = logfmt::parse_line(line).expect("fixture must parse");

    idx.insert_batch(std::slice::from_ref(&entry)).unwrap();
    let stats = idx.insert_batch(std::slice::from_ref(&entry)).unwrap();

    assert_eq!(stats.deduplicated, 1);
    assert_eq!(stats.inserted, 0);
}

// ---------------------------------------------------------------------------
// Cross-format: same logical content → two rows
// ---------------------------------------------------------------------------

#[test]
fn json_and_logfmt_with_equivalent_content_are_two_distinct_rows() {
    // The raw bytes differ between formats, so the blake3 hashes differ.
    // The indexer must store both rows, not deduplicate them.
    let mut idx = Indexer::open_in_memory().unwrap();

    let json_line = r#"{"timestamp":"2026-04-20T10:00:00Z","level":"info","message":"hello"}"#;
    let logfmt_line = "timestamp=2026-04-20T10:00:00Z level=info message=hello";

    let json_entry = json::parse_line(json_line).expect("json must parse");
    let logfmt_entry = logfmt::parse_line(logfmt_line).expect("logfmt must parse");

    let stats = idx.insert_batch(&[json_entry, logfmt_entry]).unwrap();
    assert_eq!(stats.inserted, 2, "different raw bytes → two distinct rows");
    assert_eq!(stats.deduplicated, 0);
}

// ---------------------------------------------------------------------------
// Plain text with explicit timestamp
// ---------------------------------------------------------------------------

#[test]
fn plain_entry_with_explicit_timestamp_is_inserted_and_queryable() {
    let mut idx = Indexer::open_in_memory().unwrap();

    let mut entry = plain::parse_line("startup complete").expect("plain must parse");
    // Plain parser leaves timestamp as None; set one so the indexer accepts it.
    entry.timestamp = Some("2026-04-20T10:00:00Z".to_string());

    let stats = idx.insert_batch(&[entry]).unwrap();
    assert_eq!(stats.inserted, 1);
    assert_eq!(stats.skipped_no_timestamp, 0);

    let ast = logdive_core::parse_query(r#"message contains "startup""#).unwrap();
    let results = logdive_core::execute(
        &ast,
        idx.connection(),
        logdive_core::QueryOptions::default(),
    )
    .unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].message.as_deref().unwrap().contains("startup"));
}

// ---------------------------------------------------------------------------
// Mixed-format batch
// ---------------------------------------------------------------------------

#[test]
fn mixed_format_batch_inserts_all_three_rows() {
    let mut idx = Indexer::open_in_memory().unwrap();

    let json_entry = json::parse_line(
        r#"{"timestamp":"2026-04-20T10:00:00Z","level":"error","message":"json row"}"#,
    )
    .unwrap();

    let logfmt_entry =
        logfmt::parse_line("timestamp=2026-04-20T11:00:00Z level=warn message=logfmt-row").unwrap();

    let mut plain_entry = plain::parse_line("plain row").unwrap();
    plain_entry.timestamp = Some("2026-04-20T12:00:00Z".to_string());

    let stats = idx
        .insert_batch(&[json_entry, logfmt_entry, plain_entry])
        .unwrap();
    assert_eq!(stats.inserted, 3, "all three formats must be inserted");
    assert_eq!(stats.deduplicated, 0);
    assert_eq!(stats.skipped_no_timestamp, 0);
}
