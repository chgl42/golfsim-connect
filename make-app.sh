#!/bin/bash
# Builds "GolfSim Connect.app". With --install it is also copied to /Applications;
# with --deploy <ssh-host> it replaces and starts the app on another Mac (e.g. the one running GSPro).
set -euo pipefail
cd "$(dirname "$0")"

APP_NAME="GolfSim Connect"
BIN="openlaunch-connect"
APP="target/${APP_NAME}.app"

echo "▸ Building $BIN (release) …"
cargo build --release --bin "$BIN"

echo "▸ Creating app bundle …"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "target/release/$BIN" "$APP/Contents/MacOS/$BIN"
cp packaging/Info.plist "$APP/Contents/Info.plist"
cp LICENSE-MIT LICENSE-APACHE NOTICE "$APP/Contents/Resources/"

if command -v cargo-about >/dev/null 2>&1; then
  echo "▸ Generating third-party license report …"
  cargo about generate about.hbs -o target/THIRD_PARTY_LICENSES.html
  cp target/THIRD_PARTY_LICENSES.html "$APP/Contents/Resources/"
else
  echo "  (cargo-about not installed – skipping license report)"
fi

echo "▸ Creating icon …"
ICONSET="target/AppIcon.iconset"
rm -rf "$ICONSET"
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  sips -z "$s" "$s" packaging/icon.png --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2))
  sips -z "$d" "$d" packaging/icon.png --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"

echo "▸ Signing (ad hoc) …"
codesign --force --deep --sign - "$APP"

if [[ "${1:-}" == "--install" ]]; then
  echo "▸ Installing to /Applications …"
  # Quit a running copy, otherwise `open` below only brings the old one to the front; also one of the
  # earlier name, so it doesn't write its data folder while that moves.
  for name in "${APP_NAME}" "OpenLaunch Connect" "OpenR10 Connect"; do
    osascript -e "if application \"${name}\" is running then tell application \"${name}\" to quit" 2>/dev/null || true
  done
  sleep 1
  rm -rf "/Applications/${APP_NAME}.app"
  # Remove builds installed under earlier names
  for old in "OpenLaunch Connect" "OpenR10 Connect" "R10 Bridge" "GSProR10Connect"; do
    if [[ -d "/Applications/${old}.app" ]]; then
      echo "  (removing old »${old}.app«)"
      rm -rf "/Applications/${old}.app"
    fi
  done
  cp -R "$APP" "/Applications/"
  echo "✓ Done: /Applications/${APP_NAME}.app"
  open "/Applications/${APP_NAME}.app"
elif [[ "${1:-}" == "--deploy" ]]; then
  HOST="${2:?usage: ./make-app.sh --deploy <ssh-host>}"
  echo "▸ Deploying to ${HOST}:/Applications …"
  # Quit a running copy first, also one of an earlier name, and remove those bundles (same as --install), in
  # one ssh; tar keeps the bundle and its ad-hoc signature intact and, unlike a download, adds no quarantine flag.
  QUIT=""
  for name in "${APP_NAME}" "OpenLaunch Connect" "OpenR10 Connect"; do
    QUIT+="osascript -e 'if application \"${name}\" is running then tell application \"${name}\" to quit' >/dev/null 2>&1; "
  done
  ssh "$HOST" "${QUIT}sleep 1; pkill -f 'Connect.app/Contents/MacOS/open(launch|r10)-connect' || true; rm -rf '/Applications/OpenLaunch Connect.app' '/Applications/OpenR10 Connect.app'"
  tar -C target -cf - "${APP_NAME}.app" \
    | ssh "$HOST" "rm -rf '/Applications/${APP_NAME}.app' && tar -C /Applications -xf - && open '/Applications/${APP_NAME}.app'"
  echo "✓ Done: ${HOST}:/Applications/${APP_NAME}.app (started)"
else
  echo "✓ Done: $APP"
  echo "  Install with:  ./make-app.sh --install   (or on another Mac: ./make-app.sh --deploy <ssh-host>)"
fi
