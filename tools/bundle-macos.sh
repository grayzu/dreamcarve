#!/usr/bin/env bash
#
# Copyright 2026 The DreamCarve Contributors
# SPDX-License-Identifier: Apache-2.0
#
# Builds the app in release mode and wraps it as `DreamCarve.app`, for
# trying the alpha on a Mac. Run it from anywhere inside the repository:
#
#   tools/bundle-macos.sh              # writes target/bundle/DreamCarve.app
#   open target/bundle/DreamCarve.app
#
# This is a local convenience, not the release bundle of phase 4's release
# slice (docs/specs/phase-4/design.md, slice 22). What it leaves out:
#   - It is not signed or notarised. An app built on the machine that runs
#     it is not quarantined, so it opens; a copy sent to another Mac will be
#     refused by Gatekeeper.
#   - It has no icon.
#   - It declares the `.dcv` document type, so Finder offers DreamCarve for
#     a project, but macOS hands a double-clicked file to a running app as
#     an Apple Event, which the app does not read yet: the app opens, the
#     project does not. Use File > Open, or drop the file on the window.
#   - It builds for this Mac's architecture only.

set -euo pipefail

cd "$(dirname "$0")/.."

if [ "$(uname -s)" != "Darwin" ]; then
    echo "error: this script builds a macOS app bundle; run it on a Mac" >&2
    exit 1
fi

version="$(cargo metadata --no-deps --format-version 1 |
    sed -n 's/.*"name":"dreamcarve-app","version":"\([^"]*\)".*/\1/p' | head -n 1)"
if [ -z "$version" ]; then
    echo "error: could not read dreamcarve-app's version from cargo metadata" >&2
    exit 1
fi

cargo build --release --locked -p dreamcarve-app

app="target/bundle/DreamCarve.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/dreamcarve-app "$app/Contents/MacOS/dreamcarve-app"
cp NOTICE LICENSE "$app/Contents/Resources/"

cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>DreamCarve</string>
    <key>CFBundleDisplayName</key>
    <string>DreamCarve</string>
    <key>CFBundleIdentifier</key>
    <string>org.dreamcarve.app</string>
    <key>CFBundleExecutable</key>
    <string>dreamcarve-app</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>${version}</string>
    <key>CFBundleVersion</key>
    <string>${version}</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeName</key>
            <string>DreamCarve Project</string>
            <key>CFBundleTypeRole</key>
            <string>Editor</string>
            <key>CFBundleTypeExtensions</key>
            <array>
                <string>dcv</string>
            </array>
        </dict>
    </array>
</dict>
</plist>
EOF

echo "Wrote $app (version $version)"
