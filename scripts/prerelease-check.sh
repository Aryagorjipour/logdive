#!/usr/bin/env sh
# Pre-release verification for logdive.
#
# Runs the full battery of checks that must pass before cutting a
# release tag. Intended to be run locally by the release manager; the
# CI workflow already runs the equivalent of most of these on every
# push, but this script adds a few publish-specific checks and gives
# a single pass/fail summary for release readiness.
#
# Exit code 0 means "safe to tag and publish v$VERSION". Anything
# non-zero means at least one check failed and the output above
# explains which.

set -eu

# ---------------------------------------------------------------------
# Discover the workspace version from the root Cargo.toml.
# ---------------------------------------------------------------------

if [ ! -f "Cargo.toml" ]; then
  echo "error: run this script from the repo root (no Cargo.toml here)" >&2
  exit 1
fi

# Read `version = "X.Y.Z"` from [workspace.package]. Grep is enough —
# the manifest is ours and we control its shape.
VERSION=$(grep -E '^version\s*=\s*"[^"]+"' Cargo.toml | head -1 | sed -E 's/^version\s*=\s*"([^"]+)"/\1/')

if [ -z "$VERSION" ]; then
  echo "error: could not read workspace version from Cargo.toml" >&2
  exit 1
fi

echo "=============================================================="
echo "logdive pre-release check"
echo "=============================================================="
echo "Workspace version: $VERSION"
echo "Proposed tag:      v$VERSION"
echo ""

# ---------------------------------------------------------------------
# Helpers.
# ---------------------------------------------------------------------

step() {
  echo ""
  echo "--------------------------------------------------------------"
  echo "[step] $1"
  echo "--------------------------------------------------------------"
}

fail() {
  echo ""
  echo "=============================================================="
  echo "FAILED: $1"
  echo "=============================================================="
  exit 1
}

# ---------------------------------------------------------------------
# 1. Clean working tree.
# ---------------------------------------------------------------------

step "Verifying clean working tree"
if [ -n "$(git status --porcelain)" ]; then
  git status --short
  fail "Working tree has uncommitted changes. Commit or stash first."
fi
echo "OK: working tree is clean."

# ---------------------------------------------------------------------
# 2. On main branch.
# ---------------------------------------------------------------------

step "Verifying branch"
BRANCH=$(git rev-parse --abbrev-ref HEAD)
if [ "$BRANCH" != "main" ]; then
  echo "warning: current branch is '$BRANCH', not 'main'."
  echo "         proceeding anyway — flag if this was a mistake."
else
  echo "OK: on main."
fi

# ---------------------------------------------------------------------
# 3. Tag does not already exist.
# ---------------------------------------------------------------------

step "Verifying tag v$VERSION is new"
if git rev-parse "v$VERSION" >/dev/null 2>&1; then
  fail "Tag v$VERSION already exists. Bump version or delete the tag first."
fi
echo "OK: tag v$VERSION is available."

# ---------------------------------------------------------------------
# 4. Full build.
# ---------------------------------------------------------------------

step "Building workspace (release profile)"
cargo build --workspace --release
echo "OK: release build succeeded."

# ---------------------------------------------------------------------
# 5. Full test suite.
# ---------------------------------------------------------------------

step "Running test suite"
cargo test --workspace --all-targets
echo "OK: all tests pass."

# ---------------------------------------------------------------------
# 6. Clippy (zero warnings).
# ---------------------------------------------------------------------

step "Running clippy (zero-warning strictness)"
cargo clippy --workspace --all-targets -- -D warnings
echo "OK: clippy is clean."

# ---------------------------------------------------------------------
# 7. Formatting.
# ---------------------------------------------------------------------

step "Verifying formatting"
cargo fmt --all --check
echo "OK: formatting is consistent."

# ---------------------------------------------------------------------
# 8. Binary size check.
# ---------------------------------------------------------------------

step "Verifying binary sizes (<10MB)"
sh scripts/check-binary-size.sh target/release
echo "OK: binaries are under the size limit."

# ---------------------------------------------------------------------
# 9. cargo publish --dry-run verification.
# ---------------------------------------------------------------------
#
# Publishing a workspace with interdependent crates for the first time
# is a known rough edge in Cargo. The problem: `cargo publish -p logdive`
# strips the `path = ...` portion of the `logdive-core` workspace
# dependency and tries to resolve `logdive-core = "0.1.0"` against
# crates.io. Since we haven't published core yet, that lookup fails.
#
# Cargo 1.90+ supports `cargo publish --workspace --dry-run`, which
# uses an internal local-registry overlay to resolve the unpublished
# transitive dependencies. We detect the Cargo version and use it
# when available.
#
# On older Cargo (pre-1.90), we fall back to verifying only
# `logdive-core`. The cli and api crates will be validated at real
# publish time, after core is live on crates.io.

# Extract Cargo's major.minor version number. `cargo --version` prints
# a line like: "cargo 1.86.0 (824adfb12 2025-03-22)". We parse the
# first two numeric components.
CARGO_VERSION_LINE=$(cargo --version)
CARGO_MINOR=$(echo "$CARGO_VERSION_LINE" \
  | sed -E 's/^cargo ([0-9]+)\.([0-9]+)\..*$/\2/')
CARGO_MAJOR=$(echo "$CARGO_VERSION_LINE" \
  | sed -E 's/^cargo ([0-9]+)\.([0-9]+)\..*$/\1/')

# Sanity-check the parse. If either part is non-numeric, treat as old.
case "$CARGO_MAJOR" in
  *[!0-9]*|'') CARGO_MAJOR=0 ;;
esac
case "$CARGO_MINOR" in
  *[!0-9]*|'') CARGO_MINOR=0 ;;
esac

# `cargo publish --workspace` on stable requires Cargo >= 1.90.
USE_WORKSPACE_DRY_RUN=0
if [ "$CARGO_MAJOR" -gt 1 ]; then
  USE_WORKSPACE_DRY_RUN=1
elif [ "$CARGO_MAJOR" -eq 1 ] && [ "$CARGO_MINOR" -ge 90 ]; then
  USE_WORKSPACE_DRY_RUN=1
fi

if [ "$USE_WORKSPACE_DRY_RUN" -eq 1 ]; then
  step "Dry-run: cargo publish --workspace (cargo ${CARGO_MAJOR}.${CARGO_MINOR})"
  cargo publish --dry-run --workspace --allow-dirty
  echo "OK: all three crates package and verify cleanly as a workspace."
else
  step "Dry-run: logdive-core (cargo ${CARGO_MAJOR}.${CARGO_MINOR} — partial verification)"
  echo "note: cargo 1.90+ enables verifying all three crates together."
  echo "      your toolchain is older; only logdive-core is fully verified."
  echo "      cli and api will be validated at real publish time instead."
  cargo publish --dry-run -p logdive-core --allow-dirty
  echo "OK: logdive-core packages cleanly."
fi

# ---------------------------------------------------------------------
# 10. CHANGELOG has a non-TBD date for this version.
# ---------------------------------------------------------------------

step "Verifying CHANGELOG date"
if grep -qE "^## \[$VERSION\] - [0-9]{4}-[0-9]{2}-[0-9]{2}" CHANGELOG.md; then
  echo "OK: CHANGELOG.md has a dated entry for $VERSION."
else
  fail "CHANGELOG.md does not have a dated entry for $VERSION. Update the date from 'TBD' to today's date in YYYY-MM-DD format."
fi

# ---------------------------------------------------------------------
# Summary.
# ---------------------------------------------------------------------

echo ""
echo "=============================================================="
echo "All pre-release checks passed."
echo ""
echo "Next steps:"
echo "  1. git tag -a v$VERSION -m 'v$VERSION'"
echo "  2. git push origin main v$VERSION"
echo "  3. Wait for the release workflow to build and publish binaries."
echo "  4. Publish to crates.io in dependency order:"
echo "       cargo publish -p logdive-core"
echo "       cargo publish -p logdive"
echo "       cargo publish -p logdive-api"
echo "     (allow ~30 seconds between each publish for the index to propagate)"
echo "=============================================================="
