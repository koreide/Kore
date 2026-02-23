# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Kore is a macOS desktop Kubernetes IDE (Tauri v2) combining a Rust backend (kube-rs) with a React frontend (Vite + Tailwind + shadcn/ui). It supports 10 resource types, real-time watches, pod exec/logs/metrics, deployment rollback, label filtering, port forwarding, YAML editing with live apply, CRD browsing, dependency graphs, Helm management, AI troubleshooting, multi-cluster view, and persistent event history.

## Commands

```bash
npm install                # Install frontend dependencies
npm run tauri:dev          # Full app dev mode (Vite + Tauri) — primary dev command
npm run dev                # Frontend only (Vite dev server on localhost:5173)
npm run build              # TypeScript check + Vite build (frontend only)
npm run tauri:build        # Production macOS binary
npm run lint               # ESLint
npx vitest run             # Frontend unit tests
```

Rust backend builds automatically during `tauri:dev` and `tauri:build`. To check Rust code independently:
```bash
cd src-tauri && cargo check
cd src-tauri && cargo clippy
```

## Architecture

**Two-process model**: Tauri spawns a Rust backend process and a webview rendering the React frontend. They communicate via Tauri's invoke (request/response) and event (push/streaming) systems.

### Backend (`src-tauri/src/`)

- **`main.rs`** — Tauri app setup, registers all commands and initializes `K8sState`
- **`commands.rs`** — All Tauri command handlers (~30 commands)
- **`error.rs`** — Custom error types via thiserror (`K8sError` enum)
- **`constants.rs`** — Shared constants

**`state/` module** — Core backend logic, split into focused files:
- **`mod.rs`** — `K8sState` struct wrapping kube-rs client with `Arc<RwLock<>>`. Context/namespace management, resource listing
- **`resources.rs`** — Resource CRUD, watching, describing, event listing, search across kinds
- **`logs.rs`** — Pod log fetching and streaming (`pod-logs://{ns}/{pod}` events)
- **`exec.rs`** — Pod exec terminal sessions (`exec-stdout://{session}` events)
- **`metrics.rs`** — Pod metrics via Metrics Server API
- **`port_forward.rs`** — Port forwarding via kubectl subprocess with TCP proxying
- **`yaml.rs`** — YAML get/apply/diff for resources (server-side apply via `Patch::Apply`)
- **`rollback.rs`** — Deployment revision history (reads ReplicaSets by ownerReference) and rollback
- **`dashboard.rs`** — Cluster health aggregation (pods, nodes, metrics, events → health score 0-100)
- **`event_store.rs`** — SQLite persistent event store (`~/.kore/events.db` via rusqlite)
- **`multi_logs.rs`** — Aggregated multi-pod log streaming by label selector
- **`crd.rs`** — CRD discovery and dynamic resource browsing via `DynamicObject`/`ApiResource`
- **`graph.rs`** — Resource dependency graph (ownerReferences + label selector matching)
- **`helm.rs`** — Helm release management (shells out to `helm` CLI)
- **`multi_cluster.rs`** — Cross-context resource listing with temporary Client instances
- **`ai.rs`** — AI troubleshooting (OpenAI/Anthropic/Ollama streaming via reqwest)

**Key patterns**: Commands access `K8sState` via Tauri's managed state. Watchers emit `resource://event` Tauri events. Port forwarding uses kubectl subprocess with TCP proxying. AI responses stream via `ai-response://{session}` events. Multi-pod logs emit to `multi-pod-logs://{id}`. Watch cancellation uses oneshot channels.

### Frontend (`src/`)

- **`App.tsx`** — Root component with all top-level state, routing between views (table/details/dashboard/graph/crds/helm/settings), keyboard handler
- **`lib/api.ts`** — Tauri invoke wrappers for all backend commands
- **`lib/types.ts`** — TypeScript types (ResourceItem, ResourceKind, AppView, WatchEventPayload, etc.)
- **`lib/transforms.ts`** — `toResourceItem()` converts raw K8s objects to unified `ResourceItem`
- **`lib/errors.ts`** — Error formatting utilities
- **`lib/utils.ts`** — `cn()` classname utility

**`hooks/`**:
- `use-k8s-context.ts` — Context/namespace state management
- `use-resource-watch.ts` — Resource listing + real-time watch subscription (supports multi-cluster)
- `use-keyboard-shortcuts.ts` — Global keyboard shortcut registration
- `use-pinned-resources.ts` — Bookmark/pin resources (localStorage)
- `use-restart-history.ts` — Tracks pod restart counts over time for sparklines

**`components/`** (key components):
- `resource-table.tsx` — TanStack Table with kind-specific columns, pin button, restart sparklines, multi-cluster column
- `pod-details-view.tsx` — Tabbed: logs/describe/yaml/metrics/events/shell. Log download, Copy YAML
- `deployment-details-view.tsx` — Tabbed: describe/yaml/rollback/logs/events
- `resource-details-view.tsx` — Generic detail view with describe/yaml/events tabs
- `yaml-editor.tsx` — YAML viewer/editor with diff view and live apply
- `deployment-rollback.tsx` — Revision timeline with image diffs and one-click rollback
- `cluster-dashboard.tsx` — Health score, pod ring chart (Recharts), node bars, restart hotlist
- `resource-graph.tsx` — Interactive SVG dependency graph with hierarchical layout, zoom/pan
- `crd-browser.tsx` — Two-panel CRD discovery and instance browsing
- `helm-releases.tsx` / `helm-detail-view.tsx` — Helm release table and detail (values/manifest/history/rollback)
- `ai-panel.tsx` / `ai-settings.tsx` — AI chat panel with streaming, provider config
- `multi-pod-logs.tsx` — Color-coded aggregated log viewer
- `diff-viewer.tsx` — Side-by-side YAML diff with LCS algorithm
- `events-timeline.tsx` — Live events + SQLite history toggle with time range
- `command-palette.tsx` — cmdk-based palette with resource search, views, quick filters, actions
- `sidebar.tsx` — Navigation with context/namespace dropdowns, views, pinned resources, multi-cluster toggle
- `settings.tsx` — Preferences page (event retention, accent color, shortcuts)
- `shortcut-overlay.tsx` — Keyboard shortcut reference modal
- `restart-sparkline.tsx` — Inline SVG sparkline for pod restart trends
- `pinned-resources.tsx` — Sidebar pinned resources list
- `label-filter-bar.tsx` — Label selector filter UI
- `port-forwarding.tsx` — Port forward management panel
- `pod-metrics.tsx` — CPU/memory charts (Recharts)
- `exec-terminal.tsx` — Pod exec terminal
- `searchable-dropdown.tsx` — Dropdown with search and localStorage favorites
- `toast.tsx` — Toast notification system
- `confirm-dialog.tsx` — Confirmation dialog
- `error-boundary.tsx` — React error boundary

**Resource kinds**: pods, deployments, services, nodes, events, configmaps, secrets, ingresses, jobs, cronjobs.

**Keyboard shortcuts**: `Cmd+K` palette, `/` focus search, `j/k` navigate rows, `l` enter detail, `h` go back, `1-6` switch tabs in detail views, `?` shortcut overlay.

### Styling

Dark theme with custom Tailwind config: background `#0b1221`, surface `#101828`, accent `#58d0ff`. Monospace font (SFMono). Custom `.glass` utility for glassmorphism. Animations via Framer Motion.

### Path alias

`@` maps to `/src` (configured in both vite.config.ts and tsconfig.json).

## Key Dependencies

**Rust**: kube, k8s-openapi, tokio, serde/serde_json, serde_yaml, similar (diff), rusqlite (SQLite), reqwest (AI HTTP), tauri, thiserror, tracing, rand
**Frontend**: react, @tanstack/react-table, recharts, cmdk, framer-motion, lucide-react, @tauri-apps/api

## Prerequisites

- macOS 14+, Rust stable, Node.js 18+, Xcode CLT
- Valid `~/.kube/config` with at least one reachable context
- Optional: `helm` in PATH for Helm features, `kubectl` for port forwarding and exec
