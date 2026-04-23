#!/usr/bin/env sh
# Verify release binaries are under the 10MB target.
#
# Usage: scripts/check-binary-size.sh [RELEASE_DIR]
#
# RELEASE_DIR defaults to `target/release`. For cross-compiled builds, pass
# the target-specific path, e.g. `target/aarch64-apple-darwin/release`.
#
# Exits 0 if both binaries are under the limit, 1 otherwise. Prints a
# human-readable table regardless of outcome so the output is useful
# in CI logs and during local development.

set -eu

RELEASE_DIR="${1:-target/release}"

# Maximum allowed size per binary, in bytes. 10 MB base-10 matches the
# project doc's Phase 4 target ("under 10MB") and what users see in `ls -h`.
MAX_BYTES=10000000

if [ ! -d "$RELEASE_DIR" ]; then
  echo "error: release directory not found: $RELEASE_DIR" >&2
  echo "hint: run \`cargo build --release\` first" >&2
  exit 1
fi

# BINARIES is whitespace-separated, not newline-separated, so it's
# portable across POSIX sh variants (dash, busybox, bash).
BINARIES="logdive logdive-api"

# `stat` has incompatible flags across Linux (GNU) and macOS (BSD).
# `wc -c < file` is POSIX and works everywhere.
file_size() {
  wc -c < "$1" | tr -d ' '
}

# Format a byte count as a human-readable string with one decimal.
# Pure POSIX arithmetic — no awk/bc dependency beyond what `sh` guarantees.
human_size() {
  bytes=$1
  if [ "$bytes" -ge 1000000 ]; then
    whole=$((bytes / 1000000))
    frac=$(( (bytes % 1000000) / 100000 ))
    echo "${whole}.${frac} MB"
  elif [ "$bytes" -ge 1000 ]; then
    whole=$((bytes / 1000))
    frac=$(( (bytes % 1000) / 100 ))
    echo "${whole}.${frac} KB"
  else
    echo "${bytes} B"
  fi
}

printf '%-20s %-15s %-15s %s\n' "binary" "size" "limit" "status"
printf '%-20s %-15s %-15s %s\n' "------" "----" "-----" "------"

status=0
for bin in $BINARIES; do
  path="$RELEASE_DIR/$bin"
  if [ ! -f "$path" ]; then
    printf '%-20s %-15s %-15s %s\n' "$bin" "-" "$(human_size $MAX_BYTES)" "MISSING"
    status=1
    continue
  fi
  size=$(file_size "$path")
  human=$(human_size "$size")
  limit=$(human_size "$MAX_BYTES")
  if [ "$size" -le "$MAX_BYTES" ]; then
    printf '%-20s %-15s %-15s %s\n' "$bin" "$human" "$limit" "OK"
  else
    printf '%-20s %-15s %-15s %s\n' "$bin" "$human" "$limit" "OVER LIMIT"
    status=1
  fi
done

if [ "$status" -ne 0 ]; then
  echo ""
  echo "error: one or more binaries exceeded the 10MB size limit." >&2
  echo "hint: review recent dependency additions; consider \`cargo bloat\` to identify large contributors." >&2
  exit 1
fi
