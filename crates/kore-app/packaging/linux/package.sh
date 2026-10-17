#!/usr/bin/env bash
# Package a Linux release tarball: kore, install.sh, the .desktop entry and icons.
#
#   crates/kore-app/packaging/linux/package.sh <path/to/kore> <out-dir>
#
# Writes <out-dir>/kore_<version>_<arch>.tar.gz containing kore_<version>_<arch>/;
# users extract it and run ./install.sh (installs to ~/.local, or PREFIX).
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CRATE_DIR="$(cd "$HERE/../.." && pwd)"
BIN="${1:?usage: package.sh <kore binary> <out-dir>}"
OUT="${2:?usage: package.sh <kore binary> <out-dir>}"
VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$CRATE_DIR/Cargo.toml" | head -1)"
ARCH="${ARCH:-$(uname -m)}"
NAME="kore_${VERSION}_${ARCH}"

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$STAGE/$NAME/icons"
install -m755 "$BIN" "$STAGE/$NAME/kore"
install -m755 "$HERE/install.sh" "$STAGE/$NAME/install.sh"
install -m644 "$HERE/dev.kore.desktop.desktop" "$STAGE/$NAME/"
for f in 32x32.png 128x128.png 128x128@2x.png icon.png; do
  install -m644 "$CRATE_DIR/assets/icons/$f" "$STAGE/$NAME/icons/$f"
done

mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
tar -C "$STAGE" -czf "$OUT/$NAME.tar.gz" "$NAME"
echo "$OUT/$NAME.tar.gz"
