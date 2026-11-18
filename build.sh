#!/usr/bin/env bash
# Build desktali for macOS (release) and wrap it in a menu bar app: target/release/desktali.app
set -euo pipefail
cd "$(dirname "$0")"

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo not found. Install Rust: https://rustup.rs" >&2
  exit 1
fi

cargo build --release

BIN="$(pwd)/target/release/desktali"
APP="$(pwd)/target/release/desktali.app"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
cp "$BIN" "$APP/Contents/MacOS/desktali"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>desktali</string>
  <key>CFBundleDisplayName</key><string>desktali</string>
  <key>CFBundleIdentifier</key><string>local.desktali</string>
  <key>CFBundleExecutable</key><string>desktali</string>
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
cp -f "$BIN" "$ROOT/desktali"
rm -rf "$ROOT/desktali.app"
cp -R "$APP" "$ROOT/desktali.app"

echo
echo "Done (copied to the repository root):"
echo "  Menu bar app:  $ROOT/desktali.app   (open it, or copy it to /Applications)"
echo "  CLI binary:    $ROOT/desktali"
echo
echo "Run CLI client:  ./desktali client <windows-ip>"
echo "Run CLI server:  ./desktali server --remote-side left"
echo "Remember: allow Accessibility (and Input Monitoring for server mode) in"
echo "System Settings > Privacy & Security for desktali (or your Terminal when using the CLI)."
