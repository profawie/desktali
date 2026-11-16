#!/usr/bin/env bash
# Build winmac untuk macOS (release).
set -euo pipefail
cd "$(dirname "$0")"

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo tidak dijumpai. Pasang Rust: https://rustup.rs" >&2
  exit 1
fi

cargo build --release

BIN="$(pwd)/target/release/winmac"
echo
echo "Siap: $BIN"
echo "Jalankan client:  $BIN client <IP-Windows>"
echo "Jalankan server:  $BIN server --remote-side left"
echo "Ingat: beri kebenaran Accessibility kepada Terminal (System Settings > Privacy & Security)."
