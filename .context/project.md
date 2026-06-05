# project.md

## Identity

- **Name**: logdive
- **Repository**: https://github.com/Aryagorjipour/logdive
- **License**: MIT OR Apache-2.0
- **crates.io**: https://crates.io/crates/logdive (CLI), https://crates.io/crates/logdive-api, https://crates.io/crates/logdive-core
- **Docs**: https://docs.rs/logdive-core
- **Docker registry**: ghcr.io/aryagorjipour/logdive
- **Landing page**: https://aryagorjipour.github.io/logdive/ (GitHub Pages, Astro 5)
- **Current version**: 0.3.0 (released 2026-06-05)
- **Next planned**: v0.4.0 (performance / benchmarks / speed focus)
- **Long-term target**: v1.0.0 (stable API + complete docs) → triggers Show HN

## What it is

logdive is a single-binary structured log query engine. You point it at a JSON,
logfmt, or plain-text log file; it parses and indexes every line into a local
SQLite database; you query with a typed expression language that supports AND,
OR, parenthesised groups, field comparisons, substring search, and relative or
absolute time ranges. A companion binary (`logdive-api`) exposes the same query
capability as a read-only HTTP server. No infrastructure, no daemons, no cloud
accounts.

## Target user

Backend engineer running a side project or small team service. They have logs
on disk or piped from Docker. They want to filter, search, and time-range-slice
them faster than `grep | jq` chains allow, without spinning up Loki, Datadog,
or Elastic — all of which require infrastructure they don't have, cost they
don't want, or configuration they don't have time for. They're comfortable with
a CLI and a `curl` one-liner.

## Why Rust

Three concrete reasons tied to this project's needs:

1. **Single static binary** — the `bundled` feature of rusqlite links SQLite
   directly into the binary. No runtime dependencies, no system library
   version mismatch, trivial Docker image construction.
2. **Release binary size** — with LTO + strip + `panic = "abort"`, logdive
   ships at 3.8 MB and logdive-api at 4.1 MB. An equivalent Python or Go
   solution would be 10–50× larger for comparable functionality.
3. **Synchronous CLI, async API from one codebase** — the CLI runs fully
   synchronous (no tokio), keeping ingest I/O simple and debuggable. The API
   uses tokio + axum without pulling that weight into the CLI binary.

## Maintainer

Arya Gorjipour (GitHub: @Aryagorjipour). Iranian backend engineer, bilingual
Farsi/English. Senior in C# and Go; working learner in Rust. Has solid mental
models of ownership and lifetimes but benefits from explanations of what the
borrow checker is enforcing in the *specific case* at hand — not generic Rust
101. Prefers `impl Trait` explained as "a concrete type the compiler fills in"
rather than "like generics but different."

**Sanctions-aware infra**: Never recommend AWS, Stripe, GitHub Copilot, Vercel,
or any service that blocks access from Iran. Prefer Fly.io, Hetzner,
Cloudflare, self-hosted, crates.io, GHCR.

**Strategic purpose**: logdive is Arya's Rust flagship — the primary evidence
artifact for UK Global Talent / EU consulting / international hiring
conversations over 2–3 years. Treat quality and public reputation seriously;
scope creep, half-shipped features, or mediocre docs are particularly costly.

## Versioning path

| Version | Focus | Status |
|---|---|---|
| v0.1.0 | Initial release — ingest, query, stats, HTTP API | shipped 2026-04-19 |
| v0.2.0 | OR queries, logfmt, follow mode, prune, CORS, Docker | shipped 2026-05-15 |
| v0.2.1 | Security tests, functional tests, supply-chain hardening | shipped 2026-06-01 |
| v0.3.0 | Parens, pagination, case-insensitive level, distroless | shipped 2026-06-05 |
| v0.4.0 | Performance / benchmarks / speed | in planning |
| v1.0.0 | Stable API + complete docs → Show HN trigger | target ~2027-03-31 |

Show HN is deferred to v1.0.0. Article-first strategy (dev.to) to build
audience before the launch. v2+ ideas (plugin system, marketplace) are captured
privately and will not move to active milestones until v1.0.0 ships and real
user feedback exists.

## Non-goals (permanent)

These will not be implemented. Do not propose them without explicit re-opening
by Arya:

- **Authentication on the HTTP API** — the API trusts its network layer; auth
  belongs in a reverse proxy in front of it
- **Ingestion over HTTP** — the API is read-only by design; CLI handles writes
- **Multi-machine or networked indexes** — single-host only; no replication
- **Real-time analytics or aggregation at scale** — logdive is a query tool,
  not a streaming analytics engine; use Loki or ClickHouse
- **Log shipping, agents, or daemons** — logdive is a tool you invoke, not a
  service that runs continuously
- **A browser UI** — curl and the CLI are the intended interfaces; third parties
  can build UIs against the HTTP API
- **A hosted version** — none planned, ever
