#!/bin/sh
# Scan Cargo.lock against the RustSec advisory database.
# Requires cargo-audit: cargo install cargo-audit --locked
# Exits non-zero when any vulnerability is found.
set -eu
cargo audit
