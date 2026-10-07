#!/usr/bin/env bash
# Replaces /Applications/Wiffletree.app with a published release (the latest by default).
# Downloads through `gh`, so it also works while the repository is private.
set -euo pipefail

tag="${1:-}"
app="/Applications/Wiffletree.app"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

gh release download ${tag:+"$tag"} --repo Vyttle-LLC/wiffletree \
    --pattern 'Wiffletree-*-macos-arm64.zip' --dir "$work"
ditto -x -k "$work"/Wiffletree-*.zip "$work"
codesign --verify --deep --strict "$work/Wiffletree.app"

# Quit only a running copy; asking a closed app to quit would launch it.
if pgrep -f "$app/Contents/MacOS/" >/dev/null; then
    osascript -e 'tell application id "com.vyttle.wiffletree" to quit'
    while pgrep -f "$app/Contents/MacOS/" >/dev/null; do sleep 0.2; done
fi
rm -rf "$app"
mv "$work/Wiffletree.app" "$app"
open "$app"
printf 'Installed %s\n' "$(defaults read "$app/Contents/Info.plist" CFBundleShortVersionString)"
