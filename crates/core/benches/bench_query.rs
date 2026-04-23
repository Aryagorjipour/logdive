//! Query latency benchmarks.
//!
//! Measures query execution time against a pre-populated 100k-row index.
//! The index is built once during bench setup and reused across all
//! query samples — queries are read-only and idempotent, so reusing the
//! index gives each bench the full benefit of SQLite's page cache and
//! prepared-statement overhead doesn't swamp the measurement.
//!
//! Scenarios covered:
//!
//!   1. Known-field equality (hits the `idx_level` / `idx_tag` index).
//!   2. JSON-blob field equality (json_extract path).
//!   3. CONTAINS scan (LIKE with wildcards, no index).
//!   4. AND-chain combining known + unknown fields.
//!
//! For each scenario we vary the selectivity where it matters, so users
//! can see how latency tracks result-set size.

use std::path::PathBuf;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use tempfile::TempDir;

use logdive_core::{Indexer, LogEntry, QueryNode, execute, parse_query};

/// Fixture size for all query benchmarks. 100k rows is the project doc's
/// latency target.
const FIXTURE_SIZE: usize = 100_000;

/// Build the fixture LogEntry values. Field cardinalities are chosen so
/// queries in the benches hit well-defined fractions of the dataset:
///
///   - `level=error` matches ~25% (1 in 4 levels).
///   - `level=nonsense` matches 0 rows.
///   - `service=payments` matches ~25% (1 in 4 services).
///   - `user_id=42` matches exactly one row.
///   - `message contains "event 500"` matches a specific small slice.
fn build_fixture() -> Vec<LogEntry> {
    let levels = ["info", "warn", "error", "debug"];
    let services = ["payments", "orders", "auth", "api"];
    (0..FIXTURE_SIZE)
        .map(|i| {
            let level = levels[i % levels.len()];
            let service = services[i % services.len()];
            let hour = 9 + (i / 3600) % 24;
            let minute = (i / 60) % 60;
            let second = i % 60;
            let ts = format!("2026-04-15T{hour:02}:{minute:02}:{second:02}Z");
            let raw = format!(
                r#"{{"timestamp":"{ts}","level":"{level}","message":"event {i}","service":"{service}","user_id":{i},"duration_ms":{dur}}}"#,
                dur = (i * 13) % 5000
            );
            let mut entry = LogEntry::new(raw);
            entry.timestamp = Some(ts);
            entry.level = Some(level.to_string());
            entry.message = Some(format!("event {i}"));
            entry
                .fields
                .insert("service".to_string(), serde_json::Value::String(service.to_string()));
            entry.fields.insert(
                "user_id".to_string(),
                serde_json::Value::Number(serde_json::Number::from(i as u64)),
            );
            entry.fields.insert(
                "duration_ms".to_string(),
                serde_json::Value::Number(serde_json::Number::from(((i * 13) % 5000) as u64)),
            );
            entry
        })
        .collect()
}

/// Build a populated on-disk index under the given tempdir. Returns the
/// path (and keeps the TempDir alive via the caller's ownership).
///
/// Populating the fixture takes a few seconds. We do this exactly once
/// per benchmark group via `criterion`'s outer setup — all samples
/// within a group share the same index.
fn setup_index(tmp: &TempDir) -> PathBuf {
    let path = tmp.path().join("bench.db");
    let mut indexer = Indexer::open(&path).expect("open bench index");
    let entries = build_fixture();
    indexer.insert_batch(&entries).expect("populate fixture");
    path
}

/// Parse a query string once, outside the measurement loop. Query-parse
/// overhead is its own concern; these benches measure execution time
/// against the SQLite index.
fn parse(q: &str) -> QueryNode {
    parse_query(q).unwrap_or_else(|e| panic!("bench query failed to parse: {e}"))
}

fn bench_known_field_equality(c: &mut Criterion) {
    // Pre-build the index once and share it across all samples in the group.
    let tmp = TempDir::new().expect("tempdir");
    let path = setup_index(&tmp);
    let indexer = Indexer::open_read_only(&path).expect("open read-only");

    let mut group = c.benchmark_group("query/known_field_equality");

    // Three selectivities: ~25% (high), ~0% (miss), ~100% (all via since).
    let scenarios = [
        ("level_error_25pct", "level=error"),
        ("level_nonsense_0pct", "level=nonsense"),
        ("since_epoch_100pct", "since 2020-01-01"),
    ];

    for (label, q) in scenarios {
        let ast = parse(q);
        group.bench_function(BenchmarkId::from_parameter(label), |b| {
            b.iter(|| {
                let rows = execute(&ast, indexer.connection(), Some(1_000)).expect("execute");
                // Black-box the row count so the optimizer doesn't elide
                // the query. `rows.len()` is cheap and self-documenting.
                assert!(rows.len() <= 1_000);
            });
        });
    }

    group.finish();
}

fn bench_json_field_equality(c: &mut Criterion) {
    let tmp = TempDir::new().expect("tempdir");
    let path = setup_index(&tmp);
    let indexer = Indexer::open_read_only(&path).expect("open read-only");

    let mut group = c.benchmark_group("query/json_field_equality");

    let scenarios = [
        ("service_payments_25pct", "service=payments"),
        ("service_unknown_0pct", "service=unknown"),
        ("user_id_singleton", "user_id=42"),
    ];

    for (label, q) in scenarios {
        let ast = parse(q);
        group.bench_function(BenchmarkId::from_parameter(label), |b| {
            b.iter(|| {
                let rows = execute(&ast, indexer.connection(), Some(1_000)).expect("execute");
                assert!(rows.len() <= 1_000);
            });
        });
    }

    group.finish();
}

fn bench_contains(c: &mut Criterion) {
    let tmp = TempDir::new().expect("tempdir");
    let path = setup_index(&tmp);
    let indexer = Indexer::open_read_only(&path).expect("open read-only");

    let mut group = c.benchmark_group("query/contains");

    // CONTAINS is a LIKE scan — no index help. These bench the worst case
    // where we touch every row in the table.
    let scenarios = [
        ("message_event_500", r#"message contains "event 500""#),
        (
            "message_nonsense",
            r#"message contains "xyzzy_not_present""#,
        ),
    ];

    for (label, q) in scenarios {
        let ast = parse(q);
        group.bench_function(BenchmarkId::from_parameter(label), |b| {
            b.iter(|| {
                let rows = execute(&ast, indexer.connection(), Some(1_000)).expect("execute");
                assert!(rows.len() <= 1_000);
            });
        });
    }

    group.finish();
}

fn bench_and_chain(c: &mut Criterion) {
    let tmp = TempDir::new().expect("tempdir");
    let path = setup_index(&tmp);
    let indexer = Indexer::open_read_only(&path).expect("open read-only");

    let mut group = c.benchmark_group("query/and_chain");

    let scenarios = [
        ("two_clause_known", "level=error AND since 2020-01-01"),
        ("two_clause_mixed", "level=error AND service=payments"),
        (
            "three_clause_mixed",
            r#"level=error AND service=payments AND message contains "event""#,
        ),
    ];

    for (label, q) in scenarios {
        let ast = parse(q);
        group.bench_function(BenchmarkId::from_parameter(label), |b| {
            b.iter(|| {
                let rows = execute(&ast, indexer.connection(), Some(1_000)).expect("execute");
                assert!(rows.len() <= 1_000);
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_known_field_equality,
    bench_json_field_equality,
    bench_contains,
    bench_and_chain
);
criterion_main!(benches);
