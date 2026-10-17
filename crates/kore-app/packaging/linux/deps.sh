#!/usr/bin/env bash
# Install gpui's Linux build deps, the headless smoke-test stack (Xvfb + Mesa
# lavapipe + ImageMagick), fonts and a Rust toolchain on Debian/Ubuntu.
# Used by the Dockerfile; also runnable on a Debian/Ubuntu dev box as root.
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y --no-install-recommends \
  ca-certificates curl build-essential pkg-config cmake clang git \
  libx11-dev libx11-xcb-dev libxcb1-dev libxcb-xkb-dev libxkbcommon-dev libxkbcommon-x11-dev \
  libwayland-dev libvulkan-dev libfontconfig-dev libfreetype-dev libasound2-dev \
  libssl-dev libzstd-dev libsqlite3-dev \
  xvfb xauth x11-utils imagemagick mesa-vulkan-drivers vulkan-tools \
  fonts-inter fonts-dejavu-core xdg-utils
rm -rf /var/lib/apt/lists/*
if ! command -v cargo >/dev/null; then
  curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
fi
