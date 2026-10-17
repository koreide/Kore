# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Kore is a native Kubernetes IDE for macOS, Linux and Windows. It is a single Rust binary:
- a backend library, `kore-core` (kube-rs);
- a GPU-rendered UI, `kore-app` (GPUI via gpui-kit / gpui-component).

Features:
- 11 resource kinds with real-time watches
- pod logs, exec, metrics and port forwarding
- YAML editing with diff and live apply
- deployment rollback
- CRD browsing, dependency graph, Helm management, RBAC simulator, network policies
- AI troubleshooting (OpenAI, Anthropic, Ollama)
- multi-cluster view
- persistent event history

Behavior and visual specs for every area of the app are in `docs/gpui-port/spec/` (01 shell, 02 details & workloads, 03 cluster views, 04 backend).

## Commands

```bash
cargo run -p kore-app                                   # run the app (debug)
cargo build --release -p kore-app                       # target/release/kore (what ships: no control server)
cargo clippy -p kore-app -p kore-core -- -D warnings    # must stay clean (`just lint` = what CI runs)
crates/kore-app/packaging/macos/bundle.sh               # kore.app (dev.kore.desktop)

just test-unit                  # Layer 1: cargo test (unit + headless UI tests on the fake cluster)
just test-app [-f <filter>]     # Layer 2: real app in the background via its control socket
just test-os <scenario>...      # Layer 3: real OS input. ONLY after the user approves (see below)
just test-kind                  # kore-core backend tests against the kind clusters
```
`just` is `brew install just`. Each recipe is a thin wrapper, so you can read `justfile` and run the commands directly.

## Testing layers

Three layers, cheapest first. **For every change, pick the lowest layer that can actually catch a regression.**

| Layer | What runs | Where | Cost |
|---|---|---|---|
| 1 | `#[gpui_kit::test]` tests of the real UI in a headless window against `kore-testkit`'s in-memory Kubernetes API (`crates/kore-app/src/tests/`, driver `crates/kore-app/src/testing.rs`), plus unit tests | `cargo test`, CI | seconds; no cluster, network, keychain or window |
| 2 | The real release app, built with `--features test-harness`. It is driven through its in-app control server (Unix socket): input is injected inside the app, screenshots are rendered off-screen, YAML scenarios run against the kind clusters (`harness/scenarios/*.yaml`) | `just test-app`, or the `kore` MCP server (`.mcp.json`) | minutes; runs in the background, never touches the mouse or keyboard |
| 3 | Real macOS input (CGEvent) and window-server capture (`harness/scenarios/os/`) | `just test-os <name>` | takes over the user's mouse and keyboard |

### Routing rules
- **Logic, state, actions, keybindings, data handling:** Layer 1. Always add or update a test.
- **Layout, styling, rendering, new views, anything visual:** Layer 1 for the behavior, plus Layer 2 with a screenshot check.
- **Kubernetes integration behavior:** Layer 1 with the fake cluster (`FakeCluster`, `FakeEnv`), plus Layer 2 against the kind clusters when relevant (real API server semantics, watches, helm/kubectl, RBAC).
- **Platform/input layer (NSEvent handling, IME, drag & drop outside the window, window management, system menus, permissions, multi-window focus):** Layer 3, **but only after asking the user**.
- **Before choosing Layer 3,** check whether Layer 2 can cover it. Prefer extending the control server (`crates/kore-app/src/automation/server.rs`, protocol in `crates/kore-automation`) over falling back to real input.
- **After any UI change,** run the relevant Layer 2 scenarios yourself and look at the screenshots in `harness/runs/<run>/` before calling the work done.

### Layer 3 is the user's call
Layer 3 takes over the user's mouse and keyboard, so **never run it on your own initiative.** When a change needs it:
1. Stop and explain why Layers 1–2 can't cover it.
2. Ask the user.
3. After they approve, run only the specific scenarios needed: `just test-os <name>`. The harness refuses to run Layer 3 without names unless `--all` is given; don't use `--all` without explicit approval.

The run releases all modifiers and mouse buttons and stops the app on completion, Ctrl-C, panic, or the per-scenario hard timeout (`--timeout`, default 60 s).

### How to run each layer
- **Layer 1.** `just test-unit`, or `cargo test -p kore-app tests::<area>`. To add tests:
  - Boot the UI with `Kore::boot(cx)`, which loads the harness world snapshot from `crates/kore-testkit/fixtures`.
  - Drive it by automation id: `k.click(id)`, `k.keys("cmd-k")`, `k.type_text(..)`.
  - Assert on `k.state()`, `k.element(id)` and `k.wait_*`.
  - Shape the world with `k.cluster.apply/modify/delete/forbid/set_logs`. `FakeEnv` provides fake CLIs (`tool`), update/AI endpoints and broken contexts; `kore_testkit::MockAi` is the AI provider.
- **Layer 2.** Needs Docker + kind: `just test-app -f <filter>`. Write scenarios with `click: {id: ...}`, `element:`, `state:` and `wait:`/`expect:`, then `just compare <run>` for visual regression against `harness/BASELINE_RUN`. To drive the app interactively, use the `kore` MCP server tools (`launch`, `snapshot`, `click`, `type`, `key`, `wait`, `screenshot`, ...).
- **Layer 3.** `just test-os <scenario-name>`, after approval.

Give new interactive elements a stable automation id with `.auto_id(id, role, label)`. It is layout-neutral. Layers 1, 2 and 3 all target these ids.

## Architecture

- **`crates/kore-core`**: all backend logic, with no UI dependency.
  - `Core` owns its tokio runtime and exposes one async method per operation.
  - Streams (watches, logs, exec, AI) return `Subscription<T>`; dropping it cancels the stream.
  - The `state/` modules: resources, logs, exec, metrics, port_forward, yaml, rollback, dashboard, event_store (SQLite), multi_logs, crd, graph, helm, multi_cluster, rbac, network_policy, debug, update, ai.
  - `json::sort_keys` keeps JSON/YAML output key-sorted. serde_json's `preserve_order` is enabled by GPUI.
- **`crates/kore-app`** (binary `kore`): the UI.
  - `main.rs`, `root.rs`: window, routing, global keymap.
  - `backend.rs`: bridge to Core.
  - `state/`: `AppState`, `ResourceStore`, settings, storage.
  - `views/`: one module per feature view.
  - `ui/`: shared widgets, e.g. inputs, dropdowns, toast, confirm, `mono_text`, markdown.
  - `theme.rs`.
  - `automation/`: element registry (`auto_id`); the Layer 2 control server exists only with the `test-harness` feature and starts when `KORE_CONTROL_SOCKET` is set.
  - `testing.rs` + `tests/`: the Layer 1 driver and UI tests (test builds only).
- **`crates/kore-automation`**: control protocol types.
- **`crates/kore-testkit`** (dev-only): `FakeCluster` (in-memory Kubernetes API for kube-rs), `FakeEnv` (sandboxed kubeconfig/data dirs/fake CLIs/offline endpoints), harness-world fixtures, `MockAi`.
- **`crates/kore-harness`**: Layer 2/3 driver (control socket client; CGEvent input + window capture for Layer 3 only), Vision OCR, scenario runner, screenshot compare, MCP server, mock AI server.

**Read `docs/gpui-port/CONVENTIONS.md` before changing UI code.** It covers the view constructor contract, Core access, the navigation API, keyboard/Esc rules, theme tokens, mono-text pitch, and automation ids.

The behavior specs are in `docs/gpui-port/spec/`; update them when you change behavior.

## Styling

- Dark theme: background `#0b1221`, surface `#101828`. The accent `#58d0ff` is user-configurable, so always use `theme::accent(cx)`.
- Fonts: system UI font, plus SF Mono (Menlo/Consolas/DejaVu Sans Mono fallback). Mono text is laid out at a 0.601em pitch via `ui::mono_text`.
- Icons are exact Lucide SVGs in `assets/lucide`. SVGs don't inherit color, so always set `.text_color` on the icon.

## Keyboard shortcuts

`Cmd/Ctrl+K` palette, `/` search, `j/k` rows, `l` details, `h` back, `1-6` tabs, `?` overlay, `Cmd/Ctrl+R` refresh, `Cmd/Ctrl+Shift+R` RBAC, `Cmd/Ctrl+F` search in logs/describe/YAML.

## Prerequisites

- Rust stable; a `~/.kube/config` with a reachable context.
- Optional: `helm` (Helm features) and `kubectl` (port forwarding, exec) in `PATH`.
- macOS: Xcode CLT.
- Linux: see `crates/kore-app/packaging/linux/deps.sh`.
- Windows: MSVC. Release builds need a Windows host because of GPUI's `fxc.exe` shader step.
- Harness: Docker + kind (Layer 2). Layer 3 also needs macOS Accessibility + Screen Recording permission for the terminal.
