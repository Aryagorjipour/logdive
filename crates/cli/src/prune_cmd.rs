//! `prune` subcommand: time-based retention for the index.
//!
//! Deletes entries older than a caller-specified cutoff, then `VACUUM`s the
//! database to reclaim disk space. The cutoff is given one of two ways,
//! exactly one of which is required (enforced by a clap `ArgGroup`):
//!
//! - `--older-than <DURATION>` — relative. `30d`, `2h`, `45m`. The cutoff is
//!   computed as "now minus DURATION", so `--older-than 30d` keeps the last
//!   30 days and deletes everything before that.
//! - `--before <DATETIME>` — absolute. Accepts the same three datetime
//!   shapes the query language's `since` clause accepts: RFC3339, a naive
//!   `YYYY-MM-DD HH:MM:SS` (interpreted as UTC), or a bare `YYYY-MM-DD`
//!   date (interpreted as UTC midnight).
//!
//! Duration resolution lives here rather than in `logdive-core` because it
//! is a CLI-surface concern: core's [`Indexer::prune`] takes an already
//! resolved RFC3339 cutoff string and stays a clean library boundary. There
//! is minor overlap with the executor's private datetime parsing, but
//! widening core's *published* public API for a CLI-only concern is the
//! worse trade.
//!
//! # Safety
//!
//! `prune` is destructive. By default it prints how many entries the cutoff
//! would delete and asks for an interactive `[y/N]` confirmation; anything
//! other than `y` / `yes` aborts without touching the index. The `--yes`
//! flag bypasses the prompt for cron jobs and scripts. If the cutoff would
//! delete nothing, `prune` says so and exits without prompting.
//!
//! Missing-database policy mirrors `stats`: if the configured index path
//! does not exist, `prune` errors rather than auto-creating an empty
//! database — pruning a database that isn't there is a user mistake worth
//! surfacing, not silently succeeding against an empty index.

use std::io::{self, Write};
use std::path::Path;

use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};

use logdive_core::{Indexer, LogdiveError, Result};

// ---------------------------------------------------------------------------
// Args
// ---------------------------------------------------------------------------

/// Arguments for the `prune` subcommand.
///
/// The `prune_cutoff` group makes `--older-than` and `--before` mutually
/// exclusive and requires exactly one of them: clap rejects both supplying
/// neither and supplying both before `run_prune` is ever called.
#[derive(clap::Args, Debug)]
#[command(group(
    clap::ArgGroup::new("prune_cutoff")
        .required(true)
        .args(["older_than", "before"])
))]
pub struct PruneArgs {
    /// Delete entries older than this duration relative to now.
    ///
    /// A whole number followed by a unit: `m` (minutes), `h` (hours), or
    /// `d` (days). Examples: `30d`, `12h`, `90m`.
    #[arg(long, value_name = "DURATION")]
    older_than: Option<String>,

    /// Delete entries with a timestamp strictly before this datetime.
    ///
    /// Accepts RFC3339 (`2026-01-01T00:00:00Z`), a naive datetime
    /// (`2026-01-01 00:00:00`, interpreted as UTC), or a bare date
    /// (`2026-01-01`, interpreted as UTC midnight).
    #[arg(long, value_name = "DATETIME")]
    before: Option<String>,

    /// Skip the interactive confirmation prompt.
    ///
    /// Intended for scripts and scheduled jobs. Without this flag, `prune`
    /// shows how many entries would be deleted and waits for a `y` / `yes`
    /// answer on stdin.
    #[arg(long)]
    yes: bool,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Run `logdive prune` against the index at `db`.
///
/// Resolves the cutoff from whichever flag was supplied, counts the doomed
/// rows for the confirmation prompt, deletes them via [`Indexer::prune`]
/// (which also `VACUUM`s), and prints a summary.
pub fn run_prune(db: &Path, args: PruneArgs) -> Result<()> {
    // Fail fast on a missing index, matching `stats` — don't auto-create.
    if !db.exists() {
        return Err(LogdiveError::io_at(
            db,
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("no index found at {}; nothing to prune", db.display()),
            ),
        ));
    }

    // Resolve the cutoff. clap's ArgGroup guarantees exactly one flag is
    // set, but `resolve_cutoff` handles the other shapes defensively rather
    // than panicking. A bad duration / datetime string surfaces here as an
    // `InvalidInput` I/O error so the CLI's top-level handler prints it.
    let cutoff = resolve_cutoff(&args)
        .map_err(|msg| LogdiveError::IoBare(io::Error::new(io::ErrorKind::InvalidInput, msg)))?;
    let cutoff_rfc3339 = cutoff.to_rfc3339();

    let mut indexer = Indexer::open(db)?;

    // Count what the cutoff would delete: this both makes the confirmation
    // prompt informative and lets us skip prompting entirely when there is
    // nothing to do. Strict `<` matches `Indexer::prune`'s own comparison.
    let doomed: i64 = indexer.connection().query_row(
        "SELECT COUNT(*) FROM log_entries WHERE timestamp < ?1",
        [&cutoff_rfc3339],
        |row| row.get(0),
    )?;

    if doomed == 0 {
        println!("Nothing to prune — no entries older than {cutoff_rfc3339}.");
        return Ok(());
    }

    if !args.yes && !confirm(doomed, &cutoff_rfc3339)? {
        println!("Aborted. No entries deleted.");
        return Ok(());
    }

    let stats = indexer.prune(&cutoff_rfc3339)?;
    println!(
        "Pruned {} {} older than {cutoff_rfc3339}.",
        stats.deleted,
        plural_entries(stats.deleted)
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Cutoff resolution (pure, testable)
// ---------------------------------------------------------------------------

/// Resolve the prune cutoff from whichever flag clap accepted.
///
/// Returns `Err` with a user-facing message on a malformed duration or
/// datetime, or on the both-set / neither-set states (which clap's
/// `ArgGroup` already rules out, handled here only so a future grouping
/// change can't turn into a panic).
fn resolve_cutoff(args: &PruneArgs) -> std::result::Result<DateTime<Utc>, String> {
    match (&args.older_than, &args.before) {
        (Some(spec), None) => resolve_older_than(spec, Utc::now()),
        (None, Some(spec)) => resolve_before(spec),
        (Some(_), Some(_)) => Err("--older-than and --before are mutually exclusive".to_string()),
        (None, None) => Err("one of --older-than or --before is required".to_string()),
    }
}

/// Resolve a relative `--older-than` duration spec against a given `now`.
///
/// `now` is a parameter rather than read internally so the unit tests can
/// pin it and assert exact cutoffs.
fn resolve_older_than(
    spec: &str,
    now: DateTime<Utc>,
) -> std::result::Result<DateTime<Utc>, String> {
    let spec = spec.trim();

    // The unit is the final character; `len_utf8()` keeps the byte split on
    // a char boundary even if a stray multi-byte char is supplied.
    let unit_char = spec
        .chars()
        .last()
        .ok_or_else(|| "empty duration".to_string())?;
    let num_part = &spec[..spec.len() - unit_char.len_utf8()];

    let bad = || {
        format!(
            "invalid duration {spec:?}: expected a whole number followed by \
             'm', 'h', or 'd' (e.g. '30d', '12h', '90m')"
        )
    };

    // Parse as u64 first — this rejects negatives ('-5' is not a valid u64).
    let amount_u64: u64 = num_part.parse().map_err(|_| bad())?;
    let amount =
        i64::try_from(amount_u64).map_err(|_| format!("duration {spec:?} is too large"))?;

    let seconds = match unit_char {
        'm' => amount.checked_mul(60),
        'h' => amount.checked_mul(60 * 60),
        'd' => amount.checked_mul(24 * 60 * 60),
        _ => return Err(bad()),
    }
    .ok_or_else(|| format!("duration {spec:?} is too large"))?;

    now.checked_sub_signed(chrono::Duration::seconds(seconds))
        .ok_or_else(|| format!("duration {spec:?} is too large"))
}

/// Resolve an absolute `--before` datetime spec.
///
/// Accepts the same three shapes the query language's `since` clause does:
/// RFC3339, a naive `YYYY-MM-DD HH:MM:SS` / `YYYY-MM-DDTHH:MM:SS` (UTC), or
/// a bare `YYYY-MM-DD` date (UTC midnight).
fn resolve_before(spec: &str) -> std::result::Result<DateTime<Utc>, String> {
    let spec = spec.trim();

    if let Ok(dt) = DateTime::parse_from_rfc3339(spec) {
        return Ok(dt.with_timezone(&Utc));
    }
    for fmt in &["%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M:%S"] {
        if let Ok(ndt) = NaiveDateTime::parse_from_str(spec, fmt) {
            return Ok(Utc.from_utc_datetime(&ndt));
        }
    }
    if let Ok(nd) = NaiveDate::parse_from_str(spec, "%Y-%m-%d") {
        let ndt = nd.and_hms_opt(0, 0, 0).expect("00:00:00 is a valid time");
        return Ok(Utc.from_utc_datetime(&ndt));
    }

    Err(format!(
        "invalid datetime {spec:?}: expected RFC3339, \
         `YYYY-MM-DD HH:MM:SS`, or `YYYY-MM-DD`"
    ))
}

// ---------------------------------------------------------------------------
// Confirmation prompt
// ---------------------------------------------------------------------------

/// Prompt the user to confirm a destructive prune.
///
/// Prints the doomed-row count and cutoff, then reads one line from stdin.
/// Returns `true` only for `y` / `yes` (case-insensitive); every other
/// answer — including an empty line — is treated as "no", matching the
/// `[y/N]` convention where the capital `N` is the default.
fn confirm(count: i64, cutoff: &str) -> Result<bool> {
    print!(
        "This will permanently delete {count} {} older than {cutoff}.\n\
         Continue? [y/N] ",
        plural_entries(count as u64)
    );
    io::stdout().flush().map_err(LogdiveError::IoBare)?;

    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .map_err(LogdiveError::IoBare)?;

    let answer = line.trim().to_ascii_lowercase();
    Ok(answer == "y" || answer == "yes")
}

/// `"entry"` for exactly one, `"entries"` otherwise.
fn plural_entries(n: u64) -> &'static str {
    if n == 1 { "entry" } else { "entries" }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixed reference "now" for deterministic --older-than assertions.
    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 14, 12, 0, 0).unwrap()
    }

    // --- resolve_older_than ------------------------------------------------

    #[test]
    fn older_than_minutes() {
        let cutoff = resolve_older_than("45m", now()).unwrap();
        assert_eq!(
            cutoff,
            Utc.with_ymd_and_hms(2026, 5, 14, 11, 15, 0).unwrap()
        );
    }

    #[test]
    fn older_than_hours() {
        let cutoff = resolve_older_than("2h", now()).unwrap();
        assert_eq!(cutoff, Utc.with_ymd_and_hms(2026, 5, 14, 10, 0, 0).unwrap());
    }

    #[test]
    fn older_than_days() {
        // 2026-05-14 minus 30 days = 2026-04-14.
        let cutoff = resolve_older_than("30d", now()).unwrap();
        assert_eq!(cutoff, Utc.with_ymd_and_hms(2026, 4, 14, 12, 0, 0).unwrap());
    }

    #[test]
    fn older_than_zero_resolves_to_now() {
        // "0d" is a valid (aggressive) spec — cutoff equals `now`.
        let cutoff = resolve_older_than("0d", now()).unwrap();
        assert_eq!(cutoff, now());
    }

    #[test]
    fn older_than_is_trimmed() {
        let cutoff = resolve_older_than("  2h  ", now()).unwrap();
        assert_eq!(cutoff, Utc.with_ymd_and_hms(2026, 5, 14, 10, 0, 0).unwrap());
    }

    #[test]
    fn older_than_rejects_unknown_unit() {
        let err = resolve_older_than("5y", now()).unwrap_err();
        assert!(
            err.contains("5y"),
            "message should echo the bad spec: {err}"
        );
    }

    #[test]
    fn older_than_rejects_missing_number() {
        let err = resolve_older_than("d", now()).unwrap_err();
        assert!(err.contains("\"d\""));
    }

    #[test]
    fn older_than_rejects_missing_unit() {
        // "30" with no unit: the trailing '0' is not a valid unit char.
        let err = resolve_older_than("30", now()).unwrap_err();
        assert!(err.contains("\"30\""));
    }

    #[test]
    fn older_than_rejects_negative_number() {
        // '-5' does not parse as u64, so this is rejected at the number step.
        let err = resolve_older_than("-5d", now()).unwrap_err();
        assert!(err.contains("\"-5d\""));
    }

    #[test]
    fn older_than_rejects_empty() {
        let err = resolve_older_than("", now()).unwrap_err();
        assert!(err.contains("empty"));
    }

    #[test]
    fn older_than_rejects_non_numeric_amount() {
        let err = resolve_older_than("abcd", now()).unwrap_err();
        assert!(err.contains("\"abcd\""));
    }

    // --- resolve_before ----------------------------------------------------

    #[test]
    fn before_accepts_rfc3339() {
        let cutoff = resolve_before("2026-04-15T10:30:00Z").unwrap();
        assert_eq!(
            cutoff,
            Utc.with_ymd_and_hms(2026, 4, 15, 10, 30, 0).unwrap()
        );
    }

    #[test]
    fn before_accepts_rfc3339_with_offset() {
        // +02:00 offset normalizes to 08:30 UTC.
        let cutoff = resolve_before("2026-04-15T10:30:00+02:00").unwrap();
        assert_eq!(cutoff, Utc.with_ymd_and_hms(2026, 4, 15, 8, 30, 0).unwrap());
    }

    #[test]
    fn before_accepts_naive_datetime_space_separated() {
        let cutoff = resolve_before("2026-04-15 10:30:00").unwrap();
        assert_eq!(
            cutoff,
            Utc.with_ymd_and_hms(2026, 4, 15, 10, 30, 0).unwrap()
        );
    }

    #[test]
    fn before_accepts_naive_datetime_t_separated() {
        let cutoff = resolve_before("2026-04-15T10:30:00").unwrap();
        assert_eq!(
            cutoff,
            Utc.with_ymd_and_hms(2026, 4, 15, 10, 30, 0).unwrap()
        );
    }

    #[test]
    fn before_accepts_bare_date_as_utc_midnight() {
        let cutoff = resolve_before("2026-04-15").unwrap();
        assert_eq!(cutoff, Utc.with_ymd_and_hms(2026, 4, 15, 0, 0, 0).unwrap());
    }

    #[test]
    fn before_is_trimmed() {
        let cutoff = resolve_before("  2026-04-15  ").unwrap();
        assert_eq!(cutoff, Utc.with_ymd_and_hms(2026, 4, 15, 0, 0, 0).unwrap());
    }

    #[test]
    fn before_rejects_garbage() {
        let err = resolve_before("not-a-date").unwrap_err();
        assert!(err.contains("not-a-date"));
    }

    #[test]
    fn before_rejects_impossible_date() {
        let err = resolve_before("2026-13-99").unwrap_err();
        assert!(err.contains("2026-13-99"));
    }

    #[test]
    fn before_rejects_empty() {
        let err = resolve_before("").unwrap_err();
        assert!(err.contains("invalid datetime"));
    }

    // --- resolve_cutoff routing -------------------------------------------

    #[test]
    fn cutoff_routes_to_older_than_when_only_that_is_set() {
        let args = PruneArgs {
            older_than: Some("1d".to_string()),
            before: None,
            yes: false,
        };
        // Can't pin `now` through resolve_cutoff, but we can assert the
        // cutoff is in the past relative to a freshly-taken now.
        let cutoff = resolve_cutoff(&args).unwrap();
        assert!(cutoff < Utc::now());
    }

    #[test]
    fn cutoff_routes_to_before_when_only_that_is_set() {
        let args = PruneArgs {
            older_than: None,
            before: Some("2026-04-15".to_string()),
            yes: false,
        };
        let cutoff = resolve_cutoff(&args).unwrap();
        assert_eq!(cutoff, Utc.with_ymd_and_hms(2026, 4, 15, 0, 0, 0).unwrap());
    }

    #[test]
    fn cutoff_rejects_both_flags_set() {
        // Defensive: clap's ArgGroup normally prevents this from ever
        // reaching resolve_cutoff, but the function must not panic if it does.
        let args = PruneArgs {
            older_than: Some("1d".to_string()),
            before: Some("2026-04-15".to_string()),
            yes: false,
        };
        let err = resolve_cutoff(&args).unwrap_err();
        assert!(err.contains("mutually exclusive"));
    }

    #[test]
    fn cutoff_rejects_neither_flag_set() {
        let args = PruneArgs {
            older_than: None,
            before: None,
            yes: false,
        };
        let err = resolve_cutoff(&args).unwrap_err();
        assert!(err.contains("required"));
    }

    #[test]
    fn cutoff_propagates_resolution_errors() {
        let args = PruneArgs {
            older_than: Some("nonsense".to_string()),
            before: None,
            yes: false,
        };
        assert!(resolve_cutoff(&args).is_err());
    }

    // --- plural_entries ----------------------------------------------------

    #[test]
    fn plural_entries_singular_and_plural() {
        assert_eq!(plural_entries(1), "entry");
        assert_eq!(plural_entries(0), "entries");
        assert_eq!(plural_entries(2), "entries");
    }
}
