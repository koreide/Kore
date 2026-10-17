#!/usr/bin/env bash
# Install the Kore binary, .desktop entry and icons for the current user (or PREFIX).
#   crates/kore-app/packaging/linux/install.sh [path/to/kore]   # from the repo
#   ./install.sh                                                # from an extracted release tarball
# PREFIX defaults to ~/.local. The window's app_id / WM_CLASS is dev.kore.desktop,
# which matches the desktop file name and StartupWMClass so docks show this icon.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ -x "$HERE/kore" ]]; then
  # Release tarball layout (package.sh): kore, icons/ and the desktop file next to this script.
  ICONS="$HERE/icons"
  BIN="${1:-$HERE/kore}"
else
  ICONS="$HERE/../../assets/icons"
  BIN="${1:-$HERE/../../../../target/release/kore}"
fi
PREFIX="${PREFIX:-$HOME/.local}"
install -Dm755 "$BIN" "$PREFIX/bin/kore"
install -Dm644 "$HERE/dev.kore.desktop.desktop" "$PREFIX/share/applications/dev.kore.desktop.desktop"
install -Dm644 "$ICONS/32x32.png" "$PREFIX/share/icons/hicolor/32x32/apps/dev.kore.desktop.png"
install -Dm644 "$ICONS/128x128.png" "$PREFIX/share/icons/hicolor/128x128/apps/dev.kore.desktop.png"
install -Dm644 "$ICONS/128x128@2x.png" "$PREFIX/share/icons/hicolor/256x256/apps/dev.kore.desktop.png"
install -Dm644 "$ICONS/icon.png" "$PREFIX/share/icons/hicolor/512x512/apps/dev.kore.desktop.png"
command -v update-desktop-database >/dev/null && update-desktop-database "$PREFIX/share/applications" || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -f -t "$PREFIX/share/icons/hicolor" || true
echo "installed kore to $PREFIX"
