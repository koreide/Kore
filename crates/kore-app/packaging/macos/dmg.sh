#!/usr/bin/env bash
# Package kore.app (from bundle.sh) into a drag-to-install DMG.
#
#   crates/kore-app/packaging/macos/dmg.sh <path/to/kore.app> <out.dmg>
#
# The DMG holds kore.app and an /Applications shortcut; install.sh at the repo root
# mounts it and copies the first *.app it finds. The DMG is not signed or notarized.
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

hdiutil verify "$OUT" >/dev/null
echo "$OUT"
