//! `logdive` CLI entry point.
//!
//! Milestone 6 wires up the `ingest` subcommand: read structured JSON logs
//! from a file or stdin, parse each line via `logdive_core::parser`, and
//! insert batched entries into a SQLite-backed index via
//! `logdive_core::indexer`. Progress is printed to stderr so stdout stays
//! free for future subcommands that emit data.
//!
//! Subsequent milestones will add `query` and `stats` subcommands — the
//! dispatch structure below is already set up to accept them without
//! restructuring.

use std::fs::File;
use std::io::{self, BufRead, BufReader, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use logdive_core::{
    db_path, parse_line, Indexer, InsertStats, LogEntry, LogdiveError, Result, BATCH_SIZE,
};

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

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> ExitCode {
    init_tracing();
    let cli = Cli::parse();

    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("logdive: {e}");
            ExitCode::FAILURE
        }
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
    let mut indexer = Indexer::open(&db)?;

    match cli.command {
        Command::Ingest(args) => run_ingest(&mut indexer, args),
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
// Progress output
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
    if secs <= 0.0 {
        0.0
    } else {
        n as f64 / secs
    }
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
