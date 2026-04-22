//! Shared application state for the HTTP server.
//!
//! `AppState` carries the configured database path and offers a uniform
//! helper, [`AppState::with_connection`], for running blocking SQLite work
//! on Tokio's blocking-task pool. Every handler that touches the database
//! routes through this helper so that:
//!   1. The rusqlite dependency stays contained to one module,
//!   2. No handler accidentally blocks the async runtime,
//!   3. Each request gets a fresh read-only connection — matching the
//!      milestone 8 design decision on connection strategy.
//!
//! The read-only connection is opened via [`logdive_core::Indexer::
//! open_read_only`], which enforces SQLite-level `SQLITE_OPEN_READ_ONLY`
//! semantics and fails fast if the DB file is missing (as opposed to
//! creating it, the way `Indexer::open` does).

use std::path::PathBuf;

use logdive_core::{Indexer, LogdiveError, Result};

/// State shared across every HTTP handler.
///
/// Cheap to clone: a single `PathBuf` per instance. Axum requires the
/// state type to be `Clone` so each request handler can get its own
/// owned copy via the `State` extractor.
#[derive(Debug, Clone)]
pub struct AppState {
    /// Absolute or resolved path to the logdive index database.
    ///
    /// Opened read-only per request; never modified by the server.
    pub db_path: PathBuf,
}

impl AppState {
    /// Construct a new `AppState` for the given database path.
    ///
    /// Does not perform any I/O — existence/readability of the file is
    /// checked at startup in `main`, and each request re-opens the file
    /// read-only via [`AppState::with_connection`].
    pub fn new(db_path: PathBuf) -> Self {
        Self { db_path }
    }

    /// Run `f` on Tokio's blocking-task pool with a fresh read-only
    /// [`Indexer`] over the configured database.
    ///
    /// Propagates the closure's result through as-is. Any error from
    /// opening the database, or a blocking-task join failure (which
    /// happens only if the closure itself panics), is folded into
    /// [`LogdiveError`] — handlers map this to an HTTP `AppError` at the
    /// response boundary.
    ///
    /// # Why `spawn_blocking`
    ///
    /// `rusqlite` calls are synchronous and can do real work (microseconds
    /// for a point lookup, milliseconds for a large `LIKE`). Running them
    /// directly inside an async handler would block one of Tokio's worker
    /// threads for the duration of the query, starving other connections.
    /// `spawn_blocking` hands the work to the dedicated blocking pool,
    /// leaving the worker threads free for other async tasks.
    pub async fn with_connection<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Indexer) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let path = self.db_path.clone();
        let join_result = tokio::task::spawn_blocking(move || -> Result<T> {
            let indexer = Indexer::open_read_only(&path)?;
            f(&indexer)
        })
        .await;

        match join_result {
            Ok(inner) => inner,
            Err(join_err) => {
                // `JoinError` from `spawn_blocking` means the closure panicked
                // or the runtime is shutting down. We surface both as an I/O
                // error at the DB path so they have a path context attached,
                // consistent with how other DB-adjacent failures are reported.
                let io_err = std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("blocking task failed: {join_err}"),
                );
                Err(LogdiveError::io_at(&self.db_path, io_err))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn with_connection_runs_closure_and_propagates_result() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("ws.db");

        // Initialize the DB via the core opener (creates schema).
        let _ = Indexer::open(&db).expect("create db");

        let state = AppState::new(db.clone());
        let stats = state
            .with_connection(|idx| idx.stats())
            .await
            .expect("with_connection");
        assert_eq!(stats.entries, 0);
        assert!(stats.tags.is_empty());
    }

    #[tokio::test]
    async fn with_connection_errors_when_db_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.db");

        let state = AppState::new(missing);
        let err = state
            .with_connection(|idx| idx.stats())
            .await
            .expect_err("should fail when db missing");
        // Open_read_only surfaces SQLite's "unable to open" as LogdiveError::Sqlite.
        assert!(matches!(err, LogdiveError::Sqlite(_)));
    }

    #[tokio::test]
    async fn with_connection_uses_read_only_connection() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("ro.db");
        let _ = Indexer::open(&db).unwrap();

        let state = AppState::new(db);
        let result = state
            .with_connection(|idx| {
                // Try to write via the RO connection — must fail.
                idx.connection()
                    .execute(
                        "INSERT INTO log_entries (timestamp, raw, raw_hash) \
                         VALUES ('x', 'y', 'z')",
                        [],
                    )
                    .map_err(LogdiveError::from)
            })
            .await;
        assert!(result.is_err(), "expected RO write rejection");
    }

    #[tokio::test]
    async fn with_connection_surfaces_panic_as_io_error() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("panic.db");
        let _ = Indexer::open(&db).unwrap();

        let state = AppState::new(db);
        let err = state
            .with_connection(|_idx| -> Result<()> { panic!("intentional test panic") })
            .await
            .expect_err("panic should propagate as error, not silent success");
        assert!(matches!(err, LogdiveError::Io { .. }));
    }
}
