//! `logdive` CLI entry point.
//!
//! Milestones wired up:
//!   - m6: `ingest` — read structured JSON logs from a file or stdin,
//!     parse, and insert batched entries into the index.
//!   - m7: `query` — parse a query string, run it against the index,
//!     and render matching entries to stdout in `pretty` or `json`
//!     form.
//!   - m7: `stats` — report aggregate metadata about the index.
//!
//! Database-opening policy is per-subcommand rather than hoisted into
//! `run()`: `ingest` and `query` create the index file as needed (the
//! user expects to be able to ingest into a fresh DB, or query one
//! they've just created), while `stats` refuses to run against a
//! non-existent path so typos in `--db` surface as errors rather than
//! misleading zero-entry readouts.

mod render;
mod stats_cmd;

use std::fs::File;
use std::io::{self, BufRead, BufReader, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use logdive_core::{
    BATCH_SIZE, Indexer, InsertStats, LogEntry, LogdiveError, QueryParseError, Result, db_path,
    execute, parse_line, parse_query,
};

use crate::render::{OutputFormat, render};
use crate::stats_cmd::{StatsArgs, run_stats};

// ---------------------------------------------------------------------------
// CLI grammar
// ---------------------------------------------------------------------------

#[derive(Parser, Debug)]
#[command(
    name = "logdive",
    version,
    about = "Fast, self-hosted query engine for structured JSON logs",
    long_about = None,
)]
struct Cli {
    /// Path to the index database. Defaults to ~/.logdive/index.db.
    ///
    /// Global so every subcommand can override it uniformly.
    #[arg(long, global = true, value_name = "PATH")]
    db: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Ingest structured JSON log lines from a file or stdin into the index.
    Ingest(IngestArgs),
    /// Run a query against the index and render matching entries.
    Query(QueryArgs),
    /// Report aggregate metadata about the index.
    Stats(StatsArgs),
}

#[derive(clap::Args, Debug)]
struct IngestArgs {
    /// File to read from. If omitted, lines are read from stdin.
    #[arg(long, short = 'f', value_name = "PATH")]
    file: Option<PathBuf>,

    /// Tag applied to each ingested entry whose JSON does not already
    /// carry a `tag` field. Optional; unset means "untagged".
    #[arg(long, short = 't', value_name = "TAG")]
    tag: Option<String>,
}

#[derive(clap::Args, Debug)]
struct QueryArgs {
    /// Query expression, e.g. `level=error AND service=payments last 2h`.
    ///
    /// Multi-token queries must be quoted on the shell.
    #[arg(value_name = "QUERY")]
    query: String,

    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Pretty)]
    format: OutputFormat,

    /// Maximum number of results to return. Use `0` for unlimited.
    #[arg(long, default_value_t = 1000)]
    limit: usize,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> ExitCode {
    init_tracing();
    let cli = Cli::parse();

    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            report_error(&e);
            ExitCode::FAILURE
        }
    }
}

/// Render an error message for the user. `QueryParseError` is surfaced
/// with a "query error:" prefix so users can tell parse failures apart
/// from I/O or storage failures. Caret-rendering against the source
/// string is deferred to milestone 9 polish.
fn report_error(e: &LogdiveError) {
    if let LogdiveError::QueryParse(qpe) = e {
        let qpe: &QueryParseError = qpe;
        eprintln!("logdive: query error: {qpe}");
    } else {
        eprintln!("logdive: {e}");
    }
}

fn init_tracing() {
    // LOGDIVE_LOG controls verbosity of internal diagnostics. Default to
    // `warn` so the CLI is quiet during normal use; users set
    // `LOGDIVE_LOG=info` or `=debug` when troubleshooting.
    let filter = EnvFilter::try_from_env("LOGDIVE_LOG").unwrap_or_else(|_| EnvFilter::new("warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(io::stderr)
        .init();
}

fn run(cli: Cli) -> Result<()> {
    let db = db_path(cli.db.as_deref());

    // Each subcommand decides for itself how to treat the DB path — see
    // the module-level note on the rationale.
    match cli.command {
        Command::Ingest(args) => {
            let mut indexer = Indexer::open(&db)?;
            run_ingest(&mut indexer, args)
        }
        Command::Query(args) => {
            let indexer = Indexer::open(&db)?;
            run_query(&indexer, args)
        }
        Command::Stats(args) => run_stats(&db, args),
    }
}

// ---------------------------------------------------------------------------
// Ingest
// ---------------------------------------------------------------------------

/// Aggregate counters and timing for an ingestion run.
#[derive(Default, Debug)]
struct IngestReport {
    inserted: usize,
    deduplicated: usize,
    skipped_no_timestamp: usize,
    /// Lines that failed JSON parsing (returned `None` from `parse_line`).
    malformed: usize,
    /// Lines that couldn't even be read (I/O error mid-stream). We count
    /// these rather than aborting so a corrupt tail doesn't discard a
    /// good head.
    io_errors: usize,
    /// Wall-clock time across the whole ingestion.
    elapsed: Duration,
}

impl IngestReport {
    fn fold_insert_stats(&mut self, s: InsertStats) {
        self.inserted += s.inserted;
        self.deduplicated += s.deduplicated;
        self.skipped_no_timestamp += s.skipped_no_timestamp;
    }

    fn total_seen(&self) -> usize {
        self.inserted
            + self.deduplicated
            + self.skipped_no_timestamp
            + self.malformed
            + self.io_errors
    }
}

fn run_ingest(indexer: &mut Indexer, args: IngestArgs) -> Result<()> {
    let tag = args.tag;

    // Open the source. `match` rather than `if let` so both arms have
    // the same bound type (`Box<dyn BufRead>`).
    let reader: Box<dyn BufRead> = match args.file {
        Some(ref p) => {
            let f = File::open(p).map_err(|e| LogdiveError::io_at(p.clone(), e))?;
            Box::new(BufReader::new(f))
        }
        None => Box::new(BufReader::new(io::stdin().lock())),
    };

    let tty = io::stderr().is_terminal();
    let mut progress = Progress::new(tty);

    let mut report = IngestReport::default();
    let started = Instant::now();

    let mut batch: Vec<LogEntry> = Vec::with_capacity(BATCH_SIZE);

    for line_result in reader.lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(_) => {
                // Mid-stream I/O error: log, count, move on. This can
                // happen on truncated UTF-8 or a closed pipe; either way
                // aborting would discard earlier successfully-ingested
                // entries in the same batch.
                report.io_errors += 1;
                continue;
            }
        };

        match parse_line(&line) {
            Some(entry) => batch.push(entry.with_tag(tag.clone())),
            None if line.trim().is_empty() => {
                // Blank lines are not malformed — they're just noise.
                // Don't count them.
            }
            None => report.malformed += 1,
        }

        if batch.len() >= BATCH_SIZE {
            let stats = indexer.insert_batch(&batch)?;
            report.fold_insert_stats(stats);
            batch.clear();
            progress.tick(&report, started.elapsed());
        }
    }

    // Flush remainder.
    if !batch.is_empty() {
        let stats = indexer.insert_batch(&batch)?;
        report.fold_insert_stats(stats);
    }

    report.elapsed = started.elapsed();
    progress.finish(&report);
    print_summary(&report);
    Ok(())
}

// ---------------------------------------------------------------------------
// Query
// ---------------------------------------------------------------------------

fn run_query(indexer: &Indexer, args: QueryArgs) -> Result<()> {
    // Parse: errors produce `LogdiveError::QueryParse` via the `From` impl
    // in the unified error type, and are surfaced by `report_error` with a
    // distinguishing prefix.
    let ast = parse_query(&args.query)?;

    // `--limit 0` → unlimited. Any other positive value → capped.
    let limit = if args.limit == 0 {
        None
    } else {
        Some(args.limit)
    };

    tracing::debug!(?limit, "executing query");
    let rows = execute(&ast, indexer.connection(), limit)?;
    tracing::debug!(result_count = rows.len(), "query returned results");

    render(&rows, args.format)
}

// ---------------------------------------------------------------------------
// Progress output (ingest)
// ---------------------------------------------------------------------------

/// Throttled progress renderer. On a TTY it rewrites one line via `\r`;
/// otherwise it prints a newline-separated entry at most once per second,
/// which keeps pipe-to-file output readable.
struct Progress {
    tty: bool,
    last_tick: Instant,
    tick_interval: Duration,
}

impl Progress {
    fn new(tty: bool) -> Self {
        Self {
            tty,
            // Set far enough in the past that the first tick always prints.
            last_tick: Instant::now() - Duration::from_secs(3600),
            // Quarter-second refresh is fluid on TTY, rare enough for files.
            tick_interval: Duration::from_millis(250),
        }
    }

    fn tick(&mut self, report: &IngestReport, elapsed: Duration) {
        // Rate-limit: the indexer batches at 1000/line, so ticks come
        // once per batch — still want to throttle tiny batches on slow
        // inputs.
        if self.last_tick.elapsed() < self.tick_interval {
            return;
        }
        self.last_tick = Instant::now();
        self.render(report, elapsed);
    }

    fn finish(&mut self, report: &IngestReport) {
        // Always emit a final tick so the last-seen counters reflect
        // reality, then close out the line on a TTY.
        self.render(report, report.elapsed);
        if self.tty {
            // The render loop uses `\r` without a trailing newline — add
            // one here so the summary appears on its own line.
            eprintln!();
        }
    }

    fn render(&self, report: &IngestReport, elapsed: Duration) {
        let rate = lines_per_sec(report.total_seen(), elapsed);
        let payload = format!(
            "ingesting: {total:>7} seen | {ins:>7} new | {dedup:>5} dup | {bad:>5} skip | {rate:>7.0} lines/s",
            total = report.total_seen(),
            ins = report.inserted,
            dedup = report.deduplicated,
            bad = report.malformed + report.skipped_no_timestamp + report.io_errors,
            rate = rate,
        );
        if self.tty {
            // `\r` with no newline — next tick overwrites.
            let mut err = io::stderr().lock();
            let _ = write!(err, "\r{payload}");
            let _ = err.flush();
        } else {
            eprintln!("{payload}");
        }
    }
}

fn lines_per_sec(n: usize, elapsed: Duration) -> f64 {
    let secs = elapsed.as_secs_f64();
    if secs <= 0.0 { 0.0 } else { n as f64 / secs }
}

fn print_summary(report: &IngestReport) {
    let rate = lines_per_sec(report.total_seen(), report.elapsed);
    eprintln!(
        "ingest complete in {:.2}s ({:.0} lines/s)",
        report.elapsed.as_secs_f64(),
        rate
    );
    eprintln!("  inserted:     {}", report.inserted);
    eprintln!("  deduplicated: {}", report.deduplicated);
    eprintln!("  no timestamp: {}", report.skipped_no_timestamp);
    eprintln!("  malformed:    {}", report.malformed);
    if report.io_errors > 0 {
        eprintln!("  io errors:    {}", report.io_errors);
    }
}
