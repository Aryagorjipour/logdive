//! Ingestion throughput benchmarks.
//!
//! Measures the full ingestion path: parse each JSON line, construct a
//! `LogEntry`, blake3-hash the raw line, and insert a batch into a fresh
//! on-disk SQLite index.
//!
//! We report throughput in elements (lines) per second via
//! `Throughput::Elements`. The primary number users want from these
//! benchmarks is "how many lines per second can logdive index?" and
//! criterion's throughput reporting surfaces that directly alongside
//! the time-per-batch measurement.
//!
//! Each sample starts from an empty temp-dir-backed database. We do not
//! reuse the database across iterations because `INSERT OR IGNORE` would
//! turn subsequent samples into no-ops (measuring hash-check speed, not
//! ingestion).

use std::path::PathBuf;

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use tempfile::TempDir;

use logdive_core::{Indexer, parse_line};

/// Generate `n` synthetic JSON log lines. Deterministic — same `n`
/// produces the same output across runs and machines.
///
/// The mix of fields (level, message, service, user_id, duration_ms) is
/// chosen to exercise both known-column lifts and the JSON blob path
/// during insertion. Levels cycle so ingestion doesn't hit a degenerate
/// all-same-value index, and timestamps advance monotonically so dedup
/// via raw_hash is actually per-line rather than accidentally uniform.
fn generate_lines(n: usize) -> Vec<String> {
    let levels = ["info", "warn", "error", "debug"];
    let services = ["payments", "orders", "auth", "api"];
    (0..n)
        .map(|i| {
            let level = levels[i % levels.len()];
            let service = services[i % services.len()];
            // 2026-04-15T09:00:00Z + i seconds — far in the past for any
            // "last Nh" queries, but always a valid ISO-8601 timestamp.
            let hour = 9 + (i / 3600) % 24;
            let minute = (i / 60) % 60;
            let second = i % 60;
            format!(
                r#"{{"timestamp":"2026-04-15T{hour:02}:{minute:02}:{second:02}Z","level":"{level}","message":"event {i}","service":"{service}","user_id":{i},"duration_ms":{dur}}}"#,
                dur = (i * 13) % 5000
            )
        })
        .collect()
}

/// Parse a slice of lines into LogEntry values. Used outside the
/// measurement loop so ingestion benches measure the indexer path, not
/// the parser (which has its own implicit coverage via integration
/// tests). A separate parse-only benchmark would be additive later.
fn parse_all(lines: &[String]) -> Vec<logdive_core::LogEntry> {
    lines.iter().filter_map(|l| parse_line(l)).collect()
}

/// Open a fresh on-disk `Indexer` under the given temp directory.
///
/// Returns the indexer and the path so callers can reuse the path for
/// subsequent reopenings if needed (not used here, but keeps the helper
/// general).
fn fresh_indexer(tmp: &TempDir) -> (Indexer, PathBuf) {
    let path = tmp.path().join("bench.db");
    let indexer = Indexer::open(&path).expect("open bench index");
    (indexer, path)
}

fn bench_insert_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("ingest/insert_batch");

    for &n in &[100usize, 1_000, 10_000] {
        let lines = generate_lines(n);
        let entries = parse_all(&lines);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &entries, |b, entries| {
            b.iter_batched(
                || {
                    // Per-sample setup: fresh temp dir, fresh DB, fresh indexer.
                    // Excluded from the measurement window by iter_batched.
                    let tmp = TempDir::new().expect("tempdir");
                    let (indexer, _path) = fresh_indexer(&tmp);
                    (tmp, indexer)
                },
                |(tmp, mut indexer)| {
                    // Measured: the actual batched insert.
                    let stats = indexer.insert_batch(entries).expect("insert batch");
                    // Touch the stats so the optimizer can't eliminate them.
                    assert_eq!(stats.inserted, entries.len());
                    // Keep the tempdir alive until the closure ends so the
                    // DB file isn't unlinked mid-measurement.
                    drop(indexer);
                    drop(tmp);
                },
                // LargeInput because the setup (fresh DB + parsed entries)
                // is non-trivial; we want criterion to prefer fewer,
                // larger-sample iterations to amortize setup cost.
                BatchSize::LargeInput,
            );
        });
    }

    group.finish();
}

fn bench_parse_and_insert(c: &mut Criterion) {
    // Variant that includes JSON parsing inside the measured region.
    // Closer to what users actually see when they pipe logs into
    // `logdive ingest`. Reported throughput is the end-to-end line rate.
    let mut group = c.benchmark_group("ingest/parse_and_insert");

    for &n in &[1_000usize, 10_000] {
        let lines = generate_lines(n);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &lines, |b, lines| {
            b.iter_batched(
                || {
                    let tmp = TempDir::new().expect("tempdir");
                    let (indexer, _path) = fresh_indexer(&tmp);
                    (tmp, indexer)
                },
                |(tmp, mut indexer)| {
                    // Measured: parse + batched insert.
                    let entries: Vec<_> = lines.iter().filter_map(|l| parse_line(l)).collect();
                    let stats = indexer.insert_batch(&entries).expect("insert");
                    assert_eq!(stats.inserted, entries.len());
                    drop(indexer);
                    drop(tmp);
                },
                BatchSize::LargeInput,
            );
        });
    }

    group.finish();
}

criterion_group!(benches, bench_insert_batch, bench_parse_and_insert);
criterion_main!(benches);
