#!/usr/bin/env bash
# Publish all phonon-* crates to crates.io in dependency order.
#
# Prerequisites:
#   * `cargo login` has been run with a valid CARGO_REGISTRY_TOKEN.
#   * The working tree is clean and tagged (e.g. `git tag v0.7.0`).
#   * `cargo test --workspace` is green.
#
# Usage:
#   ./scripts/publish-crates.sh           # dry-run all crates
#   ./scripts/publish-crates.sh --execute # actually publish
#
# Each `cargo publish` is followed by a short sleep so the new version
# becomes resolvable for the next crate that depends on it.

set -euo pipefail

cd "$(dirname "$0")/.."

EXECUTE=0
if [[ "${1:-}" == "--execute" ]]; then
  EXECUTE=1
fi

# Crates listed in dependency order. phonon-tests is `publish = false`
# and phonon-studio is currently marked `publish = false` (heavy GUI deps).
CRATES=(
  phonon-core
  phonon-wav
  phonon-io
  phonon-engine
  phonon-ai
  phonon-podcast
  phonon-video
  phonon-mcp
  phonon-server
  phonon-cli
  phonon-wasm
)

run_publish() {
  local crate="$1"
  local cmd=(cargo publish -p "$crate")
  if [[ $EXECUTE -eq 0 ]]; then
    cmd+=(--dry-run)
  fi

  echo
  echo "== ${cmd[*]} =="
  "${cmd[@]}"

  if [[ $EXECUTE -eq 1 ]]; then
    echo "  → waiting 30s for crates.io to index $crate"
    sleep 30
  fi
}

for c in "${CRATES[@]}"; do
  run_publish "$c"
done

echo
if [[ $EXECUTE -eq 1 ]]; then
  echo "All crates published."
else
  echo "Dry-run complete. Re-run with --execute to publish."
fi
