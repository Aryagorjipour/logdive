//! Security-focused integration tests for the logdive query pipeline.
//!
//! Tests use only the public `logdive-core` API. The per-module unit tests
//! (`executor::tests`, `indexer::tests`, `query::tests`) probe private helpers
//! (`is_safe_json_path_segment`, `validate_field_name`, etc.) directly; these
//! tests verify the outer wall — what a caller of the library experiences when
//! passing adversarial input through the full parse → execute path.

use logdive_core::{Indexer, LogEntry, QueryOptions, execute, parse_query};

fn make_entry(ts: &str, level: &str, message: &str) -> LogEntry {
    let raw = format!(r#"{{"timestamp":"{ts}","level":"{level}","message":"{message}"}}"#);
    let mut e = LogEntry::new(raw);
    e.timestamp = Some(ts.to_string());
    e.level = Some(level.to_string());
    e.message = Some(message.to_string());
    e
}

// ── SQL injection via field name ─────────────────────────────────────────────

#[test]
fn field_with_single_quote_is_parse_error() {
    // The tokenizer hits `'` before the field name reaches validate_field_name;
    // parse must fail and never reach SQL generation.
    assert!(parse_query("serv'ice=x").is_err());
}

#[test]
fn field_with_semicolon_is_parse_error() {
    assert!(parse_query("a;b=x").is_err());
}

#[test]
fn field_with_or_injection_payload_is_parse_error() {
    // Classic injection shape: tokenizer rejects `'` at byte 7.
    assert!(parse_query("service' OR 1=1--=x").is_err());
}

#[test]
fn field_with_unicode_non_ascii_is_parse_error() {
    // U+2019 RIGHT SINGLE QUOTATION MARK looks like a quote. The tokenizer
    // only allows ASCII bytes in identifiers; the multi-byte UTF-8 sequence
    // hits the catch-all error branch.
    assert!(parse_query("svc\u{2019}=x").is_err());
}

// ── SQL injection via value ──────────────────────────────────────────────────

#[test]
fn value_injection_does_not_drop_table() {
    let mut idx = Indexer::open_in_memory().unwrap();
    idx.insert_batch(&[make_entry("2026-04-20T10:00:00Z", "error", "real row")])
        .unwrap();

    // The quoted value is a DROP TABLE payload. The executor binds it as a
    // parameter; SQLite never evaluates it as SQL text.
    let ast = parse_query(r#"level="'; DROP TABLE log_entries--""#).unwrap();
    let results = execute(&ast, idx.connection(), QueryOptions::default()).unwrap();
    assert!(
        results.is_empty(),
        "adversarial value must not match the real row"
    );

    // Table must still exist — DROP TABLE was not executed.
    let stats = idx.stats().unwrap();
    assert_eq!(stats.entries, 1, "original row must still be present");
}

#[test]
fn value_injection_yields_zero_results_not_all_rows() {
    // A `1=1` payload as the value string must match literally, not act as a
    // tautology that returns every row.
    let mut idx = Indexer::open_in_memory().unwrap();
    idx.insert_batch(&[
        make_entry("2026-04-20T10:00:00Z", "info", "a"),
        make_entry("2026-04-20T10:00:01Z", "error", "b"),
    ])
    .unwrap();

    let ast = parse_query(r#"level="1=1 OR 1=1""#).unwrap();
    let results = execute(&ast, idx.connection(), QueryOptions::default()).unwrap();
    assert!(
        results.is_empty(),
        "injection payload must not widen the result set"
    );
}

// ── LIKE wildcard escaping ───────────────────────────────────────────────────

#[test]
fn like_underscore_in_contains_is_literal() {
    // SQL LIKE treats `_` as "any single character". The escape layer must
    // turn it into `\_` so it matches only the literal underscore.
    let mut idx = Indexer::open_in_memory().unwrap();
    idx.insert_batch(&[
        make_entry("2026-04-20T10:00:00Z", "info", "warn_threshold"),
        make_entry("2026-04-20T10:00:01Z", "info", "warnXthreshold"),
    ])
    .unwrap();

    let ast = parse_query(r#"message contains "warn_threshold""#).unwrap();
    let results = execute(&ast, idx.connection(), QueryOptions::default()).unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].message.as_deref().unwrap().contains('_'));
}

#[test]
fn like_backslash_in_contains_is_literal() {
    // The LIKE escape character is `\`; a literal `\` in the search term
    // must be doubled to `\\` so it matches only the literal backslash.
    let mut idx = Indexer::open_in_memory().unwrap();
    idx.insert_batch(&[
        make_entry("2026-04-20T10:00:00Z", "info", r"path\to\file"),
        make_entry("2026-04-20T10:00:01Z", "info", "unrelated message"),
    ])
    .unwrap();

    // Raw string: the backslashes here are Rust literal backslashes, which
    // the query parser also treats as literal (no escape handling in v0.2).
    let ast = parse_query(r#"message contains "path\to""#).unwrap();
    let results = execute(&ast, idx.connection(), QueryOptions::default()).unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].message.as_deref().unwrap().contains('\\'));
}

// ── Resource exhaustion ──────────────────────────────────────────────────────

#[test]
fn deep_or_1000_disjuncts_does_not_overflow() {
    // A pathologically wide OR query must parse and execute without stack
    // overflow. parse_or_expr is iterative (loop, not recursion), so this
    // also serves as a regression guard for any future recursive refactor.
    let query: String = (0..1000)
        .map(|i| format!("level=v{i}"))
        .collect::<Vec<_>>()
        .join(" OR ");

    let ast = parse_query(&query).expect("1000-disjunct query must parse");
    let idx = Indexer::open_in_memory().unwrap();
    // execute may return Err (SQLite's SQLITE_MAX_EXPR_DEPTH limit fires at
    // 1000 disjuncts) but must never panic or overflow the stack. The
    // no-panic guarantee is the security property being tested here.
    let _ = execute(&ast, idx.connection(), QueryOptions::default());
}

#[test]
fn long_line_10mb_ingested_gracefully() {
    // A single 10 MB raw line must not panic or OOM. Either insert succeeds
    // or the entry is skipped (e.g. on a future size limit), but no panic.
    let mut idx = Indexer::open_in_memory().unwrap();
    let big = "x".repeat(10 * 1024 * 1024);
    let mut e = LogEntry::new(big.clone());
    e.timestamp = Some("2026-04-20T10:00:00Z".to_string());
    e.message = Some(big);
    let _ = idx.insert_batch(&[e]); // result irrelevant; no panic is the contract
}
