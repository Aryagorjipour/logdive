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

use axum::http::HeaderValue;
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

    /// Comma-separated list of origins allowed to make cross-origin requests.
    ///
    /// Use `*` as the sole value to allow any origin. Omit the flag (or
    /// leave it empty) to disable CORS entirely — same-origin requests
    /// are always served regardless of this setting. Invalid values cause
    /// a startup error.
    ///
    /// Examples:
    ///   --cors-origins '*'
    ///   --cors-origins 'https://app.example.com,https://staging.example.com'
    ///
    /// Can also be set via `LOGDIVE_API_CORS_ORIGINS`; the command-line
    /// value takes precedence when both are provided.
    #[arg(long, value_name = "ORIGINS", env = "LOGDIVE_API_CORS_ORIGINS")]
    cors_origins: Option<String>,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let cli = Cli::parse();

    // Validate CORS config before touching the filesystem — a bad origin
    // string is a configuration error that should surface immediately, not
    // after the DB existence check.
    let cors_origins = parse_cors_origins(cli.cors_origins).unwrap_or_else(|msg| {
        eprintln!("error: invalid --cors-origins: {msg}");
        std::process::exit(1);
    });

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
    let app = build_router(state, cors_origins.clone());

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

    let cors_desc = cors_summary(&cors_origins);
    tracing::info!(
        %bound,
        index = %db.display(),
        cors = %cors_desc,
        "logdive-api listening",
    );
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
// CORS origin parsing
// ---------------------------------------------------------------------------

/// Parse the raw `--cors-origins` string into a list of [`HeaderValue`]s
/// ready for [`build_router`].
///
/// Accepts a comma-separated list of origins. Trims whitespace around each
/// entry and ignores empty tokens, so `"a, b,"` and `"a,b"` are equivalent.
///
/// Rules:
/// - `None` or an all-whitespace/empty string → CORS disabled (`[]`).
/// - A single `*` → any origin allowed.
/// - `*` mixed with other values → error (meaningless and likely a mistake).
/// - Each specific origin must be a valid HTTP header value; invalid bytes
///   or control characters → error naming the offending origin.
///
/// Errors are returned as human-readable strings for display at startup.
fn parse_cors_origins(raw: Option<String>) -> std::result::Result<Vec<HeaderValue>, String> {
    let Some(s) = raw else {
        return Ok(vec![]);
    };

    let parts: Vec<&str> = s
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    if parts.is_empty() {
        return Ok(vec![]);
    }

    // Wildcard must be the sole value — mixing it with specific origins is
    // both spec-meaningless and likely a configuration mistake.
    if parts.contains(&"*") {
        if parts.len() != 1 {
            return Err(
                "`*` (allow any origin) cannot be combined with specific origins; \
                 use either `*` alone or a list of explicit origins"
                    .to_string(),
            );
        }
        // Use from_static rather than from_str so the byte sequence is
        // guaranteed to match the `b"*"` check in `build_cors_layer`.
        return Ok(vec![HeaderValue::from_static("*")]);
    }

    parts
        .iter()
        .map(|origin| {
            HeaderValue::from_str(origin).map_err(|_| {
                format!(
                    "`{origin}` is not a valid HTTP header value \
                     (check for control characters or non-ASCII bytes)"
                )
            })
        })
        .collect()
}

/// One-line CORS summary for the startup tracing span.
fn cors_summary(origins: &[HeaderValue]) -> String {
    match origins {
        [] => "disabled".to_string(),
        [star] if star.as_bytes() == b"*" => "any origin (*)".to_string(),
        _ => format!("{} specific origin(s)", origins.len()),
    }
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cors_origins_none_returns_empty() {
        assert!(parse_cors_origins(None).unwrap().is_empty());
    }

    #[test]
    fn parse_cors_origins_empty_string_returns_empty() {
        assert!(parse_cors_origins(Some(String::new())).unwrap().is_empty());
    }

    #[test]
    fn parse_cors_origins_whitespace_only_returns_empty() {
        assert!(
            parse_cors_origins(Some("  , , ".to_string()))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn parse_cors_origins_wildcard_returns_single_star_header() {
        let result = parse_cors_origins(Some("*".to_string())).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].as_bytes(), b"*");
    }

    #[test]
    fn parse_cors_origins_wildcard_with_whitespace_is_accepted() {
        let result = parse_cors_origins(Some("  *  ".to_string())).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].as_bytes(), b"*");
    }

    #[test]
    fn parse_cors_origins_wildcard_mixed_with_origin_is_error() {
        let err = parse_cors_origins(Some("*,https://example.com".to_string())).unwrap_err();
        assert!(err.contains('*'), "error message must mention the wildcard");
    }

    #[test]
    fn parse_cors_origins_single_specific_origin() {
        let result = parse_cors_origins(Some("https://app.example.com".to_string())).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "https://app.example.com");
    }

    #[test]
    fn parse_cors_origins_multiple_specific_origins() {
        let result = parse_cors_origins(Some(
            "https://app.example.com, https://staging.example.com".to_string(),
        ))
        .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "https://app.example.com");
        assert_eq!(result[1], "https://staging.example.com");
    }

    #[test]
    fn parse_cors_origins_trims_whitespace_around_each_entry() {
        let result = parse_cors_origins(Some(
            "  https://a.example.com , https://b.example.com  ".to_string(),
        ))
        .unwrap();
        assert_eq!(result[0], "https://a.example.com");
        assert_eq!(result[1], "https://b.example.com");
    }

    #[test]
    fn parse_cors_origins_invalid_header_value_is_error() {
        // Newline characters are forbidden in header values.
        let err = parse_cors_origins(Some("https://ok.com,bad\nvalue".to_string())).unwrap_err();
        assert!(
            err.contains("bad\nvalue") || err.contains("bad"),
            "error must identify the offending origin"
        );
    }

    #[test]
    fn cors_summary_disabled() {
        assert_eq!(cors_summary(&[]), "disabled");
    }

    #[test]
    fn cors_summary_wildcard() {
        assert_eq!(
            cors_summary(&[HeaderValue::from_static("*")]),
            "any origin (*)"
        );
    }

    #[test]
    fn cors_summary_specific_origins() {
        let origins: Vec<HeaderValue> = vec![
            "https://a.example.com".parse().unwrap(),
            "https://b.example.com".parse().unwrap(),
        ];
        assert_eq!(cors_summary(&origins), "2 specific origin(s)");
    }
}
