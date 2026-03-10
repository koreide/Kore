#!/bin/sh
# Kore — Kubernetes Desktop IDE installer
# Usage: curl -fsSL https://raw.githubusercontent.com/koreide/Kore/main/install.sh | bash
set -e

# ── Colors ────────────────────────────────────────────────────────────
if [ -t 1 ]; then
  BOLD='\033[1m'
  CYAN='\033[36m'
  GREEN='\033[32m'
  RED='\033[31m'
  YELLOW='\033[33m'
  RESET='\033[0m'
else
  BOLD='' CYAN='' GREEN='' RED='' YELLOW='' RESET=''
fi

info()  { printf "${CYAN}::${RESET} %s\n" "$1"; }
ok()    { printf "${GREEN}✓${RESET} %s\n" "$1"; }
warn()  { printf "${YELLOW}!${RESET} %s\n" "$1"; }
fail()  { printf "${RED}✗${RESET} %s\n" "$1" >&2; exit 1; }

# ── Pre-checks ────────────────────────────────────────────────────────
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Darwin) ;;
  *)      fail "Kore currently supports macOS only. Got: $OS" ;;
esac

case "$ARCH" in
  arm64|aarch64) ARCH_LABEL="aarch64" ;;
  x86_64)        ARCH_LABEL="x86_64"  ;;
  *)             fail "Unsupported architecture: $ARCH" ;;
esac

command -v curl >/dev/null 2>&1 || fail "curl is required but not found."

# ── Fetch latest release ──────────────────────────────────────────────
REPO="koreide/Kore"
API_URL="https://api.github.com/repos/${REPO}/releases/latest"

info "Fetching latest Kore release..."
RELEASE_JSON="$(curl -fsSL "$API_URL")" || fail "Failed to query GitHub releases API."

TAG="$(printf '%s' "$RELEASE_JSON" | grep '"tag_name"' | head -1 | sed 's/.*: *"\(.*\)".*/\1/')"
[ -n "$TAG" ] || fail "Could not determine latest release tag."
ok "Latest version: ${BOLD}${TAG}${RESET}"

# ── Find the right DMG asset ──────────────────────────────────────────
DMG_URL="$(printf '%s' "$RELEASE_JSON" \
  | grep '"browser_download_url"' \
  | grep -i "${ARCH_LABEL}" \
  | grep -i '\.dmg"' \
  | head -1 \
  | sed 's/.*"\(https[^"]*\)".*/\1/')"

# Fallback: if no arch-specific DMG, try any DMG
if [ -z "$DMG_URL" ]; then
  DMG_URL="$(printf '%s' "$RELEASE_JSON" \
    | grep '"browser_download_url"' \
    | grep -i '\.dmg"' \
    | head -1 \
    | sed 's/.*"\(https[^"]*\)".*/\1/')"
fi

[ -n "$DMG_URL" ] || fail "No .dmg asset found for ${ARCH_LABEL} in release ${TAG}."

DMG_NAME="$(basename "$DMG_URL")"
info "Downloading ${BOLD}${DMG_NAME}${RESET}..."

# ── Download ──────────────────────────────────────────────────────────
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

curl -fSL --progress-bar -o "${TMP_DIR}/${DMG_NAME}" "$DMG_URL" \
  || fail "Download failed."
ok "Download complete"

# ── Mount & install ───────────────────────────────────────────────────
info "Installing Kore.app to /Applications..."
MOUNT_POINT="${TMP_DIR}/kore_mount"
mkdir -p "$MOUNT_POINT"

hdiutil attach "${TMP_DIR}/${DMG_NAME}" -nobrowse -quiet -mountpoint "$MOUNT_POINT" \
  || fail "Failed to mount DMG."

APP_PATH="$(find "$MOUNT_POINT" -maxdepth 1 -name '*.app' | head -1)"
[ -n "$APP_PATH" ] || { hdiutil detach "$MOUNT_POINT" -quiet 2>/dev/null; fail "No .app found in DMG."; }

# Remove previous install if present
if [ -d "/Applications/$(basename "$APP_PATH")" ]; then
  warn "Replacing existing $(basename "$APP_PATH")"
  rm -rf "/Applications/$(basename "$APP_PATH")"
fi

cp -R "$APP_PATH" /Applications/ \
  || fail "Failed to copy to /Applications. You may need to run with sudo."

hdiutil detach "$MOUNT_POINT" -quiet 2>/dev/null

ok "Kore ${TAG} installed to ${BOLD}/Applications/$(basename "$APP_PATH")${RESET}"

# ── Clear quarantine flag ─────────────────────────────────────────────
xattr -dr com.apple.quarantine "/Applications/$(basename "$APP_PATH")" 2>/dev/null || true

printf "\n${GREEN}${BOLD}Installation complete!${RESET}\n"
printf "Open Kore from your Applications folder or run:\n"
printf "  ${CYAN}open /Applications/Kore.app${RESET}\n\n"
