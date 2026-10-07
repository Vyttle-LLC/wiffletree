#!/usr/bin/env bash
# Installs or replaces Wiffletree.app with a published release, the latest by default:
#
#   curl -fsSL https://raw.githubusercontent.com/Vyttle-LLC/wiffletree/main/scripts/install.sh | bash
#   curl -fsSL .../install.sh | bash -s -- v0.2.0
#
# WIFFLETREE_INSTALL_DIR picks the destination folder (default /Applications).
set -euo pipefail

repo="Vyttle-LLC/wiffletree"
install_dir="${WIFFLETREE_INSTALL_DIR:-/Applications}"
app="$install_dir/Wiffletree.app"

fail() { printf 'wiffletree: %s\n' "$1" >&2; exit 1; }

[ "$(uname -s)" = Darwin ] || fail "Wiffletree runs on macOS only."
[ "$(uname -m)" = arm64 ] || fail "Wiffletree needs an Apple Silicon Mac."
[ -w "$install_dir" ] || fail "Cannot write to $install_dir. Set WIFFLETREE_INSTALL_DIR=~/Applications or rerun with sudo."

# Without a tag, follow the releases/latest redirect to the newest tag.
tag="${1:-$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$repo/releases/latest")}"
tag="${tag##*/}"
[[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "No release found (got '$tag')."

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
archive="Wiffletree-${tag#v}-macos-arm64.zip"
printf 'Downloading Wiffletree %s…\n' "${tag#v}"
curl -fL --progress-bar -o "$work/$archive" "https://github.com/$repo/releases/download/$tag/$archive" \
    || fail "No macOS build for $tag yet. A new release takes a few minutes to publish; try again shortly."
ditto -x -k "$work/$archive" "$work"
# Accept only an intact, Developer ID–signed and notarized bundle.
codesign --verify --deep --strict "$work/Wiffletree.app"
spctl --assess --type execute "$work/Wiffletree.app" || fail "The download did not pass Gatekeeper."

# Quit only a running copy; asking a closed app to quit would launch it.
if pgrep -f "$app/Contents/MacOS/" >/dev/null; then
    printf 'Quitting the running Wiffletree…\n'
    osascript -e 'tell application id "com.vyttle.wiffletree" to quit'
    while pgrep -f "$app/Contents/MacOS/" >/dev/null; do sleep 0.2; done
fi
rm -rf "$app"
mv "$work/Wiffletree.app" "$app"
printf 'Installed Wiffletree %s in %s\n' "${tag#v}" "$install_dir"
open "$app"
