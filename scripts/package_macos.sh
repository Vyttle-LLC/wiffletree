#!/usr/bin/env bash
set -euo pipefail

# Release builds set WIFFLETREE_VERSION, which also enables self-update in the app.
# CODESIGN_IDENTITY selects a Developer ID; the default ad-hoc signature is for local use.
# CODESIGN_KEYCHAIN limits the identity lookup to one keychain.
# --beta builds "Wiffletree Beta": its own bundle ID and data folder, so it runs beside the
# release app without touching the store or sessions that app is using.
name="Wiffletree"
bundle_id="com.vyttle.wiffletree"
if [ "${1:-}" = "--beta" ]; then
    name="Wiffletree Beta"
    bundle_id="com.vyttle.wiffletree.beta"
    export WIFFLETREE_CHANNEL=beta
elif [ -n "${1:-}" ]; then
    echo "Usage: $0 [--beta]" >&2
    exit 2
fi

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"
version="${WIFFLETREE_VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}"
identity="${CODESIGN_IDENTITY:--}"
cargo build --locked --release -p workspace-desktop -p workspace-host
app_bundle="$repo_root/target/$name.app"
# Start clean so files from an earlier build, such as a stapled ticket, never get re-signed.
rm -rf "$app_bundle"
mkdir -p "$app_bundle/Contents/MacOS" "$app_bundle/Contents/Resources"
cp target/release/workspace-host "$app_bundle/Contents/MacOS/workspace-host"
cp target/release/workspace-desktop "$app_bundle/Contents/MacOS/wiffletree"

# Build every standard and Retina representation from the canonical brand exports.
iconset="$repo_root/target/Wiffletree.iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
    for scale in 1 2; do
        pixels=$((size * scale))
        suffix=""
        if [ "$scale" -eq 2 ]; then suffix="@2x"; fi
        source_icon="$repo_root/design/brand/png/icon-deep-teal-1024.png"
        if [ "$pixels" -le 32 ]; then
            source_icon="$repo_root/design/brand/png/favicon-$pixels.png"
        fi
        sips -z "$pixels" "$pixels" "$source_icon" \
            --out "$iconset/icon_${size}x${size}${suffix}.png" >/dev/null
    done
done
iconutil -c icns "$iconset" -o "$app_bundle/Contents/Resources/Wiffletree.icns"

cat > "$app_bundle/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>$name</string>
<key>CFBundleDisplayName</key><string>$name</string>
<key>CFBundleIdentifier</key><string>$bundle_id</string>
<key>CFBundleExecutable</key><string>wiffletree</string>
<key>CFBundleIconFile</key><string>Wiffletree.icns</string>
<key>CFBundleVersion</key><string>$version</string>
<key>CFBundleShortVersionString</key><string>$version</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSHumanReadableCopyright</key><string>© 2026 Vyttle LLC · MIT License</string>
<key>LSMinimumSystemVersion</key><string>13.0</string>
</dict></plist>
PLIST
if [ "$identity" = "-" ]; then
    codesign --force --sign - "$app_bundle"
else
    # Notarization requires the hardened runtime and a timestamp on every executable.
    for target in "$app_bundle/Contents/MacOS/workspace-host" "$app_bundle"; do
        codesign --force --options runtime --timestamp --sign "$identity" \
            ${CODESIGN_KEYCHAIN:+--keychain "$CODESIGN_KEYCHAIN"} "$target"
    done
fi
printf 'Development bundle: %s\n' "$app_bundle"
