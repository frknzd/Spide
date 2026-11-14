#!/bin/sh
# Build an unsigned, self-contained native app bundle (no web assets).
set -eu
cd "$(dirname "$0")/.."
[ "$(uname -s)" = Darwin ] || { echo "macOS is required" >&2; exit 1; }
cargo build --release -p spide
bundle=target/release/bundle/macos/Spide.app
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp target/release/spide "$bundle/Contents/MacOS/spide"
cp app/icons/icon.icns "$bundle/Contents/Resources/icon.icns"
cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>spide</string>
<key>CFBundleIdentifier</key><string>dev.spide.desktop</string>
<key>CFBundleName</key><string>Spide</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>CFBundleVersion</key><string>1</string>
<key>CFBundleIconFile</key><string>icon.icns</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
printf 'Unsigned app: %s\n' "$bundle"
