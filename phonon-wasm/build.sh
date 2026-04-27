#!/usr/bin/env bash
# Build the phonon-wasm package and refresh the demo's pkg/ directory.
#
# Usage:
#   ./build.sh            # release build
#   ./build.sh --dev      # debug build (faster, larger)
#
# Output: phonon-wasm/www/pkg/{phonon_wasm.js,phonon_wasm_bg.wasm,...}

set -euo pipefail

cd "$(dirname "$0")"

PROFILE_FLAG="--release"
if [[ "${1:-}" == "--dev" ]]; then
  PROFILE_FLAG="--dev"
fi

if ! command -v wasm-pack >/dev/null 2>&1; then
  echo "wasm-pack not found. Install with:" >&2
  echo "  cargo install wasm-pack" >&2
  exit 1
fi

echo "Building phonon-wasm ($PROFILE_FLAG, target=web)…"
wasm-pack build --target web --out-dir www/pkg $PROFILE_FLAG

echo
echo "Done. Serve the demo with:"
echo "  python3 -m http.server --directory www 8080"
echo "Then open http://localhost:8080/"
