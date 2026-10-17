#!/usr/bin/env bash
# Package kore.app (from bundle.sh) into a drag-to-install DMG.
#
#   crates/kore-app/packaging/macos/dmg.sh <path/to/kore.app> <out.dmg>
#
# The DMG holds kore.app and an /Applications shortcut; install.sh at the repo root
# mounts it and copies the first *.app it finds. Optional, for releases:
#   SIGN_IDENTITY="Developer ID Application: …"   sign the DMG itself
#   APPLE_ID / APPLE_PASSWORD / APPLE_TEAM_ID       notarize (app-specific password) and staple
set -euo pipefail

APP="${1:?usage: dmg.sh <kore.app> <out.dmg>}"
OUT="${2:?usage: dmg.sh <kore.app> <out.dmg>}"
[[ -d "$APP" ]] || { echo "missing $APP" >&2; exit 1; }

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"

mkdir -p "$(dirname "$OUT")"
rm -f "$OUT"
hdiutil create -volname kore -srcfolder "$STAGE" -fs HFS+ -format UDZO -ov "$OUT" >/dev/null

if [[ -n "${SIGN_IDENTITY:-}" ]]; then
  codesign --force --sign "$SIGN_IDENTITY" --timestamp "$OUT"
fi

if [[ -n "${APPLE_ID:-}" && -n "${APPLE_PASSWORD:-}" && -n "${APPLE_TEAM_ID:-}" ]]; then
  # Notarizing the DMG covers the app inside it; stapling lets Gatekeeper verify offline.
  xcrun notarytool submit "$OUT" --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" \
    --team-id "$APPLE_TEAM_ID" --wait --timeout 30m
  xcrun stapler staple "$OUT"
fi

hdiutil verify "$OUT" >/dev/null
echo "$OUT"
