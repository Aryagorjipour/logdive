#!/bin/sh
# Generate a CycloneDX SBOM for the logdive workspace.
#
# Requires: cargo install cargo-cyclonedx --locked
# Output:   logdive-sbom.json (or the path given as $1)
#
# Usage:
#   ./scripts/sbom.sh                   # writes logdive-sbom.json
#   ./scripts/sbom.sh /tmp/sbom.json    # writes to custom path
set -eu

OUT="${1:-logdive-sbom.json}"

if ! command -v cargo-cyclonedx >/dev/null 2>&1; then
    echo "Installing cargo-cyclonedx..."
    cargo install cargo-cyclonedx --locked --quiet
fi

cargo cyclonedx --format json --output-file "$OUT"
echo "SBOM written to $OUT"
