//! `logdive-api` binary entry point.
//!
//! Reads configuration from command-line flags (with environment-variable
//! fallbacks), fails fast if the configured index does not exist, then
//! hands the built router to `axum::serve` with graceful-shutdown wiring.
//!
//! The actual HTTP surface lives in the `logdive_api` library half of
//! this crate — see `lib.rs` for the module map.

use std::net::SocketAddr;
use std::path::PathBuf;

use clap::Parser;
use tracing_subscriber::EnvFilter;

use logdive_api::router::build_router;
use logdive_api::state::AppState;
use logdive_core::{LogdiveError, Result, db_path};

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser, Debug)]
#[command(
    name = "logdive-api",
    version,
    about = "Read-only HTTP API server for a logdive index",
    long_about = None,
)]
struct Cli {
    /// Path to the index database. Defaults to ~/.logdive/index.db.
    ///
    /// Can also be set via the `LOGDIVE_DB` environment variable; the
    /// command-line value takes precedence when both are provided.
    #[arg(long, value_name = "PATH", env = "LOGDIVE_DB")]
    db: Option<PathBuf>,

    /// Port to listen on.
    ///
    /// Can also be set via `LOGDIVE_API_PORT`. Default 4000.
    #[arg(long, default_value_t = 4000, env = "LOGDIVE_API_PORT")]
    port: u16,

    /// Host/IP to bind to.
    ///
    /// Defaults to `127.0.0.1` — loopback only. Set explicitly to
    /// `0.0.0.0` (or a specific non-loopback address) to expose the
    /// server beyond localhost. Can also be set via `LOGDIVE_API_HOST`.
    #[arg(long, default_value = "127.0.0.1", env = "LOGDIVE_API_HOST")]
    host: String,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let cli = Cli::parse();

    // Resolve the DB path the same way the CLI does, so env/default
    // behavior is consistent across the two surfaces.
    let db = db_path(cli.db.as_deref());

    // Fail fast: no point starting a server that will 500 on every request
    // because the underlying file is absent. Typos surface here rather
    // than as a flurry of client-side errors.
    if !db.exists() {
        let msg = format!(
            "no index found at {}; run `logdive ingest` to create one first",
            db.display()
        );
        return Err(LogdiveError::io_at(
            &db,
            std::io::Error::new(std::io::ErrorKind::NotFound, msg),
        ));
    }

    // Build state and router.
    let state = AppState::new(db.clone());
    let app = build_router(state);

    // Bind. Parsing the host string through `format!` + `parse` keeps the
    // error path uniform: any malformed host goes through `io_at`.
    let addr: SocketAddr =
        format!("{}:{}", cli.host, cli.port)
            .parse()
            .map_err(|e: std::net::AddrParseError| {
                LogdiveError::io_at(
                    &db,
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("invalid host:port `{}:{}`: {e}", cli.host, cli.port),
                    ),
                )
            })?;

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| LogdiveError::io_at(&db, e))?;

    let bound = listener
        .local_addr()
        .map_err(|e| LogdiveError::io_at(&db, e))?;
    tracing::info!(%bound, index = %db.display(), "logdive-api listening");
    eprintln!(
        "logdive-api listening on http://{bound} (index: {})",
        db.display()
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|e| LogdiveError::io_at(&db, e))?;

    tracing::info!("logdive-api shutdown complete");
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn init_tracing() {
    // Match the CLI's default so users have one consistent knob across
    // both surfaces. LOGDIVE_LOG=debug reveals query execution details.
    let filter = EnvFilter::try_from_env("LOGDIVE_LOG").unwrap_or_else(|_| EnvFilter::new("warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
}

/// Future that completes when a shutdown signal arrives.
///
/// Listens for Ctrl-C on all platforms; additionally listens for SIGTERM
/// on Unix so the server shuts down cleanly under `systemctl stop` and
/// `docker stop`. Any `io::Error` from signal setup is swallowed and the
/// corresponding future is replaced by `std::future::pending()` — losing
/// one signal handler shouldn't crash the server at startup.
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::warn!(error = %e, "failed to install Ctrl-C handler");
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut stream) => {
                stream.recv().await;
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to install SIGTERM handler");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("Ctrl-C received, shutting down");
        }
        _ = terminate => {
            tracing::info!("SIGTERM received, shutting down");
        }
    }
}
