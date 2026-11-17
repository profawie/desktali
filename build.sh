#!/usr/bin/env bash
# Build rtetherfy for macOS (release) and wrap it in a menu bar app: target/release/rtetherfy.app
set -euo pipefail
cd "$(dirname "$0")"

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo not found. Install Rust: https://rustup.rs" >&2
  exit 1
fi

cargo build --release

BIN="$(pwd)/target/release/rtetherfy"
APP="$(pwd)/target/release/rtetherfy.app"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
cp "$BIN" "$APP/Contents/MacOS/rtetherfy"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>rtetherfy</string>
  <key>CFBundleDisplayName</key><string>rtetherfy</string>
  <key>CFBundleIdentifier</key><string>local.rtetherfy</string>
  <key>CFBundleExecutable</key><string>rtetherfy</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key><string>10.13</string>
  <key>LSUIElement</key><true/>
</dict>
</plist>
PLIST
# Ad-hoc signature so macOS can remember the Accessibility permission.
codesign --force --deep -s - "$APP" >/dev/null 2>&1 || true

# Copy the results to the repository root for easy access.
ROOT="$(pwd)"
cp -f "$BIN" "$ROOT/rtetherfy"
rm -rf "$ROOT/rtetherfy.app"
cp -R "$APP" "$ROOT/rtetherfy.app"

echo
echo "Done (copied to the repository root):"
echo "  Menu bar app:  $ROOT/rtetherfy.app   (open it, or copy it to /Applications)"
echo "  CLI binary:    $ROOT/rtetherfy"
echo
echo "Run CLI client:  ./rtetherfy client <windows-ip>"
echo "Run CLI server:  ./rtetherfy server --remote-side left"
echo "Remember: allow Accessibility (and Input Monitoring for server mode) in"
echo "System Settings > Privacy & Security for rtetherfy (or your Terminal when using the CLI)."
