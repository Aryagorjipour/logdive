//! `logdive` CLI binary.
//!
//! Four subcommands: `ingest`, `query`, `stats`, `prune`. A global `--db`
//! flag selects the index path; all subcommands respect it. The flag also
//! reads the `LOGDIVE_DB` environment variable as a fallback, matching the
//! `logdive-api` binary — the command-line value wins when both are set.
//!
//! # Changes in v0.2.0
//!
//! - `--format json|logfmt|plain` on `ingest` (M2: multi-format ingestion).
//! - `--timestamp-now` on `ingest` (M2: universal fallback timestamp).
//! - `--follow` on `ingest` with `--file` (M3: file tailing, Unix only).
//! - `prune` subcommand (M4: time-based retention).
//! - `LOGDIVE_DB` environment-variable fallback for `--db` (M4).
//!
//! # Changes in v0.3.0
//!
//! - `--output pretty|json` on `query` replaces `--format` (B3: unambiguous
//!   flag name — `--format` on `ingest` is the input format, so reusing it
//!   on `query` for the output format was confusing).
//! - `--limit` and `--offset` on `query` for result-set pagination (B2).

mod prune_cmd;
mod render;
mod stats_cmd;

use std::io::{self, BufRead, IsTerminal};
use std::path::{Path, PathBuf};

use chrono::Utc;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use logdive_core::{
    Indexer, InsertStats, LogEntry, LogFormat, LogdiveError, QueryOptions, Result, db_path,
    execute, parse_line, parse_query,
};
use prune_cmd::{PruneArgs, run_prune};
use render::{OutputFormat, render};
use stats_cmd::{StatsArgs, run_stats};

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

/// Fast, self-hosted query engine for structured JSON logs.
#[derive(Parser, Debug)]
#[command(name = "logdive", version, about, long_about = None)]
struct Cli {
    /// Path to the index database. Defaults to ~/.logdive/index.db.
    ///
    /// Applies to all subcommands. Can also be set via the `LOGDIVE_DB`
    /// environment variable; the command-line flag takes precedence when
    /// both are provided.
    #[arg(long, global = true, value_name = "PATH", env = "LOGDIVE_DB")]
    db: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Ingest structured log lines from a file or stdin into the index.
    Ingest(IngestArgs),
    /// Query the index and render matching log entries.
    Query(QueryArgs),
    /// Report aggregate metadata about the index.
    Stats(StatsArgs),
    /// Delete entries older than a cutoff, then VACUUM the database.
    Prune(PruneArgs),
}

/// Arguments for the `ingest` subcommand.
#[derive(clap::Args, Debug)]
pub struct IngestArgs {
    /// Read from this file instead of stdin.
    #[arg(long, short = 'f', value_name = "PATH")]
    file: Option<PathBuf>,

    /// Attach a tag to every ingested entry that lacks a `tag` field.
    #[arg(long, short = 't', value_name = "TAG")]
    tag: Option<String>,

    /// Input format of the log lines.
    ///
    /// `json` (default) expects newline-delimited JSON objects.
    /// `logfmt` expects `key=value` pairs.
    /// `plain` treats each line as an unstructured message.
    #[arg(
        long,
        value_name = "FORMAT",
        default_value = "json",
        value_parser = parse_log_format
    )]
    format: LogFormat,

    /// Stamp the current ingestion time on entries that have no timestamp.
    ///
    /// Without this flag, timestamp-less entries are silently skipped
    /// (no-fabrication policy). Most useful with `--format plain`.
    #[arg(long)]
    timestamp_now: bool,

    /// Watch the file for newly appended lines and ingest them continuously.
    ///
    /// Requires `--file`. Stdin already streams until EOF; `--follow` is
    /// not needed and is rejected with an actionable error message.
    ///
    /// Unix only. Exits cleanly on Ctrl-C.
    #[arg(long, requires = "file")]
    follow: bool,

    /// Exit the follow loop after this many filesystem events.
    ///
    /// Hidden flag for deterministic testing of the watch loop; not
    /// intended for end-user use.
    #[arg(long, value_name = "N", hide = true)]
    max_events: Option<usize>,
}

/// Arguments for the `query` subcommand.
#[derive(clap::Args, Debug)]
struct QueryArgs {
    /// Query expression (e.g. `level=error AND service=payments last 2h`).
    query: String,

    /// Output format. `pretty` (default) is human-readable and optionally
    /// colored; `json` emits newline-delimited JSON suitable for piping
    /// into `jq` or other tools.
    #[arg(long, value_enum, default_value_t = OutputFormat::Pretty)]
    output: OutputFormat,

    /// Maximum number of results to return. 0 means unlimited.
    #[arg(long, default_value_t = 1000)]
    limit: usize,

    /// Number of results to skip from the front of the ordered result set.
    /// Use with `--limit` to page through large result sets.
    /// 0 (default) starts from the first result.
    #[arg(long, default_value_t = 0)]
    offset: usize,
}

// ---------------------------------------------------------------------------
// Clap value parser for LogFormat (keeps the clap dependency out of core)
// ---------------------------------------------------------------------------

fn parse_log_format(s: &str) -> std::result::Result<LogFormat, String> {
    LogFormat::from_name(s)
        .ok_or_else(|| format!("unknown format {s:?}; expected one of: json, logfmt, plain"))
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    init_tracing();
    let cli = Cli::parse();
    let db = db_path(cli.db.as_deref());

    let result = match cli.command {
        Command::Ingest(args) => handle_ingest(&db, args),
        Command::Query(args) => handle_query(&db, args),
        Command::Stats(args) => run_stats(&db, args),
        Command::Prune(args) => run_prune(&db, args),
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// ingest
// ---------------------------------------------------------------------------

fn handle_ingest(db: &Path, args: IngestArgs) -> Result<()> {
    // --follow guard: Unix only; --file is guaranteed by clap `requires`.
    if args.follow {
        #[cfg(not(unix))]
        return Err(LogdiveError::IoBare(io::Error::other(
            "--follow is only supported on Unix (Linux / macOS)",
        )));

        #[cfg(unix)]
        {
            let path = args
                .file
                .as_deref()
                .expect("clap `requires = \"file\"` ensures --file is always set with --follow");
            return run_watch_loop(path, db, &args);
        }
    }

    // One-shot ingestion from --file or stdin.
    let mut indexer = Indexer::open(db)?;
    let mut total = InsertStats::default();
    let mut malformed: usize = 0;
    let is_tty = io::stderr().is_terminal();

    if let Some(ref path) = args.file {
        let file = std::fs::File::open(path).map_err(|e| LogdiveError::io_at(path, e))?;
        let reader = io::BufReader::new(file);
        ingest_reader(
            reader,
            &mut indexer,
            &args,
            &mut total,
            &mut malformed,
            is_tty,
        )?;
    } else {
        let stdin = io::stdin();
        let reader = stdin.lock();
        ingest_reader(
            reader,
            &mut indexer,
            &args,
            &mut total,
            &mut malformed,
            is_tty,
        )?;
    }

    if is_tty {
        // Move past the progress line.
        eprintln!();
    }
    print_ingest_summary(total, malformed);
    Ok(())
}

/// Read all lines from `reader`, parse them, and insert into the index.
///
/// Applies `--format`, `--timestamp-now`, and `--tag` exactly once per
/// line, matching the one-shot ingest contract. Flushes any leftover
/// partial batch at end of input.
fn ingest_reader<R: BufRead>(
    reader: R,
    indexer: &mut Indexer,
    args: &IngestArgs,
    total: &mut InsertStats,
    malformed: &mut usize,
    is_tty: bool,
) -> Result<()> {
    const BATCH: usize = 1000;
    let mut batch: Vec<LogEntry> = Vec::with_capacity(BATCH);

    for line_result in reader.lines() {
        let line = line_result.map_err(LogdiveError::IoBare)?;
        if line.trim().is_empty() {
            continue;
        }
        match parse_line(args.format, &line) {
            Some(mut entry) => {
                if entry.timestamp.is_none() && args.timestamp_now {
                    entry.timestamp = Some(Utc::now().to_rfc3339());
                }
                entry = entry.with_tag(args.tag.as_deref());
                batch.push(entry);

                if batch.len() >= BATCH {
                    let stats = indexer.insert_batch(&batch)?;
                    accumulate(total, &stats);
                    batch.clear();
                    if is_tty {
                        eprint!(
                            "\r  {} ingested  {} dedup  {} skipped",
                            total.inserted, total.deduplicated, total.skipped_no_timestamp
                        );
                    }
                }
            }
            None => *malformed += 1,
        }
    }

    // Flush the final partial batch.
    if !batch.is_empty() {
        let stats = indexer.insert_batch(&batch)?;
        accumulate(total, &stats);
    }
    Ok(())
}

fn accumulate(total: &mut InsertStats, delta: &InsertStats) {
    total.inserted += delta.inserted;
    total.deduplicated += delta.deduplicated;
    total.skipped_no_timestamp += delta.skipped_no_timestamp;
}

fn print_ingest_summary(stats: InsertStats, malformed: usize) {
    eprintln!(
        "ingested {}  dedup {}  no-timestamp {}  malformed {}",
        stats.inserted, stats.deduplicated, stats.skipped_no_timestamp, malformed
    );
}

// ---------------------------------------------------------------------------
// Follow / watch loop — Unix only
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn run_watch_loop(path: &Path, db: &Path, args: &IngestArgs) -> Result<()> {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::Duration;

    use notify::{RecursiveMode, Watcher, recommended_watcher};

    use logdive_core::FileTailer;

    let mut tailer = FileTailer::open(path)?;

    // Channel carries raw notify results (errors + events alike).
    let (tx, rx) = std::sync::mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = recommended_watcher(move |res| {
        // Ignore send errors — they mean the receiver was dropped (shutdown).
        let _ = tx.send(res);
    })
    .map_err(|e| LogdiveError::IoBare(io::Error::other(e.to_string())))?;

    watcher
        .watch(path, RecursiveMode::NonRecursive)
        .map_err(|e| LogdiveError::IoBare(io::Error::other(e.to_string())))?;

    // Shared stop flag. Ctrl-C handler sets it; main loop checks it.
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = Arc::clone(&stop);
    ctrlc::set_handler(move || stop_clone.store(true, Ordering::SeqCst))
        .map_err(|e| LogdiveError::IoBare(io::Error::other(e.to_string())))?;

    let mut indexer = Indexer::open(db)?;
    let mut total = InsertStats::default();
    let mut malformed: usize = 0;
    let mut event_count: usize = 0;

    eprintln!("following {} — Ctrl-C to stop", path.display());

    while !stop.load(Ordering::SeqCst) {
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(_event) => {
                let lines = tailer.read_new_lines()?;
                ingest_lines(&lines, &mut indexer, args, &mut total, &mut malformed)?;
                event_count += 1;
                if let Some(max) = args.max_events {
                    if event_count >= max {
                        break;
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                // Safety drain: deliver bytes that arrived without an event
                // (can happen under heavy rotation or after a watcher hiccup).
                let lines = tailer.read_new_lines()?;
                if !lines.is_empty() {
                    ingest_lines(&lines, &mut indexer, args, &mut total, &mut malformed)?;
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    print_ingest_summary(total, malformed);
    Ok(())
}

/// Parse and ingest a slice of log-line strings.
///
/// Mirrors `ingest_reader` but works on an already-split `&[String]` so
/// the follow loop can call it without owning a reader.
#[cfg(unix)]
fn ingest_lines(
    lines: &[String],
    indexer: &mut Indexer,
    args: &IngestArgs,
    total: &mut InsertStats,
    malformed: &mut usize,
) -> Result<()> {
    let mut batch: Vec<LogEntry> = Vec::with_capacity(lines.len());

    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        match parse_line(args.format, line) {
            Some(mut entry) => {
                if entry.timestamp.is_none() && args.timestamp_now {
                    entry.timestamp = Some(Utc::now().to_rfc3339());
                }
                entry = entry.with_tag(args.tag.as_deref());
                batch.push(entry);
            }
            None => *malformed += 1,
        }
    }

    if !batch.is_empty() {
        let stats = indexer.insert_batch(&batch)?;
        accumulate(total, &stats);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// query
// ---------------------------------------------------------------------------

fn handle_query(db: &Path, args: QueryArgs) -> Result<()> {
    let ast = parse_query(&args.query)?;
    let opts = QueryOptions {
        limit: match args.limit {
            0 => None,
            n => Some(n),
        },
        offset: match args.offset {
            0 => None,
            n => Some(n),
        },
    };

    let indexer = Indexer::open(db)?;
    let entries = execute(&ast, indexer.connection(), opts)?;
    render(&entries, args.output)
}

// ---------------------------------------------------------------------------
// Tracing initialisation
// ---------------------------------------------------------------------------

fn init_tracing() {
    let filter = EnvFilter::try_from_env("LOGDIVE_LOG").unwrap_or_else(|_| EnvFilter::new("warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(io::stderr)
        .init();
}
