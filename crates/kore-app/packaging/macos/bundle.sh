#!/usr/bin/env bash
# Assemble the macOS app bundle for the native Kore app.
#
#   crates/kore-app/packaging/macos/bundle.sh            # builds release, writes target/release/bundle/macos/kore.app
#   SKIP_BUILD=1 crates/kore-app/packaging/macos/bundle.sh
#   TARGET=x86_64-apple-darwin crates/kore-app/packaging/macos/bundle.sh   # target/<triple>/release/…
#
# Bundle identity: product name "kore", bundle id dev.kore.desktop, version
# from kore-app's Cargo.toml, LSMinimumSystemVersion 14.0, and icon.icns from
# crates/kore-app/assets/icons.
set -euo pipefail

CRATE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ROOT="$(cd "$CRATE_DIR/../.." && pwd)"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
PRODUCT="kore"
IDENTIFIER="dev.kore.desktop"
MIN_MACOS="14.0"
VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$CRATE_DIR/Cargo.toml" | head -1)"

RELEASE_DIR="$TARGET_DIR/${TARGET:+$TARGET/}release"

if [[ -z "${SKIP_BUILD:-}" ]]; then
  (cd "$ROOT" && cargo build --release -p kore-app ${TARGET:+--target "$TARGET"})
fi

BIN="$RELEASE_DIR/kore"
[[ -x "$BIN" ]] || { echo "missing $BIN" >&2; exit 1; }

APP="$RELEASE_DIR/bundle/macos/$PRODUCT.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/$PRODUCT"
cp "$CRATE_DIR/assets/icons/icon.icns" "$APP/Contents/Resources/icon.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>English</string>
  <key>CFBundleDisplayName</key>
  <string>$PRODUCT</string>
  <key>CFBundleExecutable</key>
  <string>$PRODUCT</string>
  <key>CFBundleIconFile</key>
  <string>icon.icns</string>
  <key>CFBundleIdentifier</key>
  <string>$IDENTIFIER</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>$PRODUCT</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>$VERSION</string>
  <key>CFBundleVersion</key>
  <string>$VERSION</string>
  <key>CSResourcesFileMapped</key>
  <true/>
  <key>LSMinimumSystemVersion</key>
  <string>$MIN_MACOS</string>
  <key>LSRequiresCarbon</key>
  <true/>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST
plutil -lint "$APP/Contents/Info.plist" >/dev/null

# Ad-hoc signature (seals Info.plist + resources; Apple Silicon requires a signature
# to run). Releases are not Developer ID signed or notarized: install.sh and the
# in-app updater clear the quarantine flag.
codesign --force --sign - --identifier "$IDENTIFIER" "$APP"

echo "$APP"
