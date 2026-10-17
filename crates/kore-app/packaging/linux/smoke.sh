#!/usr/bin/env bash
# Headless Linux smoke test: launch the container-built `kore` under Xvfb with
# Mesa lavapipe (software Vulkan) and save a root-window screenshot.
#
#   crates/kore-app/packaging/linux/smoke.sh <out-dir> [kubeconfig] [seconds]
#
# Expects the image `kore-linux-build` (Dockerfile here) and a release build in
# the `kore9a-target` volume (override with TARGET_VOLUME). The kubeconfig is
# copied and its 127.0.0.1 API servers are rewritten to host.docker.internal
# with TLS verification off (the kind certs don't cover that name). If the
# cluster is unreachable the app must still render (ConnectionSetup).
set -euo pipefail
OUT="$(mkdir -p "$1" && cd "$1" && pwd)"
KUBECONFIG_SRC="${2:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)/harness/.kube/config}"
SECS="${3:-20}"
TARGET_VOLUME="${TARGET_VOLUME:-kore9a-target}"

# kind certs don't list host.docker.internal: drop the CA, skip verification.
sed -E -e 's#server: https://(127\.0\.0\.1|localhost):#server: https://host.docker.internal:#' \
       -e 's#^( *)certificate-authority-data: .*#\1insecure-skip-tls-verify: true#' \
       "$KUBECONFIG_SRC" > "$OUT/kubeconfig"

docker run --rm --name kore-smoke \
  -v "$TARGET_VOLUME":/target:ro -v "$OUT":/out \
  -e KUBECONFIG=/out/kubeconfig -e SECS="$SECS" \
  kore-linux-build bash -c '
    set -u
    export HOME=/tmp/kore-home; mkdir -p "$HOME"
    export DISPLAY=:99 LIBGL_ALWAYS_SOFTWARE=1 RUST_LOG=info
    export VK_ICD_FILENAMES=$(ls /usr/share/vulkan/icd.d/lvp_icd*.json | head -1)
    Xvfb :99 -screen 0 1600x1000x24 -nolisten tcp >/out/xvfb.log 2>&1 &
    for i in $(seq 50); do xdpyinfo >/dev/null 2>&1 && break; sleep 0.2; done
    vulkaninfo --summary >/out/vulkaninfo.txt 2>&1 || true
    /target/release/kore >/out/kore.log 2>&1 &
    PID=$!
    sleep "$SECS"
    if kill -0 $PID 2>/dev/null; then echo "kore running (pid $PID)"; else echo "kore EXITED"; fi
    xwininfo -root -tree >/out/xwininfo.txt 2>&1 || true
    import -window root /out/linux-smoke.png
    kill $PID 2>/dev/null; wait $PID 2>/dev/null
    echo "screenshot: /out/linux-smoke.png"
  '
