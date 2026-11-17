#!/usr/bin/env bash
# Build winmac for macOS (release) and wrap it in a menu bar app: target/release/WinMac.app
set -euo pipefail
cd "$(dirname "$0")"

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo not found. Install Rust: https://rustup.rs" >&2
  exit 1
fi

cargo build --release

BIN="$(pwd)/target/release/winmac"
APP="$(pwd)/target/release/WinMac.app"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
cp "$BIN" "$APP/Contents/MacOS/winmac"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>WinMac</string>
  <key>CFBundleDisplayName</key><string>WinMac</string>
  <key>CFBundleIdentifier</key><string>local.winmac</string>
  <key>CFBundleExecutable</key><string>winmac</string>
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
cp -f "$BIN" "$ROOT/winmac"
rm -rf "$ROOT/WinMac.app"
cp -R "$APP" "$ROOT/WinMac.app"

echo
echo "Done (copied to the repository root):"
echo "  Menu bar app:  $ROOT/WinMac.app   (open it, or copy it to /Applications)"
echo "  CLI binary:    $ROOT/winmac"
echo
echo "Run CLI client:  ./winmac client <windows-ip>"
echo "Run CLI server:  ./winmac server --remote-side left"
echo "Remember: allow Accessibility (and Input Monitoring for server mode) in"
echo "System Settings > Privacy & Security for WinMac (or your Terminal when using the CLI)."
