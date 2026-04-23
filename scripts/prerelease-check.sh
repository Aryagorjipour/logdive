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
# 9. cargo publish --dry-run for each crate.
# ---------------------------------------------------------------------
#
# Ordering + verification notes:
#
#   logdive-core publishes first (it has no path dependencies) and is
#   fully verified — we let cargo rebuild the crate from the produced
#   .crate tarball to confirm it compiles in isolation.
#
#   logdive and logdive-api both depend on logdive-core. For these two
#   we use --no-verify during the dry-run, because the full verify step
#   would strip the `path` component from the workspace dependency and
#   then try to resolve `logdive-core = "X.Y.Z"` against the real
#   crates.io index. Before the first publish, that lookup fails with
#   "no matching package named `logdive-core` found" — which is a known
#   Cargo limitation when publishing multiple interdependent crates
#   from a workspace for the first time.
#
#   --no-verify still validates:
#     - Manifest is valid.
#     - Required fields are present (description, license, etc.).
#     - Files are correctly packaged.
#     - No accidental inclusion of large/bogus files.
#
#   It skips only the rebuild-from-tarball step. That step runs for
#   real anyway when `cargo publish -p logdive` is executed (after
#   logdive-core is live on crates.io), so we don't lose coverage —
#   we just move it to publish time.

step "Dry-run: logdive-core (full verification)"
cargo publish --dry-run -p logdive-core --allow-dirty
echo "OK: logdive-core packages cleanly."

step "Dry-run: logdive (packaging only; verify runs at real publish)"
cargo publish --dry-run -p logdive --allow-dirty --no-verify
echo "OK: logdive packages cleanly."

step "Dry-run: logdive-api (packaging only; verify runs at real publish)"
cargo publish --dry-run -p logdive-api --allow-dirty --no-verify
echo "OK: logdive-api packages cleanly."

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
