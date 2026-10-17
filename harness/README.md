# Kore harness

End-to-end harness for the Kore desktop app against local kind clusters. It is Layers 2 and 3
of Kore's three test layers. Layer 1 is `cargo test`; the routing rules are in `CLAUDE.md`
under "Testing layers".

- **Layer 2 (default, `--layer app`).** The release app built with `--features test-harness`
  runs in the background and is driven through its control server, a Unix socket at
  `KORE_CONTROL_SOCKET`. Input is injected inside the app and screenshots are rendered off-screen,
  so the window can be covered or minimized and **your mouse and keyboard stay yours**.
  The run fails if anything posts a real OS input event.
- **Layer 3 (`--layer os`, `harness/scenarios/os/`).** Real CGEvent input and window-server
  capture, for the OS event paths Layer 2 can't reach. It takes over the mouse and keyboard, so it
  runs only named scenarios, and only when the user asks.

Both layers locate text by OCR on the screenshot and read the app's element registry, view state
and frame timings over the control socket. Screenshots are diffed against a known-good reference
run (`harness/BASELINE_RUN`).

## Prerequisites (macOS)

- Docker, `kind`, `kubectl` and `helm` on PATH.
- The app binary built with the control server: `cargo build --release -p kore-app --features test-harness`.
  Release artifacts are built without the feature and never contain it.
- Layer 3 only: Accessibility **and** Screen Recording permission for your terminal
  (System Settings → Privacy & Security). Layer 2 needs neither.

## Quick start

```bash
cargo build -p kore-harness
H=./target/debug/kore-harness

$H cluster up                      # kind clusters kore-harness + kore-harness-2, seeded fixtures
cargo build --release -p kore-app --features test-harness
$H run                             # Layer 2: every scenario, in the background
$H run -f pods -f helm             # scenarios whose name contains, or tags equal, a filter
$H run --binary path/to/kore       # a different build
$H run --layer os os-shift-chords  # Layer 3: named scenarios only (takes over input; ask first)
$H compare harness/runs/<ref-run> harness/runs/<new-run>   # visual regression report
$H cluster down
```

The `justfile` wraps these: `just test-app`, `just test-os <name>`, `just compare <run>`.

Each run writes `harness/runs/<timestamp>/` containing `report.html`, `summary.json`,
one directory per scenario (screenshots, `result.json`, `NN-FAILURE.png` on failure), `app.log`
and the sandboxed `home/`. Runs queue on a lock file (`.app.lock` for Layer 2, `.gui.lock` for
Layer 3).

### Coordinates
Layer 2 sizes the window content like Layer 3's placed window (1400×872 points) and gives screenshots
a 28-point titlebar band. Normalized `within` regions, `click_at`, `mask_rects` and screenshot sizes
(2800×1800) therefore mean the same thing in both layers.

### Layer 3 safety
Layer 3 always gives the input devices back. It releases every modifier and mouse button, and stops the app:
- after each scenario;
- on Ctrl-C, SIGTERM or a panic;
- when a scenario exceeds its hard timeout (`--timeout`, 60 s by default; exit status 124).

### Isolation

- **Kubeconfig:** the harness never reads or writes `~/.kube/config`. Clusters live in
  `harness/.kube/config`, which is passed to the app as `KUBECONFIG`.
- **App state:** the app runs with `HOME` and `CFFIXED_USER_HOME` set to the run's `home/`,
  so `~/.kore`, the events DB and preferences start fresh every run.
- **Exception:** the OS keychain (AI API keys) is not sandboxed. AI scenarios use the
  Ollama provider, which needs no key.

### Driving interactively

The MCP server is the easiest way. `.mcp.json` registers it as `kore`, and it provides these
tools: launch, snapshot, click, hover, type, key, scroll, action, wait, screenshot, text and quit.
From a shell:

```bash
$H launch                                       # prints pid + control socket; app stays open in the background
$H do --socket <sock> text                      # OCR dump: window-space coordinates, normalized (x,y), text
$H do --socket <sock> click "Deployments"
$H do --socket <sock> key cmd+k
$H do --socket <sock> type "web"
$H do --socket <sock> screenshot /tmp/x.png
$H do --socket <sock> snapshot                  # element registry + view state
$H do --socket <sock> frames --reset            # frame timings (p50/p95/p99/max, over-budget count)
```

### AI scenarios

Run `$H mock-ai --port 47900` (a deterministic Ollama/OpenAI/Anthropic-compatible server),
then set the app's Ollama Base URL to `http://127.0.0.1:47900` (`_lib/cluster-ai-ollama-mock`
does this).

Mock triggers in the last user message:
- `boom`: HTTP 500
- `health`: a `get_cluster_health` tool call
- anything else: a fixed markdown answer containing a `| Pod | Status |` table and a code block
- streams always end with the text `KORE-MOCK-STREAM`
- port+1 (default 47901) answers every request with HTTP 503, for Test Connection error checks
- a tool-call follow-up (request carrying a tool result) is answered after a 2s delay, so the
  "Querying ..." status stays visible long enough to assert

## Fixtures (`harness/cluster/fixtures`, chart in `harness/cluster/charts`)

| Object | Namespace | Purpose |
|---|---|---|
| deploy `web` (3 replicas, 3 revisions: nginx 1.25 → 1.26 → 1.27-alpine) | kore-demo | rollout history, rollback, deployment details, graph |
| deploy `logger` (2 replicas, labels `app=logger`), lines `KORE-LOG <pod> line=N level=INFO\|ERROR` | kore-demo | logs streaming, multi-pod logs, search |
| pod `multi-container` (containers `main`, `sidecar`; logs `MAIN-CONTAINER-LOG` / `SIDECAR-CONTAINER-LOG`) | kore-demo | container selector, exec |
| pod `crashloop` (prints `KORE-CRASH about to fail`, exits 1) | kore-demo | CrashLoopBackOff, restarts, dashboard hotlist |
| pod `unschedulable` (Pending, FailedScheduling) | kore-demo | pending status, warnings |
| svc `web`, ingress `web` (host `web.kore.local`), cm `web-config`, secret `web-secret` (`API_TOKEN`) | kore-demo | resource kinds |
| job `one-shot` (Completed), cronjob `ticker` (yearly schedule) | kore-demo | jobs and cronjobs |
| CRD `widgets.kore.dev` (Widget, printer columns Color/Size); `blue-widget`, `red-widget` | kore-demo | CRD browser |
| NetworkPolicy `default-deny-ingress`, `allow-logger-to-web` | kore-demo | network policy view |
| SA `viewer-bot`, `deployer-bot` (kore-rbac); Role `pod-reader`; ClusterRole `kore-deployer` bound to deployer-bot, `alice@kore.dev`, group `kore-devs` | kore-demo / cluster | RBAC views |
| Helm release `kore-demo` (2 revisions, chart `kore-demo-0.1.0`) | kore-demo | Helm list, detail, history, rollback |
| metrics-server | kube-system | pod metrics, dashboard |
| deploy `remote-web` in a **second** cluster (`kind-kore-harness-2`, ns `kore-remote`) | kore-remote | multi-cluster view |

The app starts in context `kind-kore-harness`, namespace `default`. Use `include: ns-kore-demo` to switch.

## Scenario DSL (`harness/scenarios/*.yaml`)

Only top-level `*.yaml` files are Layer 2 scenarios. `os/` holds Layer 3 and `_lib/` holds includes.
Behavior checks that don't need the real app or cluster live in Layer 1 (`crates/kore-app/src/tests/`).

```yaml
name: deployment-rollback          # unique; also the output directory name
description: What the user-visible feature is and what passing means
tags: [deployments, core]          # `run -f <tag>` selects by tag or name substring
layer: app                         # default; `os` = Layer 3 (put the file in harness/scenarios/os/)
fresh_app: false                   # restart the app before this scenario
env: [[KEY, value]]                # extra app env (forces a restart)
steps:
  - include: ns-kore-demo          # splice harness/scenarios/_lib/ns-kore-demo.yaml
  - click: {id: sidebar.kind.deployments, until: "Deployments"}   # automation element (preferred: stable)
  - hover: {id: graph.node.web}    # mouse move (element id or text)
  - click: Deployments             # click visible text (first match, top-to-bottom)
  - click: {text: web, within: [0.2, 0.1, 1, 1], nth: 0, exact: true, until: "Rollback", timeout: 10, offset: [0, 0]}
  - click: {text: multi-container, within: [0.19, 0.14, 0.45, 1], at_x: 0.91}   # same row, x=0.91 (icon column)
  - double_click: …                # same fields as click
  - right_click: …
  - click_at: [0.5, 0.5]           # normalized window coordinates
  - key: cmd+k                     # chords: cmd/shift/alt/ctrl + key (enter, escape, tab, up, down, a, /, ?, 1…)
  - keys: [j, j, l]
  - type: "kore-demo"
  - wait: {text: "re:^web-\\w+-\\w+$", timeout: 15}
  - wait_gone: {text: Loading, timeout: 10}
  - expect: {texts: [READY, UP-TO-DATE], within: [0.2, 0, 1, 0.3], timeout: 5}
  - expect: {texts: [crashloop], within: [0.19, 0.14, 0.4, 1], crop: true}   # OCR only the region (full-window passes can drop lines)
  - expect_not: {texts: [Error]}
  - screenshot: {name: rollback-timeline, mask: ["re:^\\d+[smhd]$"], mask_rects: [[0.8, 0.1, 1, 0.2]], tolerance: 0.04}
  - scroll: {at: [0.6, 0.5], dy: 600}
  - drag: {from: [0.5, 0.5], to: [0.6, 0.6]}
  - sleep: 500
  - shell: "kubectl -n kore-demo get deploy web -o jsonpath='{.spec.template.spec.containers[0].image}'"
  - shell: {cmd: "kubectl …", expect_output: "nginx:1.26-alpine", retry: 30}   # env: KUBECONFIG, HARNESS_APP_HOME (the app's sandbox HOME)
  - state: [["/view", "pods"]]     # automation snapshot, JSON pointer → value
  - element: {id: header.count, label: "^\\d+$", timeout: 5}   # automation element exists; optional label regex (values OCR misreads)
  - element: {id: "toast.*", label: "^Port forward active on :18080$", timeout: 10}   # `id` ending in `*` = prefix match, e.g. toasts
  - element_gone: {id: searchbar.count, timeout: 5}   # no element matches (e.g. a closed search bar)
  - perf: {max_p95_ms: 16.7, steps: [{scroll: {at: [0.6, 0.5], dy: 3000}}]}   # frame timings over the inner steps
  - window_title: Kore
  - restart_app: {wait: Kubernetes Desktop}   # quit + relaunch in the same sandbox (persistence checks)
  - expect_color: {text: Pods, within: [0, 0.6, 0.19, 0.95], colors: ["#58d0ff"], min_pixels: 6, max_dist: 60}
```

`env` values may use `${HARNESS_DIR}` (absolute path of `harness/`), e.g.
`env: [[KUBECONFIG, "${HARNESS_DIR}/scenarios/_lib/shell-broken.kubeconfig:${HARNESS_DIR}/.kube/config"]]`.
Note that `fresh_app` and `restart_app` restart the app but keep the run's sandbox `home/`, so
persisted state (settings, favorites) carries over between scenarios of one run: restore it.
A scenario that fails, or that sets `env`, is followed by an app restart, so dialogs or
half-finished state it leaves on screen do not cascade into later scenarios. Persisted
state still carries over.

Text matching is case-insensitive and OCR-tolerant: `l`, `I`, `1` and `|` are treated as
the same glyph, as are `o` and `0`, and spaces and punctuation other than `-/._:` are
ignored. Prefix a query with `re:` to match a raw regex against the OCR'd line instead.
Use `within` (a normalized `[x0, y0, x1, y1]` window region) to disambiguate. The sidebar
spans x 0–0.19, and the content header spans y 0–0.12.

Writing scenarios that hold up:
- **Assert what a user sees.** Text, not internal state. Use `state` only for things that
  have no visible text.
- **Restore what you mutate** (scale back, roll forward, delete what you created), so the
  scenario can run again.
- **Mask live values** in screenshots: ages, CPU/memory numbers, restart counts and
  timestamps. Otherwise visual diffs are noise.
- **Wait for the end state.** After an action, `wait` for the resulting text, never just `sleep`.
- **Short-lived UI (toasts) via `element`.** A 3s toast can fall between OCR polls; the
  automation registry is polled every 250ms.

Scenarios tagged `fix` are regression guards for the deliberate fixes ("Deliberate Fix N"),
each asserting the required behavior (see `docs/gpui-port/`).
