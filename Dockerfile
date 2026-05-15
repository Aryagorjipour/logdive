# syntax=docker/dockerfile:1
#
# Multi-stage build for logdive.
#
# Stages:
#   chef     – current stable Rust toolchain with cargo-chef installed (shared base)
#   planner  – computes the dependency recipe from manifests + lockfile
#   builder  – cooks dependencies (cacheable layer), then builds binaries
#   runtime  – minimal debian:bookworm-slim image; no toolchain, no sources
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
# ─────────────────────────────────────────────────────────────────────────────
FROM chef AS builder
COPY --from=planner /build/recipe.json recipe.json
# Step A — cook dependencies (the key caching layer).
RUN cargo chef cook --release --recipe-path recipe.json
# Step B — compile both binaries. Source is copied after cooking so that
# source-only changes don't bust the dependency cache above.
COPY . .
RUN cargo build --release --bin logdive --bin logdive-api

# ─────────────────────────────────────────────────────────────────────────────
# Stage 4 — runtime
# Minimal debian:bookworm-slim. Contains only:
#   - the two logdive binaries
#   - curl  (HEALTHCHECK)
#   - ca-certificates (TLS verification for any future HTTPS use)
# No Rust toolchain, no build artefacts, no source code.
# rusqlite is compiled with the "bundled" feature — SQLite is statically
# linked into the binary, so no libsqlite3 runtime dep is needed.
# ─────────────────────────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
    && rm -rf /var/lib/apt/lists/*

# Non-root system user for container security.
RUN groupadd --system --gid 1000 logdive \
    && useradd --system --uid 1000 --gid logdive \
               --no-create-home --shell /usr/sbin/nologin \
               logdive

# Data directory owned by the runtime user so it's writable when a host
# volume is freshly mounted (before chown runs on the host side).
RUN mkdir -p /data && chown logdive:logdive /data

COPY --from=builder /build/target/release/logdive     /usr/local/bin/logdive
COPY --from=builder /build/target/release/logdive-api /usr/local/bin/logdive-api

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

USER logdive
WORKDIR /data

# ── Health check ──────────────────────────────────────────────────────────────
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -sf "http://127.0.0.1:${LOGDIVE_API_PORT:-4000}/version" || exit 1

# ── Entry point ───────────────────────────────────────────────────────────────
# Default: HTTP API server.
# CLI: docker run --entrypoint logdive ghcr.io/aryagorjipour/logdive <args>
ENTRYPOINT ["/usr/local/bin/logdive-api"]