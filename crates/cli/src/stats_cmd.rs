//! `stats` subcommand: report aggregate metadata about the index.
//!
//! Output is human-readable only, per the decisions log and the milestone 7
//! design notes. JSON output for stats lives in the HTTP API (milestone 8),
//! which has a natural serialization target; mirroring it in the CLI would
//! duplicate surface area without clear benefit.
//!
//! Missing-database policy: if the configured index path does not exist on
//! disk, `stats` reports an error and exits non-zero rather than auto-
//! creating an empty database. Creating a DB as a side effect of a read
//! action would hide typos in the `--db` flag behind a misleading
//! "zero entries" readout.

use std::path::Path;

use logdive_core::{Indexer, LogdiveError, Result, Stats};

// ---------------------------------------------------------------------------
// Args
// ---------------------------------------------------------------------------

/// Arguments for the `stats` subcommand.
///
/// Empty today — kept as a named type so the subcommand dispatch in `main`
/// is uniform across subcommands, and so milestone 9 can add flags (e.g.
/// `--no-tags`) without restructuring the Clap definition.
#[derive(clap::Args, Debug)]
pub struct StatsArgs {}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Run `logdive stats` against the index at `db`.
///
/// `db` is resolved by the caller (honoring `--db` or the default
/// `~/.logdive/index.db`). The function checks existence, opens the index,
/// reads [`Stats`], pairs it with on-disk file size, and prints the
/// formatted report to stdout.
pub fn run_stats(db: &Path, _args: StatsArgs) -> Result<()> {
    if !db.exists() {
        return Err(LogdiveError::io_at(
            db,
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "no index found at {}; run `logdive ingest` first",
                    db.display()
                ),
            ),
        ));
    }

    let indexer = Indexer::open(db)?;
    let stats = indexer.stats()?;
    let size_bytes = std::fs::metadata(db)
        .map_err(|e| LogdiveError::io_at(db, e))?
        .len();

    let out = format_stats(db, &stats, size_bytes);
    println!("{out}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Formatting (pure, testable)
// ---------------------------------------------------------------------------

/// Format a `Stats` report as a multi-line human-readable string.
///
/// No trailing newline — `println!` adds one.
pub fn format_stats(db: &Path, stats: &Stats, size_bytes: u64) -> String {
    let mut s = String::new();
    s.push_str(&format!("logdive index: {}\n", db.display()));

    s.push_str(&format!(
        "  Entries:       {}\n",
        with_thousands_separator(stats.entries)
    ));

    s.push_str(&format!("  Time range:    {}\n", format_time_range(stats)));

    s.push_str(&format!("  Tags:          {}\n", format_tags(&stats.tags)));

    // Size is the last line — no trailing newline so the caller's println!
    // terminates cleanly.
    s.push_str(&format!("  DB size:       {}", format_size(size_bytes)));
    s
}

fn format_time_range(stats: &Stats) -> String {
    match (
        stats.min_timestamp.as_deref(),
        stats.max_timestamp.as_deref(),
    ) {
        (Some(min), Some(max)) => format!("{min} → {max}"),
        _ => "(empty)".to_string(),
    }
}

/// Render the tags list per the Q9 display contract:
///
///   1. Non-null tags first, alphabetically ascending.
///   2. `(untagged)` appended at the end if any rows carry NULL tag.
///
/// `Indexer::stats()` returns `None` at the head of the vector (SQLite
/// NULLs-first ordering); here we reshuffle to end-of-list for display.
fn format_tags(tags: &[Option<String>]) -> String {
    if tags.is_empty() {
        return "(none)".to_string();
    }

    let mut named: Vec<&str> = Vec::with_capacity(tags.len());
    let mut has_untagged = false;
    for t in tags {
        match t {
            Some(s) => named.push(s.as_str()),
            None => has_untagged = true,
        }
    }

    let mut parts: Vec<String> = named.iter().map(|s| (*s).to_string()).collect();
    if has_untagged {
        parts.push("(untagged)".to_string());
    }
    parts.join(", ")
}

/// Render a byte count as `N.M <unit> (<bytes> bytes)`.
///
/// Uses base-10 units (1 KB = 1,000 bytes), which matches `ls -h` and
/// filesystem tooling users expect at the CLI. Internally `u64` is fine
/// — a single SQLite file can't exceed 2^64 bytes and the intermediate
/// `f64` cast only loses precision for files above ~2^53 bytes, which
/// is not a real-world case for a log index.
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1_000;
    const MB: u64 = 1_000_000;
    const GB: u64 = 1_000_000_000;

    let pretty = if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} bytes")
    };

    if bytes >= KB {
        format!("{pretty} ({} bytes)", with_thousands_separator(bytes))
    } else {
        // Below 1 KB, "123 bytes (123 bytes)" is redundant — drop the suffix.
        pretty
    }
}

/// Insert thousand separators into a non-negative integer, in English
/// convention (`1,234,567`). Written by hand to avoid pulling a formatting
/// crate for one function; straightforward to audit.
fn with_thousands_separator(n: u64) -> String {
    let digits = n.to_string();
    let bytes = digits.as_bytes();
    let len = bytes.len();

    // Capacity: original + one comma per group-of-three boundary.
    let mut out = String::with_capacity(len + len / 3);
    for (i, &b) in bytes.iter().enumerate() {
        if i > 0 && (len - i) % 3 == 0 {
            out.push(',');
        }
        out.push(b as char);
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use logdive_core::LogEntry;

    // Because `Stats` is `#[non_exhaustive]` across crate boundaries, tests
    // here cannot construct it with a struct literal. Instead they build
    // the object under test with real entries against an in-memory index
    // — which has the side benefit of exercising the real plumbing.

    /// Build a `LogEntry` with the minimum fields required to survive
    /// insertion (a non-None timestamp is mandatory per the indexer's
    /// no-fabrication policy).
    fn entry(ts: &str, level: &str, message: &str, tag: Option<&str>) -> LogEntry {
        let tag_part = tag.map(|t| format!(r#","tag":"{t}""#)).unwrap_or_default();
        let raw =
            format!(r#"{{"timestamp":"{ts}","level":"{level}","message":"{message}"{tag_part}}}"#);
        let mut e = LogEntry::new(raw);
        e.timestamp = Some(ts.to_string());
        e.level = Some(level.to_string());
        e.message = Some(message.to_string());
        e.tag = tag.map(|t| t.to_string());
        e
    }

    fn empty_stats() -> Stats {
        let idx = Indexer::open_in_memory().expect("open in-memory");
        idx.stats().expect("stats on empty index")
    }

    /// Three entries: one untagged at t0, one tagged "api" at t1, one
    /// tagged "payments" at t2. Gives us a non-empty time range and the
    /// full tag-ordering surface (`None` + two named).
    fn three_entry_stats() -> Stats {
        let mut idx = Indexer::open_in_memory().expect("open in-memory");
        idx.insert_batch(&[
            entry("2026-03-14T08:22:01Z", "info", "oldest", None),
            entry("2026-04-01T12:00:00Z", "warn", "middle", Some("api")),
            entry("2026-04-22T19:45:03Z", "error", "newest", Some("payments")),
        ])
        .expect("insert");
        idx.stats().expect("stats")
    }

    // --- Standalone helpers ---

    #[test]
    fn thousands_separator_small_numbers() {
        assert_eq!(with_thousands_separator(0), "0");
        assert_eq!(with_thousands_separator(7), "7");
        assert_eq!(with_thousands_separator(999), "999");
    }

    #[test]
    fn thousands_separator_crosses_each_boundary() {
        assert_eq!(with_thousands_separator(1_000), "1,000");
        assert_eq!(with_thousands_separator(12_345), "12,345");
        assert_eq!(with_thousands_separator(123_456), "123,456");
        assert_eq!(with_thousands_separator(1_234_567), "1,234,567");
        assert_eq!(with_thousands_separator(1_000_000_000), "1,000,000,000");
    }

    #[test]
    fn format_size_under_one_kb_omits_redundant_suffix() {
        assert_eq!(format_size(0), "0 bytes");
        assert_eq!(format_size(512), "512 bytes");
        assert_eq!(format_size(999), "999 bytes");
    }

    #[test]
    fn format_size_renders_kb_mb_gb() {
        assert_eq!(format_size(1_000), "1.0 KB (1,000 bytes)");
        assert_eq!(format_size(8_400_000), "8.4 MB (8,400,000 bytes)");
        assert_eq!(format_size(2_500_000_000), "2.5 GB (2,500,000,000 bytes)");
    }

    #[test]
    fn format_tags_empty_shows_none() {
        assert_eq!(format_tags(&[]), "(none)");
    }

    #[test]
    fn format_tags_only_untagged_shows_that_marker() {
        assert_eq!(format_tags(&[None]), "(untagged)");
    }

    #[test]
    fn format_tags_only_named_shows_comma_separated() {
        let tags = vec![Some("api".to_string()), Some("payments".to_string())];
        assert_eq!(format_tags(&tags), "api, payments");
    }

    #[test]
    fn format_tags_moves_untagged_to_end_of_display_list() {
        // Indexer::stats() puts None first (SQLite NULL-first); CLI display
        // rule is "(untagged)" at the *end*.
        let tags = vec![
            None,
            Some("api".to_string()),
            Some("payments".to_string()),
            Some("worker".to_string()),
        ];
        assert_eq!(format_tags(&tags), "api, payments, worker, (untagged)");
    }

    // --- Behavior against real Stats values ---

    #[test]
    fn format_time_range_empty_when_index_has_no_entries() {
        let stats = empty_stats();
        assert_eq!(format_time_range(&stats), "(empty)");
    }

    #[test]
    fn format_time_range_renders_arrow_between_min_and_max() {
        let stats = three_entry_stats();
        assert_eq!(
            format_time_range(&stats),
            "2026-03-14T08:22:01Z → 2026-04-22T19:45:03Z"
        );
    }

    #[test]
    fn format_stats_full_report_has_all_sections() {
        let stats = three_entry_stats();
        let out = format_stats(Path::new("/home/user/.logdive/index.db"), &stats, 8_400_000);

        assert!(out.contains("logdive index: /home/user/.logdive/index.db"));
        assert!(out.contains("Entries:       3"));
        assert!(out.contains("Time range:    2026-03-14T08:22:01Z → 2026-04-22T19:45:03Z"));
        // Non-null tags first in alphabetical order, (untagged) at end.
        assert!(out.contains("Tags:          api, payments, (untagged)"));
        assert!(out.contains("DB size:       8.4 MB (8,400,000 bytes)"));
        // No trailing newline: the render function leaves that to println!.
        assert!(!out.ends_with('\n'));
    }

    #[test]
    fn format_stats_empty_index_is_graceful() {
        let stats = empty_stats();
        let out = format_stats(Path::new("/tmp/x.db"), &stats, 12_288);

        assert!(out.contains("Entries:       0"));
        assert!(out.contains("Time range:    (empty)"));
        assert!(out.contains("Tags:          (none)"));
        assert!(out.contains("DB size:       12.3 KB (12,288 bytes)"));
    }

    /// Guard against drift between the `Stats` contract advertised by
    /// `Indexer::stats()` (NULL tag at index 0) and the display contract
    /// implemented by `format_tags` (untagged at end). If either side
    /// ever flips their ordering, this test fails loudly.
    #[test]
    fn stats_untagged_first_renders_untagged_last() {
        let stats = three_entry_stats();
        // Core guarantees: NULL first, then named in ascending order.
        assert_eq!(stats.tags[0], None);
        assert_eq!(stats.tags[1], Some("api".to_string()));
        assert_eq!(stats.tags[2], Some("payments".to_string()));
        // Display guarantees: named first, "(untagged)" last.
        assert_eq!(format_tags(&stats.tags), "api, payments, (untagged)");
    }

    #[test]
    fn stats_counts_entries_and_entries_show_in_report() {
        let stats = three_entry_stats();
        assert_eq!(stats.entries, 3);
        let out = format_stats(Path::new("/tmp/x.db"), &stats, 0);
        assert!(out.contains("Entries:       3"));
    }
}
