#!/bin/sh
# Build, sign and install Spide into /Applications (or $SPIDE_INSTALL_DIR), replacing the old copy.
# Quit Spide before running this.
set -eu
cd "$(dirname "$0")/.."
./scripts/bundle-macos.sh
dest="${SPIDE_INSTALL_DIR:-/Applications}/Spide.app"
if pgrep -f "$dest/Contents/MacOS/spide" >/dev/null 2>&1; then
    echo "Spide is running; quit it first." >&2
    exit 1
fi
rm -rf "$dest"
ditto target/release/bundle/macos/Spide.app "$dest"
codesign --verify --strict "$dest"
printf 'Installed %s\n' "$dest"
codesign -d -r- "$dest" 2>&1 | sed -n 's/^designated => /Designated requirement: /p'
