# syntax=docker/dockerfile:1
#
# Multi-stage build for logdive.
#
# Stages:
#   chef     – current stable Rust toolchain with cargo-chef installed (shared base)
#   planner  – computes the dependency recipe from manifests + lockfile
#   builder  – cooks dependencies (cacheable layer), then builds binaries
#   runtime  – distroless/cc-debian12:nonroot — no shell, no tools, no root
#
# Both binaries are shipped in one image:
#   - logdive-api  (default ENTRYPOINT — HTTP server)
#   - logdive      (CLI — invoke via: docker run --entrypoint logdive ...)
#
# Data volume: mount a named volume at /data for index persistence.
#   docker run -v logdive-data:/data -p 4000:4000 ghcr.io/aryagorjipour/logdive

# ─────────────────────────────────────────────────────────────────────────────
# Stage 1 — chef
# Current stable Rust (always >= project MSRV of 1.85) with cargo-chef.
# Using rust:1 rather than rust:1.85 because cargo-chef's own dependencies
# require a newer compiler than the project MSRV. MSRV is enforced by CI
# (cargo check / cargo test), not by the Docker builder.
# ─────────────────────────────────────────────────────────────────────────────
FROM rust:1 AS chef
RUN cargo install cargo-chef --locked
WORKDIR /build

# ─────────────────────────────────────────────────────────────────────────────
# Stage 2 — planner
# Reads every Cargo.toml in the workspace plus Cargo.lock and emits a
# recipe.json describing exactly which dependencies need to be compiled.
# This stage is re-run only when the dependency graph changes.
# ─────────────────────────────────────────────────────────────────────────────
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ─────────────────────────────────────────────────────────────────────────────
# Stage 3 — builder
# Step A (cook): compile all workspace dependencies from recipe.json.
#   This layer is cached by Docker/BuildKit and only invalidated when
#   recipe.json changes (i.e. a dep is added, removed, or version-bumped).
#   On a cache hit, step A is skipped entirely — deps are restored in < 1 s.
# Step B (build): compile the two release binaries against the cached deps.
#   Only this step re-runs on pure source changes.
#
# The /data directory is created here — distroless has no shell or mkdir,
# so directory scaffolding must happen in a shell-capable stage. uid/gid
# 65532 is the nonroot user pre-provisioned in the distroless:nonroot image.
# ─────────────────────────────────────────────────────────────────────────────
FROM chef AS builder
COPY --from=planner /build/recipe.json recipe.json
# Step A — cook dependencies (the key caching layer).
RUN cargo chef cook --release --recipe-path recipe.json
# Step B — compile both binaries. Source is copied after cooking so that
# source-only changes don't bust the dependency cache above.
COPY . .
RUN cargo build --release --bin logdive --bin logdive-api
# Pre-create /data owned by the distroless nonroot uid so the index is
# writable when a Docker named volume is mounted at /data on first run.
RUN mkdir -p /data && chown 65532:65532 /data

# ─────────────────────────────────────────────────────────────────────────────
# Stage 4 — runtime
# gcr.io/distroless/cc-debian12:nonroot — minimal image containing only the
# C runtime library (glibc + libgcc). Contains:
#   - the two logdive binaries (SQLite statically linked via rusqlite bundled)
#   - the /data directory scaffold (from builder)
#
# No shell, no curl, no package manager, no toolchain, no source code.
# The nonroot tag runs the process as uid 65532 without any root interaction.
# HEALTHCHECK uses --health-check (TCP connect), not curl.
# ─────────────────────────────────────────────────────────────────────────────
FROM gcr.io/distroless/cc-debian12:nonroot AS runtime

COPY --from=builder /build/target/release/logdive     /usr/local/bin/logdive
COPY --from=builder /build/target/release/logdive-api /usr/local/bin/logdive-api
COPY --from=builder /data /data

# ── Environment defaults ──────────────────────────────────────────────────────

# Index path. Override with --db or LOGDIVE_DB.
ENV LOGDIVE_DB=/data/index.db

# Bind address. Overrides the binary's loopback default (127.0.0.1) so the
# server is reachable via Docker port mapping.
ENV LOGDIVE_API_HOST=0.0.0.0

# ── Networking ────────────────────────────────────────────────────────────────
EXPOSE 4000

# ── Persistent volume ─────────────────────────────────────────────────────────
VOLUME ["/data"]

WORKDIR /data

# ── Health check ──────────────────────────────────────────────────────────────
# Exec form (JSON array) — no shell needed, works in distroless.
# --health-check does a TCP connect to the server's own port and exits 0/1.
# LOGDIVE_API_PORT env var is forwarded automatically by Docker.
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD ["/usr/local/bin/logdive-api", "--health-check"]

# ── Entry point ───────────────────────────────────────────────────────────────
# Default: HTTP API server.
# CLI: docker run --entrypoint logdive ghcr.io/aryagorjipour/logdive <args>
ENTRYPOINT ["/usr/local/bin/logdive-api"]
