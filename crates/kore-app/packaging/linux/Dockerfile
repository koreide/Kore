# Linux build + headless smoke-test image for the native Kore app (crates/kore-app).
#
#   docker build -t kore-linux-build crates/kore-app/packaging/linux
#   # (any Debian/Ubuntu base works: --build-arg BASE=debian:trixie)
#
# Build (repo mounted read-only; container-local target dir + cargo cache in volumes):
#   docker run --rm -v "$PWD":/src:ro -v kore-target:/target -v kore-cargo:/root/.cargo/registry \
#     kore-linux-build cargo build --release --locked -p kore-app
#
# Smoke-launch under Xvfb + Mesa lavapipe with a screenshot: see smoke.sh.
ARG BASE=ubuntu:24.04
FROM ${BASE}
COPY deps.sh /usr/local/bin/kore-deps.sh
RUN /usr/local/bin/kore-deps.sh
ENV PATH=/root/.cargo/bin:$PATH \
    CARGO_TARGET_DIR=/target
WORKDIR /src
