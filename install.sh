#!/bin/bash
# Downloads the latest GolfSim Connect release and installs it to /Applications.
set -euo pipefail

REPO="${OPENLAUNCH_REPO:-chgl42/openlaunch-connect}"
APP="GolfSim Connect.app"
ZIP="GolfSim-Connect-macos-arm64.zip"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "▸ Downloading the latest release of $REPO …"
curl -fL --progress-bar -o "$TMP/$ZIP" "https://github.com/$REPO/releases/latest/download/$ZIP"

echo "▸ Installing to /Applications …"
ditto -x -k "$TMP/$ZIP" "$TMP"
# Quit a running copy first (pkill -x can't match: macOS shortens process names to 16 characters).
for name in "GolfSim Connect" "OpenLaunch Connect" "OpenR10 Connect"; do
  osascript -e "if application \"$name\" is running then tell application \"$name\" to quit" 2>/dev/null || true
done
sleep 1
pkill -f "Connect.app/Contents/MacOS/open(launch|r10)-connect" 2>/dev/null || true
rm -rf "/Applications/$APP" "/Applications/OpenLaunch Connect.app" "/Applications/OpenR10 Connect.app"
cp -R "$TMP/$APP" /Applications/
# The app is ad-hoc signed (not notarized); remove the download quarantine so Gatekeeper lets it start.
xattr -dr com.apple.quarantine "/Applications/$APP" 2>/dev/null || true

echo "✓ Installed: /Applications/$APP"
open "/Applications/$APP"
