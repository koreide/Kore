# Kore test layers (see CLAUDE.md "Testing layers"). Requires `just` (brew install just).

harness := "./target/debug/kore-harness"

default:
    @just --list

# Layer 1: unit tests + headless GPUI UI tests against the fake cluster. No cluster, no window.
test-unit *args:
    cargo test -p kore-core -p kore-app -p kore-testkit {{args}}

# Layer 2: the real app in the background, driven through its control socket (no OS input).
# `just test-app` runs every scenario; `just test-app -f pods -f helm` filters by name/tag.
test-app *args: _app-build _cluster
    {{harness}} run {{args}}

# Layer 3: real OS input. Takes over the mouse and keyboard; ask before running.
# Scenario names are required: `just test-os os-shift-chords`.
test-os +names: _app-build _cluster
    {{harness}} run --layer os {{names}}

# Backend integration tests against the kind clusters (no UI).
test-kind: _cluster
    KORE_KIND=1 cargo test -p kore-core -- --include-ignored --test-threads=1

# Clippy as CI runs it.
lint:
    cargo clippy -p kore-app -p kore-core -p kore-automation -p kore-harness -p kore-testkit --all-targets -- -D warnings
    cargo clippy -p kore-app --features test-harness -- -D warnings

# Visual regression of a Layer 2 run against the reference run (harness/BASELINE_RUN).
compare run:
    {{harness}} compare "$(grep -v '^#' harness/BASELINE_RUN | awk 'NF {print $1; exit}')" {{run}}

# MCP server for driving the app from an agent (stdio).
mcp: _app-build
    {{harness}} mcp

_app-build:
    cargo build --release -p kore-app --features test-harness
    cargo build -p kore-harness

_cluster:
    @kubectl --kubeconfig harness/.kube/config --context kind-kore-harness get namespace kore-demo >/dev/null 2>&1 || {{harness}} cluster up
