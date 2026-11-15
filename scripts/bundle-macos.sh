#!/bin/sh
# Build a self-contained native app bundle (no web assets), signed with a stable local identity.
# Run scripts/setup-signing-macos.sh once first; without it the bundle is ad-hoc signed and macOS
# asks for privacy permissions again after every build.
set -eu
cd "$(dirname "$0")/.."
[ "$(uname -s)" = Darwin ] || { echo "macOS is required" >&2; exit 1; }
cargo build --release -p spide
version=$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -1)
build=$(date +%Y%m%d%H%M%S)
bundle=target/release/bundle/macos/Spide.app
rm -rf "$bundle"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp target/release/spide "$bundle/Contents/MacOS/spide"
cp app/icons/icon.icns "$bundle/Contents/Resources/icon.icns"
cat > "$bundle/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>spide</string>
<key>CFBundleIdentifier</key><string>dev.spide.desktop</string>
<key>CFBundleName</key><string>Spide</string>
<key>CFBundleDisplayName</key><string>Spide</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>$version</string>
<key>CFBundleVersion</key><string>$build</string>
<key>CFBundleIconFile</key><string>icon.icns</string>
<key>LSMinimumSystemVersion</key><string>11.0</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSDocumentsFolderUsageDescription</key><string>Spide and its pi agent open and save design projects in your Documents folder.</string>
<key>NSDesktopFolderUsageDescription</key><string>Spide and its pi agent open and save projects on your Desktop.</string>
<key>NSDownloadsFolderUsageDescription</key><string>Spide and its pi agent open and save projects in your Downloads folder.</string>
<key>NSNetworkVolumesUsageDescription</key><string>Spide and its pi agent open projects on network volumes.</string>
<key>NSRemovableVolumesUsageDescription</key><string>Spide and its pi agent open projects on removable volumes.</string>
<key>NSFileProviderDomainUsageDescription</key><string>Spide and its pi agent open projects in cloud storage folders such as OneDrive or iCloud Drive.</string>
<key>NSAppleEventsUsageDescription</key><string>Commands run by the pi agent may control other apps.</string>
</dict></plist>
PLIST
identity="${SPIDE_SIGN_IDENTITY:-Spide Local Signing}"
if security find-certificate -c "$identity" >/dev/null 2>&1; then
    codesign --force --timestamp=none --sign "$identity" "$bundle"
    printf 'Signed with "%s": %s\n' "$identity" "$bundle"
else
    codesign --force --sign - "$bundle"
    printf 'Ad-hoc signed (run scripts/setup-signing-macos.sh for stable permissions): %s\n' "$bundle"
fi
