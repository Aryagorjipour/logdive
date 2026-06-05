# operating-rules.md

Complete rules for working on this project. Rules 1–12 are from CLAUDE.md;
rules 13+ are derived from discovered constraints and past incidents.

## Rules

1. **Plan before any non-trivial code.** Write a plan, get explicit approval,
   then execute milestone-by-milestone. Code written before a plan is approved
   will be discarded.

2. **Full files only.** Never deliver diffs or partial snippets with
   "...rest unchanged...". Every changed file ships as its complete new content.
   Use the Write tool for full rewrites; Edit tool for targeted, bounded changes.

3. **Zero placeholders, zero TODOs, zero "// for now" stubs.** If you cannot
   finish something, surface the blocker. Do not paper over it.

4. **One patch per error report.** Fix exactly what was reported. Surface other
   findings as separate items rather than silently fixing them.

5. **Don't invent scope.** If the user didn't ask for it, don't add it.
   A bug fix doesn't need a refactor. An endpoint doesn't need its own module
   if it fits in the existing one.

6. **Flag design decisions upfront.** List options, recommend one with
   justification, get approval, then write code.

7. **Compile checkpoint after every meaningful change.** All four must pass
   before moving to the next file:
   ```
   cargo build --workspace
   cargo test --workspace --no-fail-fast
   cargo clippy --workspace --all-targets -- -D warnings
   cargo fmt --all --check
   ```
   Zero warnings. Zero format diff. Do not move on with a failing checkpoint.

8. **Conventional Commits.** Subject ≤72 chars. Types in use: `feat:`, `fix:`,
   `test:`, `chore:`, `docs:`, `refactor:`. Milestone commits use
   `chore(vX.Y.Z):` or `feat(vX.Y.Z):` with a slug suffix.

9. **Web-search before stating present-day facts** (crate versions, GHA action
   versions, API shapes, Docker base image tags). Do not guess from training
   data — versions move.

10. **`cargo clean` if stale-cache symptoms appear.** Unexplained linker errors,
    incremental compilation panics, or "already defined" symbol errors are
    usually stale incremental cache.

11. **Honor MSRV 1.85.** Do not use features from Rust editions or stable
    releases after 1.85 — the CI msrv job enforces this.

12. **Use the right binary name.** The CLI binary and crate name are both
    `logdive`. The crate *path* is `crates/cli/`. Never call it `cli`.

13. **Test and bench deps go in `[dev-dependencies]`, not `[dependencies]`.**
    `criterion`, `tempfile`, and `proptest` are in `[dev-dependencies]` in
    `crates/cli/Cargo.toml` (fixed in v0.3.0). Do not revert — these must never
    appear in the release dependency graph.

14. **Keep GitHub Actions action versions consistent across all workflow files.**
    All four workflows (`ci.yml`, `docker.yml`, `release.yml`, `audit.yml`)
    pin `actions/checkout@v4` (fixed in v0.3.0). Do not introduce a different
    version without updating all four files simultaneously.

15. **tower-http version must be kept consistent with axum version.** These
    crates share `tower` trait objects and `http` types. `tower-http` is declared
    directly in `crates/api/Cargo.toml` (not inherited from workspace). When
    bumping axum, always check tower-http compatibility and update both together.

16. **Do not break the dedup invariant.** `raw_hash UNIQUE` + `INSERT OR IGNORE`
    is the deduplication contract. Any change to how `raw` is computed or stored
    must be considered a breaking change — existing indexes would stop deduping
    against previously ingested lines.

17. **Do not open the DB read-write from the API.** The API's
    `AppState::with_connection` must always call `Indexer::open_read_only`.
    This is enforced by tests in `state.rs` but must not be weakened.

18. **Parsers follow graceful-skip, not fail-fast.** A line that cannot be
    parsed in the selected format is counted in `malformed` and skipped.
    Never make parse failures fatal to an ingest run.

19. **The `timestamp NOT NULL` schema constraint is intentional.** The indexer
    skips entries with `timestamp = None` rather than fabricating a timestamp.
    Do not change this behavior without explicit discussion — `--timestamp-now`
    is the user-controlled escape hatch.

20. **SQL query construction is always parameterized.** Values are bound via
    `params![]` / `params_from_iter`. Field names for `json_extract()` are
    validated through two layers (`validate_field_name` in parser +
    `is_safe_json_path_segment` in executor). Never interpolate user input
    into SQL text.

21. **`execute()` and `execute_at()` take `QueryOptions`, not bare `Option<usize>`.**
    Introduced in v0.3.0. Pass `QueryOptions { limit, offset: None }` when you
    only need a limit. Adding new pagination-style options goes into `QueryOptions`,
    not as new function parameters.

22. **`idx_level_norm` is idempotent — it runs on every `Indexer::open()`.**
    `CREATE INDEX IF NOT EXISTS idx_level_norm ON log_entries(lower(level))` is
    in `init_schema`. This is intentional: existing databases pick it up on the
    next open without migration. Do not move it to a migration path.
