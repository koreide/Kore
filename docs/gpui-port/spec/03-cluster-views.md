# 03 — Cluster Views

This spec covers the Cluster Dashboard, Resource Graph, CRD Browser, Helm Releases and Helm Detail, the AI surfaces (AI Chat view, AI slide-over panel, AI Settings, chat bubbles and markdown), the RBAC Simulator (matrix, role browser, query, impersonation, identity picker, rule chain, impersonation banner, forbidden banner) and the Network Policies view of the native GPUI app.

Behavior was originally ported from the legacy Tauri/React app (git history at `3a66870`).

Implementation: `crates/kore-app/src/views/{dashboard,graph,graph_layout,crd_browser,helm_releases,helm_detail,ai_panel,ai_chat,ai_chat_bubble,ai_settings,network_policy,banners,settings}.rs`, `crates/kore-app/src/views/rbac/{mod,matrix,picker,query,roles,rule_chain}.rs`, `crates/kore-app/src/ui/{markdown,confirm_dialog,toast,widgets,scrollbar,mono_text,text}.rs`, `crates/kore-app/src/state/{ai_config,ai_stream,storage,settings,app_state}.rs`, `crates/kore-app/src/theme.rs`; backend in `crates/kore-core/src/state/{dashboard,multi_cluster,graph,crd,helm,ai,rbac,network_policy}.rs`, `crates/kore-core/src/{lib,secrets,constants}.rs`. Read `docs/gpui-port/CONVENTIONS.md` for the view contract, navigation API, keyboard rules and automation ids.

The spec describes **what the app renders and does**. Inherited oddities that are kept on purpose are marked **[QUIRK]**; places where the native app deliberately behaves better than the original are marked **[FIXED]** (with the "Deliberate Fix N" label where one exists). Section 10 lists all of them.

---

## 0. Shared foundations

### 0.1 Design tokens (`theme.rs`)

| Token | Value |
|---|---|
| `theme::BACKGROUND` | `#0b1221` |
| `theme::SURFACE` | `#101828` |
| `theme::MUTED` | `#1d2939` |
| accent | `theme::accent(cx)`, default `#58d0ff` (`DEFAULT_ACCENT`). User-selectable in Settings (`ACCENT_CHOICES`: Cyan `#58d0ff`, Violet `#a78bfa`, Emerald `#34d399`, Orange `#fb923c`, Red `#f87171`). Never hard-code it. |
| `theme::PREFLIGHT_BORDER` | `#e5e7eb` (light gray used for "accent-tinted" 1px borders, see 0.3) |
| Body text | slate-100 `#f1f5f9`; root font size 16px, line height 1.5 |
| Sans font | `theme::SANS` (macOS `.SystemUIFont`, Windows `Segoe UI`, else `Inter`) |
| Mono font | `theme::MONO` (macOS `.AppleSystemUIFontMonospaced`, Windows `Consolas`, else `DejaVu Sans Mono`) |

Palette (`theme::slate`, `emerald`, `amber`, `red`, `misc`, plus literal hex where a module has no constant):

| Name | Hex |
|---|---|
| slate-100/200/300/400/500/600/700/800/900 | `#f1f5f9` / `#e2e8f0` / `#cbd5e1` / `#94a3b8` / `#64748b` / `#475569` / `#334155` / `#1e293b` / `#0f172a` |
| emerald-200/300/400/500 | `#a7f3d0` / `#6ee7b7` / `#34d399` / `#10b981` |
| amber-200/300/400/500 | `#fde68a` / `#fcd34d` / `#fbbf24` / `#f59e0b` |
| red-300/400/500/600/700 | `#fca5a5` / `#f87171` / `#ef4444` / `#dc2626` / `#b91c1c` |
| yellow-400/500/600/700 | `#facc15` / `#eab308` / `#ca8a04` / `#a16207` |
| blue-400/500/600/700 | `#60a5fa` / `#3b82f6` / `#2563eb` / `#1d4ed8` |
| purple-200/300/400/500 | `#e9d5ff` / `#d8b4fe` / `#c084fc` / `#a855f7` |
| indigo-200/300/400/500 | `#c7d2fe` / `#a5b4fc` / `#818cf8` / `#6366f1` |
| orange-400/500 | `#fb923c` / `#f97316` |

Alpha notation: `X/NN` means color `X` at alpha `NN/100`, written `theme::hexa(0x1e293b, 0.5)` in code. For example `slate-800/50` = `rgba(30,41,59,0.5)`, `surface/80` = `rgba(16,24,40,0.8)`, `muted/30` = `rgba(29,41,57,0.3)`, `background/50` = `rgba(11,18,33,0.5)`, `white/1` = `rgba(255,255,255,0.01)`.

Type helpers (`KoreStyled`): `text_9/10/11()` set size only (line height inherits); `text_xs_tw()` 12/16, `text_sm_tw()` 14/20, `text_base_tw()` 16/24, `text_lg_tw()` 18/28. Larger one-offs (36/40 score) use `text_size` + `line_height`. Weights: medium 500, semibold 600, bold 700. Letter-spaced uppercase labels use `ui::text::caps(text, size)` (uppercase + 0.05em) or `tracked(text, size, 0.025)`. "leading 1.625" means line height = 1.625 × font size.

All spacing below is in px. Radius values: 2, 4, 6, 8, 12, 16, full (pill).

Shadows: `shadow-lg` = `0 10px 15px -3px` + `0 4px 6px -4px`, each in black/10. `shadow-xl` = `0 20px 25px -5px` + `0 8px 10px -6px`, black/10. `shadow-2xl` = `0 25px 50px -12px` black/25. A tinted shadow uses the same geometry with the given color.

Mono text that must line up column-for-column is drawn at a 0.601em pitch (`theme::MONO_ADVANCE_EM`) by `ui::mono_text::{mono_text, mono_paragraph}`. The graph canvas and a few chips instead scale mono font sizes by 0.975 (`graph.rs::MONO_FIX`) so their widths match.

No full-window noise overlay is drawn.

### 0.2 Shared style primitives

- **`.glass()`** (`KoreStyled::glass`): background `rgba(16,24,40,0.7)` (`GLASS_BG`), `1px` border `rgba(255,255,255,0.06)` (`GLASS_BORDER`), shadow `0 0 0 1px white/2` + `0 4px 24px black/30`. It **replaces** any background, border and shadow on the same element, so a "glass card with a slate-800 border" renders the white/6 border. There is no backdrop blur anywhere in the app; overlays use plain tints.
- **Skeleton bar** (`ui::widgets::skeleton_bar(id, width, height)`): radius 4, base `slate-800/40`, a `slate-700/40` gradient band sweeping left to right every 1.8s (ease-in-out, repeating).
- **Focus ring**: a focused text field gets a 2px ring in accent at 40% alpha drawn 2–3px outside its box (`views::ai_chat::focus_ring`, `views::rbac::focus_ring`); the field keeps its resting corner radius (8 or 12px) and the ring is rounded to match. Buttons show no focus ring.
- **Scrollbars** (`ui::scrollbar`): 6px wide, transparent track, thumb `#334155` with radius 3, hover `#475569`. They take layout space: content reserves 6px while the area overflows.
- **Spinners**: `ui::widgets::spinning_icon(id, "loader-2", size, color)`, 360° per 1s, linear, repeating.
- **Ping** (`ui::widgets::ping`): a same-size dot behind a status dot that scales 1→2 while fading from 75% to 0 over the first 75% of a 1s repeating cycle.
- Icons are Lucide SVGs (`ui::icon::icon(name, size)`); the color is always set on the icon itself. Hover color changes on icons use a group hover on the parent.

### 0.3 Accent tints (rendered values)

The app's look intentionally has **no accent-tinted fills** and uses a light-gray border where an "accent-tinted" border would be expected:

1. Tiles and pills that would carry an accent tint (avatar tiles, header icon tiles, count pills, selected segmented-control buttons, user chat bubbles) have **no background**. Only the icon or text color shows.
2. Icons that would be "accent at 60%" are drawn slate-100 `#f1f5f9`.
3. Buttons and cards whose only border color would be an accent tint get a 1px **`PREFLIGHT_BORDER` `#e5e7eb`** border: the enabled AI send buttons, the idle/testing AI Settings "Test Connection" button, the RBAC "Check" and "Enter Impersonation Mode" buttons, the AI Chat tool-status pill, and the selected Network Policy card.
4. There are no accent hover or focus border effects; the resting border stays.

**[FIXED]** (quirk 10.2) The 32px header tiles of the AI Chat view and the AI Panel draw their `Bot` icon at **18px** in accent, and the empty-state `Bot` icons (AI Chat 32px, AI Panel and Network Policies 28px) are **accent at 60%** (they used to be 24px and slate-100).

The RBAC role-browser loading placeholders are static bars (no shimmer).

### 0.4 Host container (`root.rs`)

- The shell is the 256px sidebar (`views::sidebar::WIDTH`) plus the main column. Main is a column: an optional **RBAC impersonation banner** (section 7.9), then a padded area (`padding 16`, column, `gap 12`) holding an auto-height first row and the routed view (flex 1, relative, overflow hidden).
- None of the views in this spec show the resource-table header (it is shown only for the table and details views). An empty element occupies the first row, so **every view's top edge sits 16 + 12 = 28px below main's top, with 16px left, right and bottom insets**.
- Views switch instantly. Each routed view is created when its route is entered and dropped when it is left, so its state resets on re-entry. The AI Panel overlay is recreated on every open, but its conversation lives in an app-global session (6.6).
- `namespace == "*"` (All namespaces) is passed to the backend as `None`.
- Navigation entry points:
  - Sidebar "Views" list labels: **AI Chat, Dashboard, Graph, Network Policies, RBAC, Custom Resources, Helm**.
  - Command palette view entries: **AI Chat, Dashboard, Resource Graph, Custom Resources, Helm Releases, RBAC Simulator, Settings**. There is no Network Policies entry.
  - The palette action **"Ask AI"** opens the AI Panel.
  - **Cmd/Ctrl+Shift+R** opens RBAC from anywhere (outside text entry; Fix 17 in `root.rs`).
- The default start view is **`chat`** (AI Chat), unless the persisted `kore-settings` (`state/settings.rs`) has another `default_view`.
- Navigation calls (`AppState`):
  - Dashboard row click: `open_resource(Some(Pods), ResourceItem { name, namespace })`, which opens the Pod details view.
  - Graph node click maps the kind with `{Pod→pods, Deployment→deployments, Service→services, Ingress→ingresses, Job→jobs, CronJob→cronjobs}` (unknown kinds map to pods), then `open_resource` (`graph_layout::click_target`). **[FIXED]** (quirk 10.11) ReplicaSets have no details view in the app, so a ReplicaSet click opens the **Deployment that owns it** (the source of the `owns` edge into the RS, e.g. `web-7d9f8c` → Deployment `web`). A ReplicaSet without an owning Deployment opens nothing and shows the info toast `ReplicaSet {name} has no owning Deployment to open`.
  - Helm row select: `open_helm_release(release)` (helm-detail). Helm detail Back / Esc: `back()` returns to `helm`.
  - CRD Back and RBAC Back return to `table`.

### 0.5 Shared components

**Kbd (CRD and Helm variant):** `ui::widgets::kbd(text)` with line height 20 inside the 14px Back buttons: `px 6, py 2`, bg `slate-800/80`, radius 4, 1px border `slate-700/50`, 10px mono slate-400, margin-left 6.

**Kbd (RBAC variant):** `rbac::rbac_kbd(key)`: centered, min-width 24, height 20, `px 4`, bg `slate-800/80`, radius 4, 10px mono slate-400, 1px border `slate-700/50`, shadow `0 1px 0 black/30`.

**ConfirmDialog** (`ui/confirm_dialog.rs`, opened with `AppState::confirm(ConfirmRequest { … })`)
- Backdrop: covers the window, `rgba(0,0,0,.6)`, no blur. Clicking it does nothing; Esc does nothing. Only the X, Cancel and the confirm action close it (the confirm handler calls `close_confirm` itself when its work succeeds).
- Panel: centered with 16px outer padding, full width up to 448px, 1px variant border, radius 8, shadow-xl, padding 24.
  - Variant border: danger `red-500/50`, warning `yellow-500/50` (`#eab308`), info `blue-500/50`.
  - Background: danger and info `#101828` (surface). Warning `#1b180d` (an opaque dark yellow-tinted panel; the yellow tint composited over the dimmed app).
  - Top row (gap 16, margin-bottom 16, top-aligned): icon circle (padding 8, pill, bg `red|yellow|blue-500/20`) with `alert-triangle` 24px in red-400 / yellow-400 `#facc15` / blue-400; then the title (18/28 semibold slate-100, margin-bottom 8) over the message (14px slate-300); then the close `x` 20px slate-400 (hover slate-200).
  - Footer: margin-top 24, right-aligned, gap 12.
    - Cancel: `px 16, py 8`, radius 4, 1px border slate-800, 14px slate-100; hover border slate-700 and bg `muted/40`.
    - Confirm: same box, white text. Danger bg `#dc2626` (hover `#b91c1c`), border red-500; warning bg `#ca8a04` (hover `#a16207`), border `#eab308`; info bg `#2563eb` (hover `#1d4ed8`), border blue-500.
- Animation: 250ms ease-out-quint, opacity 0→1 and y offset 20→0.

**Toast** (`ui/toast.rs`, `toast(cx, message, ToastKind)`): bottom-right at 16px, column, right-aligned, gap 8, above everything.
- Each toast: glass (so its border is white/6 whatever the type), radius 8, `px 16, py 10`, gap 10, 14px slate-200, max-width 320 (text shrink-wraps, wrapping at 236px).
- Icon 16px: success `check-circle-2` emerald-400; error `x-circle` red-400; info `info` accent. Trailing `x` 14px slate-500.
- Auto-dismiss after 3000ms; click dismisses.
- Enter: 200ms ease-out-quint, opacity 0→1 with y 16→0. No exit animation.

**Animation conventions used below:** "fade" = opacity 0→1 over 300ms. "fade-up N" = opacity 0→1 while the element moves up N px into place. Durations and easings are given where they differ.

---

## 1. Cluster Dashboard (`view = "dashboard"`, `views/dashboard.rs`)

### 1.1 Data
- Single cluster: `Core::get_cluster_health()` → `ClusterHealth`.
- Multi-cluster (sidebar Multi-Cluster toggle on, `AppState::multi_cluster`): `Core::get_cluster_health_multi_cluster()` → `MultiClusterHealth { clusters: [ClusterHealthEntry { context, health }] }`. One entry per kubeconfig context whose client could be built and that answered in time. **[FIXED] Deliberate Fix 9:** every context runs in parallel under a **5s timeout** (`constants::MULTI_CLUSTER_CONTEXT_TIMEOUT`); a context that fails to build a client or times out is dropped silently, so one unreachable context never holds up the others. Each context lists at most 1000 objects per kind (`MAX_DASHBOARD_RESOURCES`); list failures yield empty lists.
- Polling: fetch on view creation (and whenever `multi_cluster` changes; the poll restarts), then every **30s**. There is no manual refresh button.
- `ClusterHealth = { score: u32, pods: {running, pending, failed, succeeded, crash_looping, total}, nodes: [{name, status, cpu_capacity, memory_capacity, cpu_allocatable, memory_allocatable, cpu_usage: Option, memory_usage: Option}], restart_hotlist: [{name, namespace, restarts, status}], pending_pods: [{name, namespace, reason, age}], recent_warnings: [{reason, message, involved_object, namespace, count, last_seen}] }`.
- Node metrics: alongside pods, nodes and events, the backend lists `metrics.k8s.io/v1beta1` **NodeMetrics** (`fetch_node_metrics`, also per context in multi-cluster mode). If metrics-server is missing or the call fails, every node's usage is `None` (logged, not an error).
- Backend semantics (`kore-core state/dashboard.rs::compute_health_with_metrics`), with pods, nodes and events listed across all namespaces (single-cluster list errors are logged and treated as empty):
  - A pod is **crash_looping** if any `containerStatuses[].state.waiting.reason == "CrashLoopBackOff"`. Such pods are not counted under their phase. Other pods are bucketed by `status.phase`: Running, Pending, Failed or Succeeded. Unknown phases are ignored and not counted in total. `total = running + pending + failed + succeeded + crash_looping`.
  - **pending_pods**: phase Pending. `reason` is the `reason` of the first `status.conditions[]` entry with `status=="False"` (for example `Unschedulable`), else `"Unknown"`. `age` carries `metadata.creationTimestamp` (RFC 3339); **[FIXED]** (quirk 10.12) the card renders it as a relative age (1.8).
  - **restart_hotlist**: pods whose summed `containerStatuses[].restartCount` is > 0, sorted descending, truncated to 10.
  - **nodes**: `status` from the `Ready` condition: `Ready` / `NotReady` / `Unknown`. `cpu_capacity`/`memory_capacity` come from `status.capacity` and `cpu_allocatable`/`memory_allocatable` from `status.allocatable` (a missing value becomes `"0"`). **[FIXED]** (quirk 10.12) `cpu_usage`/`memory_usage` are the live NodeMetrics `usage.cpu`/`usage.memory` of the node with the same name (they used to be the allocatable values), or `None` when there are no metrics for it.
  - **recent_warnings**: **[FIXED]** the 20 most recently seen events with `type=="Warning"` (sorted by `last_seen` descending, stable; they used to be the first 20 in list order). `involved_object = "<Kind>/<name>"`. `last_seen = lastTimestamp || eventTime || series.lastObservedTime || metadata.creationTimestamp` (RFC 3339); **[FIXED]** (quirk 10.12) the card renders it relative (1.9). `count` defaults to 1.
  - **score** = clamp(`ready_nodes/max(nodes,1)*30` + `running/max(total,1)*40` + 30 − min(crash*5,15) − min(pending*2,10) − min(warnings*0.5,5), 0, 100), truncated to an integer. Empty cluster = 30.

### 1.2 States
- **Loading** (no data yet): centered row, gap 8: `loader-2` 20px spinning slate-400 + "Loading cluster health..." in slate-400 at the inherited 16px size.
- **Error** (only if no data yet): centered column, slate-400.
  - `alert-triangle` 48px in `amber-400/50`, margin-bottom 16.
  - The formatted error (`backend::format_error`) at 18/28, centered.
  - "Retry" with `refresh-cw` 14px: margin-top 16, 14px accent, row gap 4. No hover change. Click refetches.
  - Errors after a successful load are kept internally but not shown; the stale data stays.
- **Single cluster**: a vertical scroll area filling the view, padding 16 (inside the 16px host inset), with a 6px scrollbar. The card grid has 1 column below a 768px **window** width, 2 columns from 768px, 3 from 1280px. Gap 16 between rows and cards. Cards in a row share the row's height (they stretch). The whole content fades in (300ms).
  - 3 columns: cards 1–3, then 4–6.
  - 2 columns: [1, 2], [3, 4], [5, empty], then Recent Warnings alone spanning the full width.
  - 1 column: one card per row.
- **Multi-cluster** (shown only when every entry has a context): the same scroll area, sections spaced 24. Per context:
  - Header row (gap 12, margin-bottom 12): the context name, 16/24 semibold slate-200 mono.
  - Score pill: 12px mono semibold, `px 8, py 2`, pill, 1px border, text `"{score} - {Healthy|Degraded|Critical}"`. Colors (score ≥80 / ≥50 / <50): bg `emerald|amber|red-500/15`, text `-400`, border `-500/30`.
  - Below it, the same 6-card grid.

### 1.3 Card chrome (`dashboard.rs::card`)
- Glass, radius 8, padding 16, column. The border is therefore white/6.
- Enter animation: fade-up 12, 300ms ease-out-quad.
- Header row (gap 8, margin-bottom 12): Lucide icon 16px accent; title 14px semibold slate-200, **UPPERCASE**, tracking 0.025em.
- Body: flex 1. Empty-state messages are centered both ways in the stretched body, 14px slate-500.

Cards in order: **Cluster Health** (`heart`), **Pod Health** (`activity`), **Node Status** (`server`), **Restart Hotlist** (`refresh-cw`), **Pending Pods** (`clock`), **Recent Warnings** (`skull`). Automation ids `dashboard.card.<title-kebab>`. Text ids (labels = displayed text): `dashboard.score.<key>`, `dashboard.score-label.<key>`, `dashboard.summary.<key>`, `dashboard.breakdown.<key>.<i>|total|note`, `dashboard.pod-legend.<key>.<name|Total>`, `dashboard.node.<node>` (status) and `.cpu`/`.mem` (`"{usage} / {allocatable}"`), `dashboard.pending.<ns>/<name>.age|reason`, `dashboard.warning.<i>` (reason) and `.object`/`.seen`.

### 1.4 Cluster Health card
- Column, centered, vertical padding 16.
- Score ring: 112×112 circle with a 4px border colored by score (≥80 / ≥50 / else): `emerald-500/30`, `amber-500/30`, `red-500/30`. shadow-lg tinted with the same hue at 10%. Transparent fill.
- Inside: the score, 36/40 bold mono, emerald-400 / amber-400 / red-400.
- Label (margin-top 12, 14px medium, same color): "Healthy" (≥80), "Degraded" (≥50), "Critical".
- "{total} pods across {nodes.length} nodes": 12px slate-500, margin-top 4.
- Toggle (margin-top 8, 10px slate-500, hover slate-300, gap 4, `info` 12px): "How is this calculated?" / "Hide breakdown". Automation id `dashboard.breakdown-toggle.<key>` (label = the shown text).
- Breakdown (appears with a 200ms opacity fade; no height animation; disappears instantly):
  - Container: margin-top 12, padding-top 12, top border `slate-800/60`, rows gap 6, 11px mono, line height 16.5.
  - Row: label (slate-400, flex 1), optional detail (slate-600), value 48px wide right-aligned, medium. Positive values `emerald-400/80`, negative `red-400/80`.
  - Rows (values with one decimal, `toFixed(1)` rounding):
    1. "Node health": "{ready}/{n} ready", "+{x.x}"
    2. "Pod health": "{running}/{total} running", "+{x.x}"
    3. "Base score": "+30.0"
    4. Only if the penalty > 0: "CrashLoop penalty" "{n} pod(s)" "-x.x"; "Pending penalty" "{n} pod(s)"; "Warning penalty" "{n} event(s)". Singular when n == 1.
  - Total row: padding-top 6, top border `slate-800/40`. "Total" slate-300 medium; the score semibold in the score color.
  - Footnote: 10px slate-600, line height 16.25, padding-top 4. Exact text: "Score is clamped to 0-100. Node & pod ratios scale up to 30 and 40 points respectively. Penalties are capped at -15 (crash), -10 (pending), -5 (warnings)."

### 1.5 Pod Health card (donut)
- Empty (no non-zero buckets): "No pods found".
- Otherwise a row, gap 16: a 160×160 chart painted on a canvas, then the legend.
- Data = [Running `#10b981`, Pending `#f59e0b`, Failed `#ef4444`, CrashLooping `#a855f7`, Succeeded `#64748b`], filtered to value > 0, in that order (`dashboard.rs::pod_data`).
- Geometry (`pie_sectors`): center (80,80), inner radius 35, outer radius 60, no stroke. Angles start at 0° (3 o'clock) and run **counter-clockwise** to 360°, with a 2° padding after every non-zero sector including the wrap-around, so a single sector covers 358°. No labels.
- Entry animation: starts 400ms after the first data arrives, lasts 1500ms with CSS `ease` (`cubic-bezier(.25,.1,.25,1)`); the sectors grow one after another from the first sector's start. It plays once per view (30s polls do not re-animate).
- Tooltip on sector hover (hit test on the 35–60 ring): anchored at the sector's mid-angle on the mid radius (47.5), offset +10px right and down; no position transition.
  - Box: bg surface, 1px border slate-700, radius 4, `px 12, py 8`, 12px, shadow-lg (black/10), no wrap.
  - Content: 8px dot in the sector color (margin-right 8), "{name}: " slate-300, value slate-100 semibold.
- Legend (column, gap 6, 12px), one row per entry, gap 8: 10px dot; name slate-400; value slate-200 mono, right-aligned.
- Then a Total row: top border slate-800, padding-top 4, margin-top 4. "Total" slate-500; the total slate-300 mono, right-aligned.

### 1.6 Node Status card
- Empty: "No node data".
- List: max-height 192, scrolls (6px scrollbar), gap 12 between nodes; each node is a column with gap 4.
- Per node:
  - Header row (12px, space-between): name mono slate-300, truncated at 62% width; status (medium) colored Ready `#10b981`, NotReady `#ef4444`, other `#f59e0b`.
  - Two bar rows ("CPU", "Mem"), 10px slate-500, line height 15, gap 8:
    - Label, 32px wide.
    - Track: flex 1, height 6, slate-800, pill, clipped.
    - **[FIXED]** (quirk 10.12) The bars show **live usage (metrics-server) against allocatable** (they used to show allocatable against capacity).
    - Fill: width = `usage_percent(usage, allocatable)`%. Kube quantity parsing (`parse_kube_quantity`): `Ki/Mi/Gi/Ti` binary, `m` = /1000, `n` = /1e9, else the numeric prefix; result `round(u/c*100)` (half up) capped at 100, 0 if either is missing or the allocatable is 0. Pill, in the **status color**. No width transition. Without node metrics the track stays empty.
    - Value text: 96px wide, right-aligned, slate-400: "{usage} / {allocatable}", both humanized: CPU (`format_cpu`) as millicores below one core ("108m"), else cores with at most one decimal ("2", "3.8"); memory (`format_memory`) in binary units ("512Mi", "1.7Gi", "13.6Gi"). Unknown usage (no metrics-server, e.g. `kind-kore-harness-2`) shows "?" ("? / 12").

### 1.7 Restart Hotlist card
- Empty: "No restarts detected".
- List: max-height 192, scrolls, gap 4.
- Each row is clickable: full width, row gap 8, `px 8, py 6`, radius 4, 12px, hover bg `slate-800/60`. Contents:
  - Rank "1", "2", …: mono slate-500, 16px wide, right-aligned (no trailing period).
  - Name: mono slate-300, truncated, flex 1, turns accent on row hover.
  - Namespace: 10px slate-500, max 80px, truncated.
  - Restarts: mono semibold. ≥10 red-400, ≥5 amber-400, else slate-400.
- Click opens the Pod details view (`dashboard.hotlist.<ns>/<name>`).

### 1.8 Pending Pods card
- Empty: "No pending pods".
- List: max-height 192, scrolls, gap 8.
- Each row is clickable, `px 8, py 6`, radius 4, 12px, hover bg `slate-800/60`:
  - Line 1 (space-between): name (mono slate-300, truncated, accent on row hover) and age (10px slate-600, margin-left 8): **[FIXED]** relative like the resource tables (`format_relative_age`: "12m", "3h", "2d"; "-" if unknown).
  - Line 2 (margin-top 2, gap 4): reason (10px `amber-400/70`, "Unknown" if empty, truncated); namespace (10px slate-600, right-aligned, max 80px).
- Click opens the Pod details view (`dashboard.pending.<ns>/<name>`).

### 1.9 Recent Warnings card
- Empty: "No recent warnings".
- List: max-height 192, scrolls, gap 8.
- Each item: `px 8, py 6`, radius 4, 1px border `amber-500/10`, bg `amber-500/5`, 12px.
  - Row 1 (gap 6): `alert-triangle` 12px amber-400; reason amber-300 medium, truncated; if count > 1, "x{count}" in 10px mono slate-500, right-aligned.
  - Message: slate-400, margin-top 2, **clamped to 2 lines** with an ellipsis, leading 1.625.
  - Footer (margin-top 4, 10px slate-600, line height 15, space-between): `involved_object` (truncated at 60%) and `last_seen`: **[FIXED]** relative with "ago" (`last_seen_label`: "3m ago"; "-" if unknown).
- Not clickable.

---

## 2. Resource Graph (`view = "graph"`, `views/graph.rs`, `views/graph_layout.rs`)

### 2.1 Data
- `Core::build_resource_graph(namespace: Option<String>)`, with `None` meaning all namespaces.
- Fetched on view creation and when the namespace changes (the view returns to the loading state while refetching). **No polling, no watch.**
- `ResourceGraph = {nodes: [{id, name, namespace, kind, status}], edges: [{source, target, relation}]}`. Node id is `"{Kind}/{ns}/{name}"`.
- Backend node emission order (`kore-core state/graph.rs`; it matters for layout tie-breaks): **Pods, Deployments, ReplicaSets (only those with `status.replicas > 0`), Services, Ingresses, Jobs, CronJobs**. List errors yield no nodes of that kind.
- Statuses:
  - Pod: `status.phase`, else "Unknown".
  - Deployment: "Ready" if `readyReplicas == replicas > 0`, else "Progressing".
  - ReplicaSet: "Active".
  - Service: the spec type ("ClusterIP", "NodePort", …; default "ClusterIP").
  - Ingress: "Active".
  - Job: the type of the first condition with status True (for example "Complete" or "Failed"), else "Running".
  - CronJob: "Active".
- Edges:
  - Deployment→RS (`owns`, via RS ownerRefs).
  - Pod owner→Pod (`owns`, for **every** pod ownerReference kind, for example ReplicaSet, Job, DaemonSet, StatefulSet, Node). The source may not exist as a node.
  - Service→Pod (`selects`: all selector labels match and same namespace).
  - Ingress→Service (`routes`, from `spec.rules[].http.paths[].backend.service.name`).
  - CronJob→Job (`owns`).
- The edge count shown in the UI includes edges whose endpoint node is missing. Such edges are not drawn.

### 2.2 Layout algorithm (`graph_layout.rs::compute_layout`, unit-tested)

Constants: `NODE_WIDTH=180`, `NODE_HEIGHT=56`, `LAYER_GAP_Y=100`, `NODE_GAP_X=40`, `PADDING=60`.

Layer by kind (`layer_for_kind`): `Ingress 0, Service 1, Deployment 2, CronJob 2, ReplicaSet 3, Job 3, Pod 4`. Any other kind defaults to 2.

```
layers = group nodes by layer (preserving input order within each layer)
keys = sorted ascending distinct layer numbers present (absent layers collapse; row index li = index in keys)
parentMap[target] = list of edge.source for every edge (all relations)
maxLayerWidth = max over layers of n*180 + (n-1)*40
totalWidth = maxLayerWidth + 120
layoutNodes = []
for li, key in keys:
   pos = { id -> x } for nodes already placed (earlier rows only)
   score(node) = average of pos[p] over parents p of node that are already placed;
                 0 if none placed / no parents
   stable-sort layer nodes by score ascending (ties keep input order)
   layerWidth = n*180 + (n-1)*40
   startX = (totalWidth - layerWidth)/2
   y = 60 + li*(56+100)
   x_i = startX + i*(180+40)
totalHeight = 120 + L*56 + (L-1)*100
```

x and y are the node's top-left corner. Scores use the parents' left-edge x.

Edge curve (`edge_curve`), from the bottom-center of the source to the top-center of the target: a cubic Bézier `(sx,sy) → ctrl (sx,midY), (tx,midY) → (tx,ty)`, where `sx = src.x+90`, `sy = src.y+56`, `tx = tgt.x+90`, `ty = tgt.y`, `midY = (sy+ty)/2`.

### 2.3 Colors (`kind_color`, `status_color`)

| Kind | fill | stroke | text |
|---|---|---|---|
| Deployment | `#1e3a5f` | `#3b82f6` | `#93c5fd` |
| Service | `#2d1b4e` | `#a855f7` | `#d8b4fe` |
| Pod | `#1a3a2a` | `#10b981` | `#6ee7b7` |
| Ingress | `#0c3642` | `#22d3ee` | `#67e8f9` |
| Job | `#3b2a0a` | `#f97316` | `#fdba74` |
| CronJob | `#3b2a0a` | `#f97316` | `#fdba74` |
| ReplicaSet / default | `#1e293b` | `#64748b` | `#94a3b8` |

Status border color, case-insensitive:
- running, ready, active, succeeded, complete, completed, available → `#10b981`
- pending, containercreating, init, waiting → `#f59e0b`
- failed, crashloopbackoff, error, imagepullbackoff, evicted, oomkilled → `#ef4444`
- anything else → `#64748b` (for example Service "ClusterIP", Deployment "Progressing", Pod "Unknown")

### 2.4 Canvas
- The canvas fills the view (relative, clipped), with no border and no radius. The view fades in (300ms). Everything is painted in `paint_graph` on one canvas.
- Background: a radial gradient from `#0f1a2e` at the center to `#0b1221` at the farthest corner (painted as 64 concentric discs).
- Grid: one dot per 40×40 graph-unit cell, centered at (20,20) of the cell, color `#1e293b`, diameter `max(scale, 0.5)`px. The grid pans and zooms with the content. It is not drawn when a cell is smaller than 8px on screen (scale < 0.2).
- Cursor: open hand, closed hand while panning, pointing hand over a node.
- Content transform: `screen = (tx + x*s, ty + y*s)`. Initial `{0,0,1}`.

### 2.5 Node rendering (graph coordinates, scaled by `s`)
- Hover glow (hovered node only): a 1px outline in the status color at 40% on the rectangle `(x-3, y-3, 186, 62)`, radius 12, no fill.
- Body: rect `(x, y, 180, 56)`, radius 10, fill = kind fill, outline = status color, outline width 1.5 (hovered: 2), whole body at opacity .9 (hovered: 1). The outline is centered on the rect edge.
- Status dot: circle at `(x+168, y+12)`, radius 3.5, status color.
- Kind icon: Lucide 14×14 with stroke width 1.5, kind text color, at `(x+11, y+9)` (centered in a 16×16 box at `(x+10, y+8)`). Icons: Pod `box`, Service `network`, Deployment `container`, Ingress `globe`, ReplicaSet `layers`, Job `timer`, CronJob `clock`, other `server`.
- Kind label: baseline at `(x+30, y+19)`, kind text color at 80%, mono medium at 9×0.975px, e.g. "Deployment".
- Name: baseline at `(x+10, y+40)`, `#e2e8f0`, mono medium at 11×0.975px. Names longer than 20 chars show the first 18 chars + "..." (`display_name`).
- Node opacity (2.7) multiplies every part.

### 2.6 Edge rendering
- Default: stroke `#334155`, width 1, opacity .5. Dashed (4 on, 3 off, scaled by stroke width) unless the relation is `owns` (so `selects` and `routes` are dashed).
- Arrowhead at the target end: a filled triangle pointing down with its tip 1 stroke-width below the target top, 6 wide and 8 long in stroke-width units, in the edge color at opacity .6 × the edge opacity. A highlighted 2px edge therefore has a 2× larger arrowhead.
- Highlighted (a node is hovered, and **both** endpoints are in {hovered} ∪ neighbors(hovered)): stroke accent, width 2, opacity .9, arrowhead accent at .8 × edge opacity. Edges between two neighbors of the hovered node also highlight.
- Hover-dimmed (a node is hovered, edge not highlighted): opacity .15.
- Filter-dimmed (a filter is active and either endpoint is "hidden"): opacity .05. Filter dimming takes precedence over hover dimming.
- Changes apply immediately (no transition).

### 2.7 Hover
- The hovered node is the topmost node under the pointer, updated on real mouse movement only (zooming a node under a still pointer does not change hover). Leaving the canvas clears it.
- Node opacity:
  1. Base 1.
  2. If a filter is active: "hidden" → .08, "connected" → .6.
  3. If a node is hovered and this node is neither it nor a neighbor (and not filter-hidden): min(current, .3).
- **Hover tooltip** **[FIXED]** (quirk 10.21; the original design's tooltip was never visible as rendered): while a node is hovered a tooltip is drawn over the canvas (`GraphView::render_tooltip`, automation id `graph.tooltip`, label = node name; `graph.tooltip.kind` = "{kind} {status}", `graph.tooltip.ns` = "ns: {namespace}"). It is pointer-transparent.
  - Box: glass, radius 8, `px 12, py 8`, 12px, max-width 240 (content-sized below that).
  - Row 1: the full name, mono medium slate-100 at the 0.601em pitch, broken anywhere (CSS `break-all`, `graph_layout::break_all`), margin-bottom 4.
  - Row 2 (gap 12): the kind in slate-400, then the status (or "Unknown") medium in the status color (2.3).
  - Row 3 (when the node has a namespace): `ns: {namespace}`, slate-500, margin-top 2.
  - Placement (`graph_layout::tooltip_origin`): centered on the node horizontally, its bottom 8px above the node's top edge, in screen coordinates (it follows zoom and pan). If it would come within 4px of the canvas top it flips below the node (8px under its bottom edge). It is kept 4px inside the canvas horizontally.
  - Enter: 150ms, opacity 0→1 with y +4→0, per hovered node. It disappears instantly when hover clears.

### 2.8 Interaction
- **Wheel zoom** (`zoom_at`): factor 0.92 when scrolling down (zoom out), else 1.08. Scale clamped to [0.15, 4]. Zoom is anchored at the cursor: `newTx = mx - (mx - tx)*k`, same for y, with `k = newS/s`. A purely horizontal scroll counts as "not down" and zooms in.
- **Pan:** left mouse-down anywhere on the canvas (including on nodes) starts the pan. Moving with the button held sets `t = start + delta`; mouse-up, releasing the button, or leaving the canvas ends it.
- **Click** on a node: mouse-down and mouse-up on the same node opens it (see 0.4). **[FIXED]** A pan whose pointer moved more than 4px between mouse-down and mouse-up (`graph_layout::is_drag`) is not a click, even when it starts and ends on the same node.
- **Fit** (`fit`): `s = min((W-40)/graphW, (H-40)/graphH, 1.5)`, `tx = (W - graphW*s)/2`, `ty = (H - graphH*s)/2`. It runs automatically 50ms after a load whose node count differs from the previous load (and is > 0), and on the Fit button. It does not run on container resize.

### 2.9 Overlays (all absolutely positioned above the canvas; clicks on them do not reach the canvas)
Overlay boxes share `overlay_box()`: bg `surface/80`, 1px border slate-800, radius 8, no blur.
- **Count badge** (top 12, left 12): overlay box, `px 8, py 4`, 10×0.975px mono slate-500, line height 15. Text `"{nodes} nodes · {edges} edges"` (U+00B7). Automation id `graph.count` (label = the text).
- **Toolbar** (top 12, right 12, row gap 6):
  - Fit button: overlay box, `px 10, py 6`, 12px slate-300, `maximize-2` 14px + "Fit", gap 6. Hover bg `muted/30`. Automation id `graph.fit`.
  - Zoom label: `"{round(s*100)}%"`, 10×0.975px mono slate-600, `px 6`. Automation id `graph.zoom` (label = the text). Node ids `graph.node.<Kind>/<ns>/<name>` are `disabled` while the kind/name filter dims them (anything but a direct match).
- **Filter bar** (top 46, left 12, row gap 8):
  - Kind chips: overlay box, `px 8, py 6`, gap 4. One chip per kind present, sorted by layer (stable, `unique_kinds`). Each chip: `px 6, py 2`, radius 4, 10×0.975px mono, line height 15, gap 4, 1px border, with a 6px dot.
    - Enabled: bg = kind fill, border = kind stroke, text = kind text, dot = stroke.
    - Disabled: no bg, border `#334155`, text `#475569`, dot `#475569`, chip opacity .5.
    - Click toggles. Automation id `graph.chip.<Kind>` (`selected` = enabled).
  - Name filter: 160px wide `TextInput` with placeholder "Filter by name..." (slate-600), 11×0.975px mono slate-300, line height 16.5, padding left/right 24 and top/bottom 4, in a box with bg `surface/80`, 1px border slate-800, radius 8 (4 plus the focus ring while focused). `search` 12px slate-500 at left 8.
  - When the query is non-empty: an `x` clear button at right 6 (padding 2, 12px icon, slate-500, hover slate-300) that empties the query.
- **Legend** (bottom 12, left 12): overlay box, `px 12, py 8`, row gap 12. It **always lists all 7 kinds** (`LEGEND_KINDS`), in order Deployment, Service, Pod, Ingress, Job, CronJob, ReplicaSet: a 10px square (radius 2) in the stroke color, gap 6, then the label in 10px slate-400.

### 2.10 Filtering (`graph_layout.rs::visibility`)
- Active when any kind is disabled or the query is non-empty.
- **direct** = kind enabled AND (query empty OR `name.to_lowercase().contains(query.to_lowercase())`).
- **connected** = not direct, but 1 hop from a direct node via any edge.
- **hidden** = everything else.
- The layout never changes. Filter state resets when the view is recreated.

### 2.11 States
- **Loading**: centered column, gap 12: `loader-2` 32px accent spinning; "Building resource graph..." 14px slate-400.
- **Error**: centered column, gap 12, red-400: "Failed to load resource graph" 14px medium; the error at 12px slate-500, max-width 448, centered.
- **Empty** (no nodes): `inbox` 48px slate-600; "No resources found" 14px medium slate-400; then 12px slate-500: `No resources in namespace "{ns}"`, or "Select a namespace to view the resource graph" when All namespaces is selected.

---

## 3. CRD Browser (`view = "crds"`, `views/crd_browser.rs`)

### 3.1 Data
- `Core::list_crds()` → `[CrdInfo {name, group, version, kind, scope: "Namespaced"|"Cluster", plural}]`. `version` = the first served version. Fetched once when the view is created.
- `Core::list_crd_instances(group, version, plural, namespace: Option<String>)`. The namespace is the current namespace for Namespaced CRDs (`None` = all), and always `None` for Cluster CRDs. Refetched when the selected CRD or the namespace changes (this also closes an open instance detail).
- `Core::get_crd_instance(group, version, plural, namespace: String, name)` and `Core::delete_crd_instance(group, version, plural, namespace: String, name)`. **[FIXED]** (quirk 10.10) The namespace is `kore_core::state::crd::instance_namespace(crd.scope, instance namespace)`: `""` (cluster scope) for a `Cluster` CRD whatever the instance or the table cell says, else the instance's `metadata.namespace` (`"default"` if missing). The backend addresses an empty namespace (or the table placeholder `"-"`) cluster-wide. Cluster-scoped instances therefore open and delete (they used to send `"default"` / `"-"` and fail).

### 3.2 Layout
- Root: full-size column, bg background. Enter: 300ms, opacity 0→1 while sliding from 20px right. No exit animation.
- **Header**: padding 16, bottom border slate-800, bg `surface/50`. Row, space-between:
  - Back button: `px 12, py 6`, radius 6, 1px border slate-800, 14px slate-300, gap 8, `arrow-left` 16px, "Back", Kbd "Esc". Hover bg `muted/30`. Automation id `crd.back`.
  - Title block (gap 10): 28×28 tile (radius 8, no fill) with `puzzle` 16px accent; "Custom Resources" 14px semibold slate-100 over "{n} CRD(s) discovered" 10px slate-500, line height 15 (singular when n == 1).
  - An 80px empty spacer on the right.
- **Left panel**: 288px wide, right border slate-800, bg `surface/30`, column.
  - Search area: padding 12, bottom border `slate-800/50`. Box: row gap 8, `px 10, py 6`, radius 6, 1px border slate-800, bg `background/50`. `search` 14px slate-500; `TextInput` 12/16 slate-200, placeholder "Search CRDs..." slate-600; the focus ring wraps the input while focused. Automation id `crd.search`.
  - Filter: case-insensitive substring over kind, group or name.
  - List (scrolls with a 6px scrollbar, `py 4`): groups sorted by group name (case-insensitive), items sorted by kind (case-insensitive).
    - Group header: full width, row gap 6, `px 12, py 8`, hover bg `muted/20`. Chevron 12px slate-500 (`chevron-down` expanded, `chevron-right` collapsed); group name 10px uppercase tracking .05em slate-500 medium, line height 15, clipped; item count 10px slate-600, right-aligned. Automation id `crd.group.<group>`.
    - All groups start expanded. Expanding fades the items in (150ms); collapsing is instant.
    - CRD item: full width, row gap 8, `py 6`, padding-left 28, padding-right 12, 12px.
      - Unselected: slate-300; hover bg `muted/20` and text (and icon) slate-100.
      - Selected (matched by name + group): text accent, **2px left border accent**, no fill.
      - Contents: `puzzle` 12px in the text color; kind mono, truncated; for Cluster scope a "cluster" tag (9px, line height 13.5, bg `slate-700/50`, slate-400, `px 4, py 2`, radius 4).
      - Automation id `crd.item.<crd name>`.
  - Left states:
    - Loading skeleton: padding 12, 3 groups spaced 12; each is a 12px-tall 128px bar, then (margin-left 8, gap 6) bars of 160, 144 and 176px (gap 8 between the group bar and its items).
    - Error: padding 16, 14px red-400.
    - Empty: centered column, gap 8, padding 16, slate-400: `inbox` 40px slate-600; 12px "No CRDs match your search" or "No CRDs found".
- **Right panel** (flex 1):
  - **Nothing selected**: centered column, gap 12: `puzzle` 48px slate-600; "Select a Custom Resource" 14px medium slate-400; "Choose a CRD from the left panel to view its instances" 12px slate-500.
  - **Instance list** (fades in, 300ms, on each CRD selection):
    - Header: padding 12, bottom border slate-800, bg `surface/30`, row gap 12. `puzzle` 16px accent; kind 14px semibold slate-100 over "{group}/{version} - {scope}" 10px slate-500 (line height 15). Count pill (right-aligned): 10px medium accent, `px 6, py 2`, pill, no fill. Automation id `crd.count`.
    - Body (scrolls with a 6px scrollbar) states:
      - Loading skeleton: padding 16, gap 12: a 16px-tall 192px bar, then 5 rows (gap 8; 16 between bars) of 12px-tall bars 160, 96 and 64px wide.
      - Error: padding 16, 14px red-400.
      - Empty: centered, gap 12, slate-400: `inbox` 40px slate-600; "No instances found" 12px; then 10px slate-500 `No {Kind} resources in this namespace` (a namespace is selected) or `No {Kind} resources found`.
    - Table: margin 16, glass, radius 8, clipped, 14px base. Automation id `crd.table`.
      - Header row (scrolls with the rows): bg `surface/80`, bottom border slate-800. Cells `px 12, py 8`, left-aligned, 12px medium uppercase tracking .05em slate-400. Columns: **Name, Namespace, Age**, plus an unlabeled 64px actions column.
      - Column widths follow automatic table layout: each column's natural width is the larger of its header text and its longest mono value (12px at the 0.601em pitch), +24 padding; leftover width is shared in proportion to those widths.
      - Rows: 40px tall with a 1px `slate-800/50` divider (the last row is 39px without one). Hover bg `muted/30`, pointer cursor; click opens the detail. Automation id `crd.row.<ns>/<name>`.
      - Cells: `px 12`, slate-100 mono 12px. Namespace "-" if missing. Age = relative from `creationTimestamp` (`format_relative_age`: `Nd` if days > 0, else `Nh`, `Nm`, `Ns`); "-" if missing.
      - Actions cell (right-aligned): `trash-2` 14px button (padding 4, radius 4, slate-500, hover red-400 with bg `red-500/10`; opacity .5 and inert while a delete runs). Click does not open the row; it opens the confirm dialog. Automation id `crd.delete.<ns>/<name>`.
  - **Instance detail** (fades in, 300ms):
    - Header bar: padding 12, bottom border slate-800, bg `surface/30`, row gap 8.
      - "Back to list" button: `px 10, py 4`, radius 6, 1px border slate-800, 12px slate-300, `arrow-left` 14px, gap 6, hover bg `muted/30`. Automation id `crd.back-to-list`.
      - Then "|" in 12px slate-500, then the name in 12px mono accent ("unknown" if missing).
    - Content box: margin 16, flex 1, scrolls vertically (6px scrollbar), bg `surface/20`, 1px border slate-800, radius 8.
      - Loading skeleton: padding 16: a 16px-tall 128px bar; indented (margin-left 16) 12px bars 192, 224, 160; a 16px-tall 112px bar at margin-top 16; indented bars 256, 208.
      - Loaded: padding 16, mono 12px with 19px line height, wrapped (`mono_paragraph`), of the pretty-printed JSON (2-space indent, keys sorted by the backend), or "No details available", or "Error fetching instance: {msg}". Automation id `crd.detail.json`.
      - JSON highlighting (`highlight_json`) is a small tokenizer that **never changes the text**. **[FIXED]** The original regex chain rewrote `:` + whitespace to `": "` everywhere, so a string like `"x:12"` was displayed as `"x: 12"` with `12` amber.
        1. A string followed (after whitespace) by `:` is a key: accent, quotes included.
        2. The value token right after a `:`: string emerald-400; number (`-`, digits, `.`, exponent) amber-400; `true|false` indigo-400 `#818cf8`; `null` slate-500.
        Everything else, including array items and punctuation, is slate-100.
- **Delete confirm**: ConfirmDialog variant `Danger`.
  - Title "Delete {Kind}". Message `Are you sure you want to delete "{name}" in namespace "{ns}"? This action cannot be undone.` For a cluster-scoped CRD: `Are you sure you want to delete "{name}"? This action cannot be undone.`
  - Confirm label "Delete", which changes to "Deleting..." while the request runs; Cancel "Cancel".
  - Success: toast "Resource deleted" (success); the dialog closes; the row is removed locally; the detail closes if it was showing that object.
  - Failure: toast with the error text (error); the dialog **stays open** and its label returns to "Delete".

### 3.3 Keyboard
- **Esc** walks back one level: from the detail to the list; from the list to no CRD selected; from no selection to the resource table (`set_view(Table)`).
- It listens to `AppEvent::Escape`, which root emits only when no modal, dropdown, palette, overlay (AI Panel) or focused text entry is active. **[FIXED]** (Fix 4) Esc typed in the CRD search field, or pressed while the AI Panel is open, does not walk back.

---

## 4. Helm Releases (`view = "helm"`, `views/helm_releases.rs`)

### 4.1 Data
- `Core::helm_available()` (runs `helm version --short`). If true, `Core::list_helm_releases(namespace: Option<String>)`, which runs `helm list --output json` with `--namespace ns` or `--all-namespaces`, plus `--kube-context <current>` and the app's kubeconfig. Refetched when the namespace changes.
- `HelmRelease = {name, namespace, revision (string; integers are accepted too), updated, status, chart, app_version}`. `updated` is Helm's list format, for example `"2026-10-15 10:00:00.123456 +0000 UTC"`.

### 4.2 States
- **Helm not available**: centered column, gap 16, fade-up 10 (300ms ease-out-quint).
  - Circle: padding 16, pill, bg `slate-800/50`, containing `alert-circle` 48px slate-500.
  - "Helm Not Available" 14px medium slate-300, margin-bottom 4.
  - "The Helm CLI was not found on your system. Install Helm to manage releases." 12px slate-500, max-width 320, centered.
- **Loading**: a full-size glass box (radius 8), padding 16, 8 rows spaced 12. Each row: gap 16, 16px-tall skeleton bars of 128, 96, 48, 80, 112, 64px and a flex-1 bar. Row i has opacity `1 - i*0.08`.
- **Error**: centered column, gap 12: `alert-circle` 48px red-400; "Failed to load Helm releases" 14px medium red-400; the message 12px slate-500, max-width 384, centered.
- **Empty**: same circle treatment and fade-up as "not available", gap 12, with `package` 48px slate-600; "No Helm releases found" 14px medium slate-400; then 12px slate-500 `No releases in namespace "{ns}"`, or "Try selecting a namespace or deploying a chart".

### 4.3 Table
- The table is the view: full size, radius 8, glass, clipped; its content scrolls both ways (the header row scrolls with the rows). Fades in (300ms). Automation id `view.helm`.
- Column widths: automatic table layout (header label + 4 + 14px icon, or the longest cell; mono cells at the 0.601em pitch; +24 padding), scaled up proportionally to fill the available width.
- Header row: bg `surface/80`, bottom border slate-800.
  - Cells: `px 12, py 8`, 12px medium uppercase tracking .05em slate-400, pointer cursor, hover text accent. Automation id `helm.header.<id>`.
  - Content: row gap 4, label + sort icon (14px): `chevron-up` accent for ascending, `chevron-down` accent for descending, `chevrons-up-down` slate-600 when unsorted.
- Columns: Name, Namespace, Revision, Status, Chart, App Version, Updated. Default sort **Name ascending**. Clicking the sorted column toggles direction; clicking another column sorts it ascending. Sorting can't be removed. Comparison is "alphanumeric" (`alphanumeric`): case-insensitive, digit runs compared numerically (so `rev9 < rev10`).
- Rows: 36px tall, separated by a 1px `slate-800/50` line (none above the first). 12px slate-100 (white when selected), pointer cursor, cells `px 12`, no wrap.
  - Name and Namespace: mono.
  - Revision: plain.
  - Status: badge (below).
  - Chart: mono slate-300.
  - App Version: mono slate-400, "-" if empty.
  - Updated (`format_helm_date`): local time as `"%b %-d, %I:%M %p"` (for example "Oct 15, 10:00 AM"). **[FIXED]** (quirk 10.9) Both Helm formats are parsed (`kore_core::state::helm::parse_helm_time`): RFC 3339 (`helm history`) and the Go `time.Time` string that `helm list` emits (`2026-10-16 19:02:43.123456 +0300 IDT`; the zone abbreviation and an optional `m=±…` monotonic suffix are ignored), so list and history read alike. Anything unparsable is shown raw; "-" if empty.
  - Sorting by Updated compares the parsed instants (chronological across offsets), falling back to the alphanumeric comparison when a value does not parse.
  - Selected row bg `slate-700/60`; otherwise hover bg `muted/30`. Click selects the row and opens the detail. Automation id `helm.row.<ns>/<name>`.
- **Status badge** (`helm_status_badge`): row gap 6, `px 8, py 2`, pill, 12px medium. A 6px dot, then the status text. Variants (case-insensitive):

| Status | Dot | Bg | Text | Ping |
|---|---|---|---|---|
| deployed | emerald-400 | emerald-500/10 | emerald-400 | yes (0.5) |
| failed | red-400 | red-500/10 | red-400 | no |
| pending-* | amber-400 | amber-500/10 | amber-400 | no |
| superseded | slate-400 | slate-500/10 | slate-400 | no |
| uninstalling | orange-400 | orange-500/10 | orange-400 | no |
| other | slate-400 | slate-500/10 | slate-300 | no |

- **Keyboard** (`keys::on_window_key`; ignored while a text entry has focus, with modifiers, or while a modal is open):
  - ↓: index+1, clamped to the last row. From -1 it goes to 0.
  - ↑: index-1, minimum 0.
  - Enter: opens the selected release.
  - Initial index -1 (no selection). j/k are not handled here.

---

## 5. Helm Detail (`view = "helm-detail"`, `views/helm_detail.rs`)

### 5.1 Data
The release comes from `AppState::helm_release`. Each tab fetches on every activation (and Values on view creation):
- Values: `Core::get_helm_values(release, namespace)` → `helm get values <r> --namespace <ns> --all`.
- Manifest: `Core::get_helm_manifest(release, namespace)` → `helm get manifest`.
- History: `Core::get_helm_history(release, namespace)` → `helm history -o json`. **[FIXED] Deliberate Fix 8:** `revision` is accepted as a JSON integer or string (Helm emits an integer), so the history table lists the revisions instead of failing.
- Notes: **[FIXED]** (quirk 10.8) `Core::get_helm_notes(release, namespace)` → `helm get notes <r> --namespace <ns>`, with the leading `NOTES:` header stripped (`clean_helm_notes`). The tab shows the rendered NOTES.txt as plain text, "No notes available for this release." when it is empty (a chart without NOTES.txt), or "Error fetching notes: ..." when the call fails. (It used to show the values.)
- Rollback: `Core::rollback_helm_release(release, namespace, revision)` → `helm rollback <r> <rev>`, returning helm's stdout.
- All helm calls pass `--kube-context <current>` and the app's kubeconfig. A failed history fetch shows an error toast and the empty history state.

### 5.2 Layout
- Root: column, bg background. Enter: 300ms ease-out-quint, opacity 0→1 while sliding from 20px right.
- **Header**: padding 16, bottom border slate-800, bg `surface/50`.
  - Row 1 (space-between, margin-bottom 16):
    - Back button: CRD style (`px 12, py 6`, radius 6, 1px slate-800 border, 14px slate-300, gap 8, hover bg `muted/30`), `arrow-left` 16px, "Back" + Kbd "Esc". Automation id `helm-detail.back` (label "Go back to Helm releases list").
    - Right side: a **Copy** button only on the Values and Manifest tabs. Same button style, `copy` 16px + "Copy". It copies the raw text and toasts "Copied to clipboard" (success).
  - Row 2: four equal columns, gap 16; the first two hold the fields:
    - Label (10px uppercase tracking .05em slate-500, margin-bottom 4): "Release" / "Namespace".
    - Value: 12px mono slate-100.
- **Tabs**: row with a bottom border slate-800.
  - Each tab: `px 16, py 10`, 14px, gap 6, icon 14px: Values (`file-text`), Manifest (`scroll-text`), History (`history`), Notes (`sticky-note`). Automation ids `tab.values` … `tab.notes`.
  - Active: accent text and icon, plus a 2px accent bar at the bottom spanning the tab (pill ends). The bar moves instantly.
  - Inactive: slate-400, hover slate-200.
  - No number-key shortcuts.
- **Content** (flex 1, clipped). Switching tabs swaps the panel instantly and resets its scroll.
  - The content area pads 16 on all sides (column) and the panel fills it (flex 1). **[FIXED]** (quirk 10.8) The panel used to be margin 16 + height 100%, i.e. 32px taller than the space, so its bottom edge was clipped; now the bottom border and last lines are visible. The History panel is sized the same way.
  - **Values / Manifest / Notes panel**: fills the padded content area, scrolls vertically (6px scrollbar), bg `surface/30`, 1px border slate-800, radius 8.
    - Loading: padding 16, 10 bars 12px tall, gap 10, widths 80, 65, 90, 45, 75, 55, 85, 40, 70 and 60%, with opacity `1 - i*0.06`.
    - Loaded: padding 16, mono 12px with 19px line height, wrapped.
    - Values and Manifest are YAML-highlighted line by line (`highlight_line`):
      - Lines whose first non-blank char is `#` are entirely slate-500.
      - `^(\s*)([\w.-]+)(\s*:\s*)(.*)` → the key in accent; the value colored by the value rule.
      - `^(\s*-\s+)(.*)` → the dash prefix in slate-500, plus the value rule.
      - Value rule: `true`/`false` (case-insensitive) indigo-400 `#818cf8`; `^-?\d+(\.\d+)?$` amber-400; `null` or `~` slate-500; anything else (quoted or not) emerald-400. An empty value is uncolored.
      - Other lines are uncolored (slate-100).
    - Empty fallbacks: "# No values configured" and "# No manifest available" (rendered as comments). Fetch errors show "Error fetching values: ..." or "Error fetching manifest: ..." in the panel.
    - Notes: slate-300 plain text.
  - **History panel**: fills the padded content area, scrolls vertically.
    - Loading: glass box (radius 8) with padding 16, gap 12: a full-width 16px bar, then 5 rows (gap 16, opacity `1 - i*0.1`) of 16px bars 48, 128, 80, 96 and flex.
    - Empty: centered `history` 48px slate-600 and "No revision history found" 14px medium slate-400, gap 12.
    - Table: glass, radius 8, clipped, header styled like the CRD table (bg `surface/80`, 12px medium uppercase slate-400, `px 12, py 8`). Column widths: automatic layout like the list. Automation id `helm-detail.history`.
      - Columns: Revision, Updated, Status, Chart, App Version, Description, Actions (right-aligned).
      - Rows: 36px, 1px `slate-800/50` separators. Bg `emerald-500/5` if the status is deployed, else hover bg `muted/30`.
      - Revision: mono 12px slate-100, plus a "latest" pill on the **last array element** (gap 6; 10px medium accent sans, line height 12, `px 6, py 2`, radius 4, no fill).
      - Updated: formatted like the list view, slate-300.
      - Status: the same badge as the list (ping when deployed).
      - Chart: mono slate-300. App Version: mono slate-400, "-" if empty. Description: slate-400, truncated at 176px, "-" if empty.
      - Actions: for every non-deployed row, a "Rollback" button: `px 10, py 4`, radius 6, 1px border slate-700, 12px slate-300, `rotate-ccw` 12px, gap 6. Hover: border `amber-500/50`, bg `amber-500/10`, text and icon amber-400. While a rollback runs: opacity .5 and inert. Automation id `helm-detail.rollback.<rev>`.
- **Rollback confirm**: ConfirmDialog variant `Warning`.
  - Title "Rollback Helm Release". Message `Are you sure you want to rollback "{name}" in namespace "{ns}" to revision {rev}? This will create a new revision.`
  - Confirm "Rollback"; Cancel "Cancel".
  - Success: toast with helm's stdout, or the fallback `Rolled back to revision {rev}` (success); the dialog closes; the history is refetched if the History tab is active.
  - Failure: error toast; the dialog stays open.
- **Keyboard**: `AppEvent::Escape` calls `back()` (returns to the Helm list).

---

## 6. AI features

### 6.1 Config model and persistence (`state/ai_config.rs`)

`AiConfig = { provider: "openai"|"anthropic"|"ollama"|"claude_cli"|"cursor_agent", api_key: Option<String>, model: String, base_url: Option<String> }`, converted to `kore_core::AIConfig` by `to_core()` (empty key and base URL become `None`).

Providers (`PROVIDERS`, in segmented-control order):

| id | label | default model | needs key | needs base URL | default base URL |
|---|---|---|---|---|---|
| openai | OpenAI | `gpt-4o` | yes | no | none |
| anthropic | Anthropic | `claude-sonnet-4-20250514` | yes | no | none |
| ollama | Ollama | `llama3.1` | no | yes | `http://localhost:11434` |
| claude_cli | Claude Code | `sonnet` | no | no | none |
| cursor_agent | Cursor Agent | `auto` | no | no | none |

Persistence:
- Non-secret config is stored under the key `kore-ai-config` in the app's key/value store (`state/storage.rs`: `<OS data dir>/kore/local-storage.json`, written atomically) as JSON `{provider, model, base_url?}`. `api_key` is never serialized.
- Default when nothing is stored: `{provider:"openai", model:"gpt-4o"}`.
- **App-global model**: `ai_config::model(cx)` returns one shared `AiConfigModel` entity. AI Chat, the AI Panel, Network Policies and AI Settings all `cx.observe` it, so a change in Settings updates every surface immediately. `AiConfigModel::set` persists (without the key) and notifies.
- **API keys** live in the OS keychain (`kore-core secrets.rs`, `keyring` service `"kore"`, account = provider id):
  - `Core::store_api_key(provider, key)` on every edit of the key field while non-empty.
  - `Core::delete_api_key(provider)` when the field is emptied.
  - `Core::get_api_key(provider)`: **[FIXED] Deliberate Fix 11:** the shared model loads the key from the keychain when it is created and whenever the provider changes to a key-needing one, so OpenAI and Anthropic work in every chat surface once a key is stored.
- `is_provider_configured(config)` is true if the provider exists and (it needs no key, or `api_key` is non-empty).

### 6.2 AI Settings (inside the Settings page, `views/ai_settings.rs`)
- Hosted in the Settings "AI" card (`settings.rs::card`): glass, radius 12, padding 20. No enter animation. Contents are a column with gap 20.
- Header (gap 8): `settings-2` 16px accent; "AI Provider" 14px semibold slate-100.
- **On view creation**: `Core::check_providers_availability(ollama_base_url)` → `{claude_cli, cursor_agent, ollama}`.
  - claude_cli: `claude --version` succeeds.
  - cursor_agent: `agent --version` succeeds.
  - ollama: `GET {base}/api/tags` returns 2xx within 3s. **[FIXED] Deliberate Fix 13:** `{base}` is the configured Base URL (default `http://localhost:11434`), not always the default.
  - On failure every provider counts as available. openai and anthropic are never in the map, so they are always enabled.
- **Segmented control**: a row with a 1px slate-800 border, radius 8, clipped. Five equal-width buttons, `px 12, py 8`, 12px medium, centered, separated by a 1px slate-800 right border (not on the last). Automation ids `settings.ai.provider.<id>`.
  - Selected: accent text and **no background** (so the card's glass shows through).
  - Unselected: bg `surface/60`, slate-400; hover slate-200 and bg `muted/40`.
  - Unavailable: bg `surface/60`, slate-600, opacity .4, not-allowed cursor, inert, with a second line "not found" (9px slate-600, line height 11.25, margin-top 2).
  - Clicking a provider sets `model = default_model` and `base_url = default_base_url` (`None` if none), keeps `api_key` only when re-selecting the same provider (otherwise the keychain key for the new provider is loaded), updates the API Key and Model placeholders, and resets the test result.
- Fields: label (12px slate-400, margin-bottom 6) over a 38px box (`text_box`): bg `surface/60`, 1px border slate-800, radius 8 (4 + focus ring when focused), `TextInput` with padding 12/12/8/8, 14px slate-100, placeholder slate-600.
- **API Key** (OpenAI and Anthropic only): masked input, placeholder `Enter your {Label} API key`. Automation id `settings.ai.api-key`.
- **Model** (label "Model"). Render modes:
  1. ollama, claude_cli or cursor_agent with a fetched model list: a select (`ui::native_controls::NativeSelect`, no arrow), mono, `px 12, py 8`, radius 8, 14/20 slate-100, bg `surface/60`, border slate-800, full width. Options are the model names.
  2. Any of those three while loading: a box (`px 12, py 8`, same bg/border/radius) with `loader-2` 14px spinning + "Loading models..." in 14px slate-500.
  3. Otherwise: a mono text input with the provider's default model as placeholder (automation id `settings.ai.model-input`).
  - The list is fetched when the provider (or, for Ollama, the base URL) changes: `Core::list_ollama_models(base_url)` (`GET {base}/api/tags`, names from `.models[].name`), `Core::list_claude_models()` (`["opus","sonnet","haiku"]` when the `claude` CLI exists), `Core::list_cursor_agent_models()` (parsed from `agent models`). If the current model is not in a non-empty list, the first model is selected.
- **Base URL** (Ollama only): label "Base URL"; mono text input with placeholder "http://localhost:11434". An empty value is stored as `None`. Automation id `settings.ai.base-url`.
- **Test Connection**: full-width button, `px 16, py 8`, radius 8, 1px border, 14px medium, centered, gap 8. Automation id `settings.ai.test` (label = current text).
  - Idle: accent text, `PREFLIGHT_BORDER` `#e5e7eb` border, no fill. Label "Test Connection".
  - Testing: `loader-2` 16px accent spinning + "Testing...", same border.
  - Success: bg `emerald-500/10`, border `emerald-500/50`, text emerald-400, `check-circle-2` 16px + "Connected". Toast "Connection successful".
  - Error: bg `red-500/10`, border `red-500/50`, text red-400, `alert-circle` 16px + "Failed - Retry". Toast `Connection failed: {msg}`.
  - Disabled (opacity .4, not-allowed cursor) while testing, or when the provider needs a key and none is present.
  - Backend `Core::ai_test_connection_checked(config)` (15s HTTP timeout):
    - OpenAI: `GET {openai base}/v1/models` with the bearer key.
    - Anthropic: `POST {anthropic base}/v1/messages` with `max_tokens 1`.
    - Ollama: `GET {base}/api/tags`.
    - Claude Code / Cursor Agent: CLI availability; unavailable is an error ("… CLI is not installed or not in PATH").
    - **[FIXED] Deliberate Fix 12:** a non-2xx response is an error with the message `HTTP {status} {reason}: {body}` (body trimmed, capped at 200 chars + "…"), e.g. `Connection failed: HTTP 503 Service Unavailable: mock unavailable`. A wrong key therefore shows "Failed - Retry".
- Any field edit resets the test result to idle.

### 6.3 Backend AI protocol (`kore-core state/ai.rs`; needed to reimplement or mock)

**Stream events** (`AIStreamEvent`, serialized tagged by `type`):
- `Chunk { content }` (`{type:"chunk", content}`)
- `Done` (`{type:"done"}`)
- `Error { message }` (`{type:"error", message}`)
- `Status { message }` (`{type:"status", message}`)

Both AI calls return a `Subscription<AIStreamEvent>`; dropping the subscription cancels the request (and kills a `claude`/`agent` child process). The UI drains it with `state/ai_stream.rs` (`start_chat` / `start_diagnose`), which coalesces bursts into one update, stops at the first `Done` or `Error`, synthesizes a `Done` if the stream closes without one (so a surface never stays stuck "streaming"), and reports a failure of the call itself as `AiUpdate::Failed(formatted message)`.

Provider endpoints: OpenAI `https://api.openai.com` and Anthropic `https://api.anthropic.com`, overridable with the environment variables **`KORE_OPENAI_BASE_URL`** / **`KORE_ANTHROPIC_BASE_URL`** (trailing `/` and `/v1` are stripped; Deliberate Fix 13). Ollama uses the configured base URL, default `http://localhost:11434`.

**`Core::ai_diagnose(config, DiagnoseRequest { kind, namespace, name, prompt, session_id })`** (used by the AI Panel).
- If kind, name and namespace are all present, the backend gathers: the describe output, the resource's events, and for pods also the last 200 log lines and pod metrics. Failures are logged and skipped.
- It then makes a **streaming** call with the system prompt and one user message:
  - OpenAI: `stream:true` SSE. Each line is `data: {...}` with `choices[0].delta.content`; `data: [DONE]` ends.
  - Anthropic: SSE with `content_block_delta` → `delta.text`; `message_stop` ends.
  - Ollama: `POST {base}/api/chat` with `stream:true`, newline-delimited JSON. Each object is `{message:{content}, done}`; `done:true` ends.
  - Claude Code and Cursor Agent: subprocess stream-json.
- A chunk is emitted per text piece, then done. On HTTP error: an error event `AI provider returned HTTP {status}: {body}`.
- Responses are capped at `MAX_AI_RESPONSE_BYTES` (2 MiB); exceeding it emits an error.

**`Core::ai_chat(config, AIChatRequest { messages: [ChatMessage {role, content}], session_id, namespace })`** (used by AI Chat and Network Policies).
- **Non-streaming** tool loop, at most 10 rounds:
  - OpenAI: `POST /v1/chat/completions` (no `stream`) with tools.
  - Anthropic: `/v1/messages`, `max_tokens 4096`, with tools.
  - Ollama: `POST {base}/api/chat` with `stream:false` and tools.
- If the response has tool calls, then for each one it emits `Status { "Querying {tool name with _→space}..." }`, for example "Querying get cluster health...", executes the tool and appends the result.
- Tools: `list_resources`, `get_cluster_health`, `describe_resource`, `get_resource_events`, `get_pod_logs`, `get_pod_metrics`, `search_resources`, `list_namespaces`.
- Ollama fallback: if `message.tool_calls` is empty but the content contains JSON like `{"name":"<known tool>","parameters":{...}}`, those calls are executed and the results are injected as a user message, followed by one final call without tools.
- The final text is emitted as **80-byte chunks every 15ms** (simulated streaming), then done.
- If 10 rounds are exhausted, it emits the chunk `"\n\n*[Reached maximum tool call rounds]*"` and then done.
- System prompt: "You are Kore AI, a Kubernetes cluster assistant embedded in the Kore desktop IDE. …" with the current context and namespace lines. The UI's messages are sent as-is (Network Policies sends its own `system` message too).

Session ID formats (`ai_stream::session_id(prefix)` = `{prefix}-{unix millis}-{n}`):
- AI Panel: `ai-session-…`
- AI Chat: `chat-…`
- Network Policies: `np-ai-…`

Each send uses a new session ID. Starting a new send replaces (and therefore cancels) the previous stream task of that surface; leaving the view drops it.

### 6.4 Chat bubble (`views/ai_chat_bubble.rs::chat_bubble_sized`) and markdown (`ui/markdown.rs`)

Inputs: a `DisplayMessage { role, content }`, a variant `Compact` (AI Panel, Network Policies) or `Comfortable` (AI Chat), whether assistant messages get a copy button, and the row width.

- Row: full width, gap 10 (compact) or 12 (comfortable). User messages are right-justified, assistant messages left-justified. Automation id = the bubble id (`chat-msg-N`, `ai-panel-msg-N`, `np-msg-N`), role `listitem`; `<id>.text` carries the raw message (label = content) and `<id>.copy` the copy button.
- **Assistant avatar**: compact 24×24 radius 6; comfortable 28×28 radius 8. No fill. `bot` icon 14px (compact) or 16px (comfortable) in accent. Margin-top 2, does not shrink.
- **Bubble**:
  - Shrink-to-fit: its width is the content's max-content width (+ padding, + 2 for the assistant border), capped at 85% of the row. 14px, line height 22.75 (1.625), clipped.
  - Compact: radius 8 with the **bottom-left radius 2** (assistant) or **bottom-right radius 2** (user); padding 12 horizontal / 8 vertical.
  - Comfortable: radius 12 with the same 2px corner; padding 16 / 12.
  - User: text slate-100, wrapped, plain text (not markdown); **no background**, so the text floats right with no bubble fill.
  - Assistant compact: bg `muted/60`, 1px border `slate-800/50`, text slate-200.
  - Assistant comfortable: bg surface `#101828`, same border, text slate-200.
- **Copy button** (assistant messages):
  - Positioned at top -8, right -8 of the bubble. Padding 4, radius 4, bg surface, 1px border slate-700, `copy` icon 12px slate-400.
  - Hidden (opacity 0) until the bubble is hovered, then shown (opacity 1, no transition).
  - **[FIXED]** (quirk 10.13) The bubble's sizing, hover group and the button live in an unclipped wrapper; only the inner bubble (bg, border, padding, content) clips. The whole 22×22 button is visible over the bubble's top-right corner (it used to show only its bottom-left 14×14).
  - Click copies the raw markdown and toasts "Copied to clipboard". Automation id `<bubble id>.copy`.
- **Markdown** (`markdown::parse` + `render`): GFM via the `markdown` crate (mdast). Supported: headings, paragraphs, bold, italic, strikethrough (1px line-through), inline code, fenced code (no syntax highlighting), block quotes, ordered/unordered lists, task lists (a read-only 13px checkbox, `✓` when checked), tables, thematic breaks, autolinks. Images render their alt text. Raw HTML is dropped. Link URLs are sanitized (only `http`, `https`, `mailto`, `irc`, `ircs`, `xmpp` or relative URLs keep their link). **[FIXED]** Clicking a link opens it in the system browser. Soft line breaks render as spaces; hard breaks as new lines.

Markdown styles (root: 14px, line height 22.75, slate-200, min-width 0; the first block's top margin and the last block's bottom margin are dropped; vertical margins between blocks collapse to the larger one):

| Element | Style |
|---|---|
| h1 | 16/24 bold slate-100, margin 20 top / 8 bottom |
| h2 | 14/20 bold slate-100, 20/8 |
| h3 | 14px semibold slate-100, 16/6 |
| h4-h6 | 14px medium slate-200, 12/4 |
| p | margin 8 vertical (0 inside tight list items) |
| strong | semibold slate-100 |
| em | italic slate-300 |
| a | accent, 1px accent underline |
| ul | margin 8 vertical, margin-left 16, 5px slate-200 dot markers 14px left of the text, items spaced 4 |
| ol | same, `N.` markers right-aligned before the text |
| blockquote | margin 12 vertical, padding-left 12, 2px left border accent at 40%, slate-400 italic |
| hr | margin 16 vertical, 1px `slate-700/60` |
| inline code | mono, color = accent mixed 90% with white (`#69d5ff` for the default accent), bg `slate-800/80`, padded by a thin space on each side |
| pre | margin 12 vertical, radius 8, bg `slate-900/80`, 1px border `slate-800/60`, horizontal scroll; inner `px 16, py 12`, mono 12px, line height 19, slate-300, no wrap |
| table | margin 12 vertical, 12/16, horizontal scroll; columns sized to their widest cell + 24 |
| table header | bottom border slate-700; cells `px 12, py 8`, semibold slate-300, bg `slate-800/50`; first cell top-left radius 6, last cell top-right radius 6 |
| table cells | `px 12, py 8`, slate-300, bottom border `slate-800/40`, no wrap; row hover bg `slate-800/20` |

### 6.5 AI config warning (`banners::ai_config_warning`, `ai_chat::config_warning`)
A strip with `px 16, py 10` (AI Chat uses `px 24`), bottom border `slate-800/50`, bg `amber-500/5`. Row gap 8, 12px: `alert-circle` 14px amber-400, then "Configure an AI provider in Settings to use this feature" in `amber-300/90`. Automation id `banner.ai-config`.

### 6.6 AI Panel (slide-over overlay, `views/ai_panel.rs`)

**Open and close**
- Opens from the header "AI" button (table and details views), the command palette "Ask AI", via `AppState::open_ai_panel(AiPanelRequest)`.
- The request carries the resource context when the app is in the details view: `{kind: <plural kind, e.g. "pods">, namespace: selected.namespace or "default", name}`; otherwise it is empty.
- **Backdrop**: covers the window, `rgba(0,0,0,.3)`, no fade. Click closes.
- **Panel**: pinned to the right edge, full height, width 420 (max 90% of the window), column, bg surface, 1px left border slate-800, shadow-2xl. Slides in from the right (250ms ease-out-quint). No exit animation. Clicks inside do not reach the backdrop. Automation ids `ai-panel`, `ai-panel.close`.
- While open it registers the modal id `ai-panel`, so Esc never navigates back underneath it. Esc (window key, unless the palette, shortcut overlay or a confirm dialog is open) closes it, also from its input.
- The input is focused 200ms after open (only if it is enabled).
- The conversation (messages + streaming flag + stream task) lives in an app-global `AiPanelSession`, so it **persists** across close and reopen. **[FIXED]** A stream keeps running while the panel is closed and finishes normally, so closing mid-stream never leaves the input disabled.

**Header**: `px 16, py 12`, bottom border slate-800, bg `surface/80`, space-between.
- Left (gap 10):
  - Tile: 32×32, radius 8, no fill, containing `bot` at **18px** in accent.
  - "AI Assistant" 14px semibold slate-100 over "Kubernetes troubleshooting" 10px slate-500.
- Right (gap 4):
  - Clear (`ai_chat::clear_button`): padding 6, radius 6, `trash-2` 16px. Enabled when messages exist: slate-400, hover red-400 with bg `muted/50`. Otherwise slate-700, not-allowed. Clearing empties the messages; a running stream continues. Automation id `ai-panel.clear`.
  - Close: padding 6, radius 6, `x` 16px slate-400, hover slate-200 with bg `muted/50`.

**Optional sections, in order**
- Config warning (full width, 6.5), when not configured.
- Context strip (when the request has kind, namespace and name): `px 16, py 8`, bottom border `slate-800/50`, no fill. 12px row gap 8: `sparkles` 14px accent; "Context:" in slate-400; `{kind}/{namespace}/{name}` mono accent, truncated. Automation id `ai-panel.context` (label = `{kind}/{namespace}/{name}`).
- Quick Actions (when there are no messages): `px 16, py 12`, bottom border `slate-800/50`.
  - Label "QUICK ACTIONS": 10px uppercase tracking .05em slate-500, margin-bottom 8.
  - Wrapping row, gap 6. Buttons: `px 12, py 6`, radius 8, 1px border slate-700, 12px slate-300, hover text accent; disabled (streaming or unconfigured) opacity .4. Automation ids `ai-panel.quick.<label>`.
  - Labels and prompts:
    - "Diagnose" → "Diagnose this resource and identify any issues."
    - "Why is this failing?" → "Why is this resource failing? Analyze the status, events, and conditions."
    - "Suggest fixes" → "Suggest fixes for the current issues with this resource."

**Messages area**: flex 1, vertical scroll (6px scrollbar), `px 16, py 12`, items spaced 16. Scrolls to the bottom on every session change and on open.
- Empty state: centered column, padding-x 24, centered text.
  - Tile 56×56, radius 16, no fill, margin-bottom 16, containing `bot` 28px in accent at 60% (**[FIXED]** quirk 10.2; was slate-100).
  - "Ask about your Kubernetes resources" 14px slate-400, margin-bottom 4.
  - "Diagnose issues, understand failures, and get fix suggestions." 12px slate-600.
- Messages use the compact bubble. Assistant messages have copy.
- While streaming, after the messages: `loader-2` 14px accent spinning + "Thinking..." 12px slate-500. It shows during the **entire** stream.

**Input form**: `px 16, py 12`, top border slate-800, bg `surface/80`.
- Row gap 8. Input box: flex 1, 38px tall, radius 8 (4 + focus ring when focused), bg background, 1px border slate-800, `TextInput` 14px slate-100 with padding 12/12/8/8, placeholder slate-600. Automation id `ai-panel.input`.
  - Placeholder: "Configure an AI provider to get started..." (unconfigured), "Waiting for response..." (streaming), or "Ask about this resource...".
  - Disabled (opacity .5, clicks blocked, not-allowed cursor) when streaming or unconfigured.
- Send button (`ai_chat::send_button`): padding 8, radius 8, 1px border, `send` 16px. Automation id `ai-panel.send`.
  - Active (non-empty text, not streaming, configured): accent icon, `PREFLIGHT_BORDER` border, no fill.
  - Otherwise: bg `muted/30`, slate-600 icon, border slate-800, not-allowed.
- Disclaimer: margin-top 6, 10px slate-600, centered: "AI responses may be inaccurate. Always verify before acting."
- Enter submits. Empty or whitespace input is ignored.

**Send flow** (`AiPanelSession::send`)
1. Ignored when the text is blank, a stream is running, or AI is not configured. Otherwise append the user message (trimmed) and clear the input.
2. `ai_stream::start_diagnose(config, DiagnoseRequest { kind, namespace, name, prompt, session_id: "ai-session-…" })`.
3. Event handling:
   - Chunk (non-empty): append to the last assistant message, or create one.
   - Done: stop streaming.
   - Error: stop streaming; append `\n\n[Error: msg]` to the last assistant message, or add a new `[Error: msg]` message (`msg` = "An unknown error occurred." if empty).
   - The call itself failing: stop streaming and add `[Error: msg]`.

### 6.7 AI Chat view (`view = "chat"`, the default landing view, `views/ai_chat.rs`)
- Root: full-size column. Automation id `view.chat`.
- **Header**: `px 24, py 12`, bottom border slate-800, bg `surface/50`, space-between.
  - Left (gap 12): 32px tile (no fill) with `bot` at 18px accent; "Kore AI" 14px semibold slate-100 over "Cluster assistant with live data" 10px slate-500.
  - Right: the `trash-2` clear button, same as the AI Panel. Automation id `chat.clear`.
- Config warning (`px 24`), when not configured.
- **Messages area**: flex 1, vertical scroll (6px scrollbar). Inner column full width up to 768, centered, padding 24.
  - **Empty state** (no messages and not streaming): column, centered, padding-top 64, padding-bottom 32.
    - Tile 64×64, radius 16, no fill, margin-bottom 20, `bot` 32px in accent at 60% (**[FIXED]** quirk 10.2; was slate-100).
    - "What can I help you with?" 18/28 semibold slate-100, margin-bottom 6.
    - "Ask anything about your cluster. I'll query live data to give you accurate answers." 14px slate-500, margin-bottom 32, max-width 448, centered.
    - Suggestion grid: a wrapping row, gap 8, full width up to 512; each button is ~165px wide (three per row at full width).
      - Button: row gap 8, `px 12, py 10`, radius 8, 1px border slate-800, bg `surface/60`, 12px slate-300 (icon too), left-aligned, hover text and icon accent, disabled (streaming or unconfigured) opacity .4 and not-allowed. Icon 14px. Automation ids `chat.suggestion.<label>`.
      - Buttons:
        - `activity` "Cluster health" → "Give me an overview of the cluster health. Are there any issues?"
        - `alert-triangle` "Failing pods" → "What pods are currently failing or in an error state? Why are they failing?"
        - `arrow-up-circle` "High restarts" → "Which pods have the most restarts? What's causing them?"
        - `bell` "Recent warnings" → "Show me recent warning events in the cluster and explain what they mean."
        - `layout-list` "Resource overview" → "Give me an overview of all deployments and their status."
  - Message list: spaced 20, comfortable bubbles, copy on assistant messages.
  - **Tool status pill** (when the latest event was a status): row gap 10, `px 12, py 8`, radius 8, 1px `PREFLIGHT_BORDER` border, no fill, 12px accent. Contents: `wrench` 14px accent, `loader-2` 12px accent spinning, then the status text. Cleared on the next chunk, on done, or on error. Automation id `chat.tool-status`.
  - **Progress indicator** (automation id `chat.progress`): shown when streaming AND no tool status. Padding-left 40: `loader-2` 14px accent + 12px slate-500 text: "Thinking..." until the first chunk (the last message is the user's), then "Generating...". **[FIXED]** There used to be no indicator between send and the first status or chunk.
  - Scrolls to the bottom (no smooth scrolling) on send and on every stream update.
- **Input area**: top border slate-800, bg `surface/50`, `px 24, py 16`. The form is full width up to 768, centered.
  - Row aligned to the bottom, gap 12.
  - Text area box (`ui::textarea::TextArea`): flex 1, `px 16, py 10`, radius 12 (4 + focus ring when focused), bg background, 1px border slate-800, 14px slate-100, placeholder slate-600. It **auto-grows** with its content up to 120px total height, then scrolls. Clicking the box focuses the text area (when enabled). Disabled: opacity .5. Automation id `chat.input`.
  - Placeholder: as in the AI Panel, except the idle text is "Ask about your cluster...".
  - **Enter sends; Shift+Enter inserts a newline.**
  - Send button: padding 10, radius 12, styled like the panel's. Automation id `chat.send`.
  - Hint: margin-top 8, 10px slate-600, centered: "Enter to send, Shift+Enter for newline. AI queries live cluster data."
- Focus: the text area is focused 200ms after the view is created (if enabled).
- **Send flow**:
  - Ignored when blank, streaming, or unconfigured. Otherwise append the trimmed user message, clear the input, reset the tool status, then `ai_stream::start_chat(config, AIChatRequest { messages: <entire conversation incl. the new user message>, session_id: "chat-…", namespace: current namespace or None })`.
  - Events: chunk clears the tool status and appends to (or creates) the assistant message; status sets the tool status; done ends; error appends `\n\n**Error:** msg` (rendered bold), or adds a new `**Error:** msg` message; a failed call adds `**Error:** msg`.
- Clear empties the messages and the tool status; a running stream continues.
- State is lost when leaving the view, and kept across namespace changes.

---

## 7. RBAC Simulator (`view = "rbac"`, `views/rbac/`)

### 7.1 Data (`rbac/mod.rs`)
- On view creation: `Core::rbac_list_identities()` → `RbacIdentityList { service_accounts: [{name, namespace}], users: [...], groups: [...] }`. Service accounts are all SAs in the cluster. Users and groups are the distinct subjects of all (Cluster)RoleBindings, sorted.
- `Core::rbac_build_matrix(identity, namespace)` runs when the selected identity or the namespace changes.
- `Core::rbac_list_roles(namespace)` runs every time the Role Browser tab is activated (and on namespace change while it is active). All ClusterRoles are included, plus Roles (filtered to the namespace if one is set). Sorted by kind ("ClusterRole" before "Role"), then name. Each has `rule_count` and `rules`.
- `Core::rbac_reverse_lookup(role_kind, role_name, role_namespace)` runs on role click. It returns the role's own rules (aggregation is not expanded here) and subjects from CRBs (`scope "cluster-wide"`) and RBs (`scope "namespace: X"`).
- `Core::rbac_check_permission(identity, verb, resource, namespace)`: the backend derives the apiGroup from the resource.
- `Core::rbac_who_can(verb, resource, namespace)`: every SA plus every binding subject, filtered to those allowed.
- `RbacIdentity` is tagged by `kind`: `{kind:"User",name}`, `{kind:"Group",name}`, `{kind:"ServiceAccount",name,namespace}`.
- **Permission semantics** (`kore-core state/rbac.rs`):
  - Collect a rule chain from every ClusterRoleBinding whose subjects match the identity, then every RoleBinding **in the target namespace** (only if a namespace is given).
  - For each binding, resolve the referenced Role or ClusterRole's rules. ClusterRole aggregation via `aggregationRule.clusterRoleSelectors.matchLabels` is expanded here.
  - A rule matches if its verbs contain `*` or the verb, its resources contain `*` or the base resource (the part before `/`), and its apiGroups contain `*` or the group (`""` = core).
  - Allowed = the chain is non-empty.
  - Subject matching is exact: SA by name and namespace, User and Group by name. **Groups are not inherited** (an SA does not get `system:serviceaccounts` bindings).
  - Summary text: allowed → `Allowed because of {RoleKind} '{role}' via {BindingKind} '{binding}'`; denied → `{Identity} cannot {verb} {resource} in {ns|cluster scope}`. Identity is formatted `ServiceAccount "ns/name"`, `User "x"` or `Group "x"`.
- **Matrix**:
  - Rows are fixed and ordered (`RESOURCES`): pods, deployments(apps), replicasets(apps), statefulsets(apps), daemonsets(apps), services, ingresses(networking.k8s.io), configmaps, secrets, nodes, namespaces, persistentvolumeclaims, persistentvolumes, serviceaccounts, jobs(batch), cronjobs(batch), roles(rbac.authorization.k8s.io), rolebindings(rbac…), clusterroles(rbac…), clusterrolebindings(rbac…), events, networkpolicies(networking.k8s.io).
  - Columns (`VERBS`): get, list, watch, create, update, patch, delete.
  - Cell status: **allowed**; **conditional** (allowed, but some rule in the chain has non-empty `resourceNames`); **denied**.
- Errors from identities, matrix, roles and reverse-lookup calls set the view error, which is shown in the header (and analyzed for the Forbidden banner, 7.8).

### 7.2 Layout
- Root: full-size column, fades in (300ms). Automation id `view.rbac`.
- **Header**: `px 16, py 12`, bottom border slate-800, row gap 12.
  - Back button (`back()` → table view): padding 4, radius 4, hover bg `slate-700/40`, `arrow-left` 16px slate-400, hover slate-200. Automation id `rbac.back`.
  - `shield-check` 20px accent; "RBAC Simulator" 14px medium slate-200.
  - Spacer; then, if there is an error, `alert-circle` 14px + the error text in 12px red-400, gap 6 (`rbac.error`).
- **Tab bar**: `px 16, py 8`, bottom border slate-800, gap 4.
  - Each tab: row gap 6, `px 12, py 6`, radius 6, 12px. Icon 14px + label + RBAC Kbd with the number. Automation ids `tab.permission-matrix`, `tab.role-browser`, `tab.query`, `tab.impersonation`.
  - Active: bg `slate-700/60`, slate-200. Inactive: slate-500, hover slate-300 with bg `slate-800/50`.
  - Tabs: 1 "Permission Matrix" (`grid-3x3`), 2 "Role Browser" (`book-open`), 3 "Query" (`message-circle-question`), 4 "Impersonation" (`user-cog`).
  - Switching tabs closes the matrix popover and any open select, and resets the content scroll.
- **Keyboard** (`keys::on_window_key`):
  - Esc (unless the palette, shortcut overlay, a confirm dialog or the AI Panel is open; they own it) closes, in order, an open identity picker (also while its search field has focus; **[FIXED]** quirk 10.22), the cell popover, or an open Query select (consumed). Otherwise it falls through to root (Esc → back is not wired for this view).
  - Ignored while a text entry has focus or the palette / shortcut overlay is open:
  - `1`–`4` switch tabs.
  - `i` (no Cmd/Ctrl) exits impersonation if it is active.
  - App-level: Cmd/Ctrl+Shift+R opens this view.
- **Content**: flex 1, vertical scroll, padding 16.
  - When the latest RBAC view error (identities, matrix, roles, reverse lookup or a query) is a 403, the Forbidden banner (7.8) is shown at the top of the content (max-width 672, margin-bottom 16; automation id `rbac.forbidden`).

### 7.3 Tab 1: Permission Matrix
- Column, gap 16, height `window height - 200`.
- Row (gap 12): the identity picker (7.7). While a matrix (or reverse lookup) is loading: `loader-2` 16px slate-500 spinning.
- Body:
  - If a matrix exists: the matrix (flex 1).
  - Else if not loading: "Select an identity to view their permissions", 14px slate-500, centered.
- Selecting an identity anywhere (who-can result, subject in the role browser) sets it and switches to this tab.
- **Matrix** (`rbac/matrix.rs`):
  - Top row (space-between):
    - Legend (gap 16, 10px slate-500). Three items (gap 6): a 12×12 swatch (radius 2) with bg `X-500/20` and 1px border `X-500/30`, then the label: Allowed (emerald), Denied (red), Conditional (amber).
    - "Copy CSV" button: `px 8, py 4`, radius 4, 10px slate-500, hover slate-300 with bg `slate-700/40`, `copy` 12px, gap 6. Automation id `rbac.copy-csv`.
      - It copies `Resource,get,list,watch,create,update,patch,delete` followed by one line per row: `pods,allowed,allowed,...` (a missing cell is written as `denied`; `matrix_csv`).
      - **No toast.**
  - Table container: margin-top 12, flex 1, scrolls vertically (6px scrollbar), radius 8, 1px border slate-800, clipped. The table always fills the container width: the resource column's natural width is 315.4px and each verb column's 60px, all scaled up proportionally (`column_widths`); there is no horizontal scrolling. Automation id `rbac.matrix`.
    - Header row (pinned at the top while rows scroll): 32px, bg surface, bottom border slate-800.
      - "Resource": left-aligned, `px 12, py 8`, 10px uppercase tracking .05em slate-500 medium.
      - Verb headers: centered, `px 8, py 8`, same text style (lowercase names displayed uppercase).
    - Rows: 39px + a 1px `slate-800/50` divider. Odd rows (index 1, 3, …) bg `white/1`. Automation id `rbac.row.<resource>`.
    - Resource cell: `px 12`, 12px mono slate-300, bg surface. If `api_group` is non-empty, it follows in 10px slate-600 with margin-left 4 (for example "deployments apps").
    - Verb cell: centered, a 20×20 button (radius 2) 5.5px below the row top. Automation id `rbac.cell.<resource>.<verb>` (label `"{resource} {verb}: {Allowed|Denied|Conditional}"`).
      - Fill: allowed `emerald-500/20`, denied `red-500/20`, conditional `amber-500/20`.
      - Hover (and while its popover is open): grows to 25×25 (radius 2.5) centered on the same spot, fill at .3, 1px border in `X-500/50`. No transition.
      - A missing cell is a non-interactive 20px square in `slate-800/50`.
  - **Cell popover** (on click; `render_popover`):
    - Window-anchored at `top = cell.bottom + 8`, `left = min(cell.left, windowWidth - 360)` (measured on the 25px hovered cell). Width 340, max-height 320, vertical scroll, glass, radius 8, padding 12, drawn above everything. Fades in with y 4 over 150ms. Automation id `rbac.popover`.
    - Header (space-between, margin-bottom 8): `"{resource} / {verb}"` 12px mono slate-400; a status pill (10px medium, line height 15, `px 6, py 2`, radius 4; allowed bg `emerald-500/15` + emerald-400 text, denied red-500/15 + red-400, conditional amber-500/15 + amber-400).
    - Body: the rule chain (7.4), with `allowed = status != denied`.
    - Closes on a mouse-down outside it or on Esc. A new matrix replaces it (closes it).

### 7.4 Rule chain (`rbac/rule_chain.rs`)
- Empty chain: "No matching rule found", 14px slate-500 italic.
- Otherwise a column (gap 4); each entry is a block (internal gap 4) that fades in with y 4→0 over 150ms, delayed `i*50ms`.
  1. **Binding box**: row gap 8, `px 10, py 6`, radius 4, bg `indigo-500/10`, 1px border `indigo-500/20`.
     - `link-2` 14px indigo-300.
     - 12px mono, wrapping: `binding_kind` in indigo-400, " / " in slate-500, `binding_name` in indigo-200, and optionally " (ns: X)" in slate-600.
  2. Connector: a centered 1×12 line in slate-700.
  3. **Role box**: same layout in purple (bg `purple-500/10`, border `purple-500/20`, `shield` purple-300, kind purple-400, name purple-200, optional ns).
  4. Connector.
  5. **Rule box**: row aligned to the top, gap 8, `px 10, py 6`, radius 4, 1px border.
     - Allowed: bg `emerald-500/10`, border `emerald-500/20`, `file-text` 14px emerald-300 (margin-top 2).
     - Denied: the red equivalents (red-300 icon).
     - 12px mono text (`rule_text`):
       - Resources joined with ", " (emerald-300 or red-300).
       - `" ["` in slate-500, verbs joined in slate-300, `"]"` in slate-500.
       - If `api_groups[0] != ""`: ` in "g1, g2"` in slate-600.
       - If `resource_names` is non-empty: ` (only: a, b)` in amber-400.
  - Between entries: a 1px slate-800 bottom border with 8px vertical margin.

### 7.5 Tab 2: Role Browser (`rbac/roles.rs`)
- Height `window height - 200`. Row container with a 1px slate-800 border, radius 8, clipped.
- **Left pane**: 280px, right border slate-800, column.
  - Search area: padding 8, bottom border slate-800. A 32px field (radius 4, focus ring when focused): `search` 14px slate-500 at left 8; `TextInput` with padding-left 28, padding-right 12, `py 6`, transparent, 14/20 slate-200, placeholder "Filter roles..." slate-600, no border. Automation id `rbac.roles.filter`.
  - Filter: case-insensitive substring on the name only.
  - List scrolls. Groups (padding 4 each), shown only when non-empty:
    - Header: `px 8, py 4`, 10px uppercase tracking .05em slate-500 medium, gap 6. `shield-check` 12px "ClusterRoles (N)" and `shield` 12px "Roles (N)".
    - Item: full width, space-between, `px 8, py 6`, radius 4, hover bg `slate-700/40`, selected bg `slate-700/60`. Automation id `rbac.role.<kind>/<ns>/<name>`.
      - Name: 12px mono slate-300, truncated. Roles are prefixed with "{ns}/" in slate-600.
      - Rule count: 10px slate-600, margin-left 8.
  - Empty: padding 24, centered column, gap 8, 14px slate-500: `inbox` 20px + "No roles found".
- **Right pane**: flex 1, vertical scroll, padding 16.
  - Loading (shared loading flag): three static 48px-tall bars, radius 4, bg `slate-800/40`, gap 12.
  - Nothing selected: "Select a role to see who holds it", 14px slate-500, centered.
  - Result (sections gap 16):
    - Title row (gap 8): `shield-check` 16px purple-400; `"{role_kind}/{role_name}"` 14px mono slate-200; optional "(ns: X)" 12px slate-500.
    - "Rules (N)" (only if N > 0): label 10px uppercase tracking .05em slate-500 medium, margin-bottom 8. Rows (gap 4): `px 10, py 6`, radius 4, bg `purple-500/5`, 1px border `purple-500/10`, 12px mono. Resources in purple-300, ` [verbs]` with slate-500 brackets and slate-300 verbs, then the api groups like the rule chain (resource names are not shown).
    - "Subjects (N)": same label style.
      - Empty: "No subjects bound to this role", 14px slate-500 italic.
      - Rows (gap 4) are buttons: full width, row gap 8, `px 10, py 8`, radius 4, bg `slate-800/30`, 1px border slate-800, hover bg `slate-700/40`. Automation id `rbac.subject.<label>`.
        - Icon 14px: ServiceAccount `box` emerald-400, User `user` blue-400, Group `users` purple-400.
        - Name: 12px mono slate-200, truncated. Service accounts show as "ns/name".
        - Sub-line: 10px slate-500, `via {RoleBinding|ClusterRoleBinding} "{binding}" ({scope})`. Scope is "cluster-wide" or "namespace: X".
      - Click selects the identity and switches to the Matrix tab.

### 7.6 Tab 3: Query (`rbac/query.rs`)
- Column, gap 16. Controls row: wrap, gap 8, centered vertically.
  - Mode toggle: radius 6, 1px border `slate-700/50`, clipped. Two buttons `px 12, py 6`, 12px: "Check Permission" and "Who Can?". The second has a left border `slate-700/50`. Automation ids `rbac.query.mode.check|whocan`.
    - Active: bg `slate-700/60`, slate-200. Inactive: slate-500, hover slate-300 with bg `slate-800/50`.
    - Switching mode clears the results.
  - Identity picker (Check mode only). Its selection is separate from the matrix selection.
  - Verb and Resource selects (`render_select`): a compact popup button, 20px tall, width = longest option (12px sans) + 32.5, `pl 8, pr 6`, radius 5, bg surface, 1px border `slate-700/50`, 12px slate-200 value + `chevrons-up-down` 12px slate-200. Click toggles a menu below it (top 22): min-width = the button's, max-height 400, scrolls, `py 4`, radius 6, bg `#1f2633`, 1px border `white/12`, shadow-lg. Items: `px 10, py 2`, margin-x 4, radius 4, 13px slate-200, prefixed "✓ " when current; hover bg `#2563eb` and white text. Picking an item or a mouse-down outside closes it; Esc closes it. Automation ids `rbac.select.verb|resource` and `rbac.select.<id>.<option>`.
    - Verb options: get, list, watch, create, update, patch, delete. Default "get".
    - Resource options: the 22 matrix resources, in matrix order. Default "pods".
  - Namespace chip (if a namespace is set): `px 8, py 6`, radius 6, bg `slate-800/50`, 10px mono slate-500, "ns: {ns}".
  - **Check** button: row gap 6, `px 12, py 6`, radius 6, 12px medium. Icon `play` 14px (or `loader-2` 14px slate-600 spinning while loading), label "Check" in both modes. Automation id `rbac.query.check`.
    - Enabled: 1px `PREFLIGHT_BORDER` border, accent text and icon, no fill.
    - Disabled (loading, or Check mode without an identity): bg `slate-700/30`, slate-600, no border.
- Loading line: `loader-2` 16px slate-500 spinning + "Checking permissions...", 14px slate-500, gap 8.
- **Check result card** (fade-up 8, 300ms ease-out-quint): radius 8, 1px border, padding 16, max-width 672. Automation id `rbac.query.result`.
  - Allowed: border `emerald-500/20`, bg `emerald-500/5`, title "Allowed" 14px medium emerald-300, margin-bottom 8.
  - Denied: border `red-500/20`, bg `red-500/5`, title "Denied" in red-300.
  - Then the summary (12px slate-400, margin-bottom 12) and the rule chain.
- **Who-can results** (fade-up 8, max-width 672, gap 8):
  - Heading: `"{n} identities can {verb} {resource}[ in {ns}]:"`, 12px slate-500 medium.
  - List: max-height `window height - 320`, scrolls (6px scrollbar), gap 6, padding-right 4. Each row is a button: full width, row gap 8, `px 12, py 8`, radius 4, bg `emerald-500/5`, 1px border `emerald-500/15`, hover bg `emerald-500/10`. Automation id `rbac.whocan.<label>`.
    - An 8px dot in emerald-400.
    - Name: 12px mono emerald-200 (service accounts as ns/name).
    - Right-aligned: `via {rule_chain[0].role_name}`, 10px slate-500.
  - Click selects the identity and switches to the Matrix tab.
- Empty who-can (after a successful run): `No identities can {verb} {resource}[ in {ns}].`, 14px slate-500 italic.
- **Query error** **[FIXED]** (quirk 10.15; errors used to be swallowed): a failed Check or Who-can call shows a card (fade-up 8, max-width 672, radius 8, 1px border `red-500/20`, bg `red-500/5`, padding 16, row gap 8): `alert-circle` 14px red-400; "Query failed" 14px medium red-300 over the formatted error in 12px slate-400 (automation id `rbac.query.error`, label = the error). It clears on the next run or a mode switch. A 403 also feeds the Forbidden banner.

### 7.7 Identity picker (`rbac/picker.rs`)
- **Trigger**: row gap 8, `px 12, py 6`, radius 6, 1px border `slate-700/50` (hover slate-600), bg surface, 14px mono, min-width 200, max-width 320. Automation id `rbac.picker.<matrix|query|impersonation>`.
  - Text: slate-200 when an identity is selected, otherwise slate-500 "Select identity...".
  - Leading icon by kind (14px, only when selected): SA `box` emerald-400, User `user` blue-400, Group `users` purple-400.
  - Label: SA "ns/name", otherwise the name. Truncated.
- **Open** (trigger toggles): registers a modal id (`rbac-picker:<id>`) so Esc does not navigate back, focuses the search input and resets the list scroll. A mouse-down outside the panel (and not on the trigger) closes it.
- **Dropdown**: below the trigger (top 38), drawn above the page, width 320, max-height 360, glass, radius 8, clipped. Automation id `rbac.picker.<id>.panel`.
  - Search: padding 8, bottom border slate-800. A 32px field (radius 4, focus ring when focused) with `search` 14px slate-500 at left 8 and a `TextInput` (padding-left 28, padding-right 12, `py 6`, 14/20 slate-200, placeholder "Search identities..." slate-600).
  - Filter: SAs match by name or namespace; users and groups by name. Case-insensitive.
  - List: max-height 300, scrolls (6px scrollbar). Sections (padding 4 each), shown only when non-empty, with headers `px 8, py 4` in 10px uppercase tracking .05em slate-500 medium: "Service Accounts", "Users", "Groups".
  - Item: full width, row gap 8, `px 8, py 6`, radius 4, 14px, hover bg `slate-700/40`, selected bg `slate-700/60`. Automation id `rbac.picker.item.<label>`.
    - Icon, then mono slate-300 text. SA items show "{ns}/" in slate-500 followed by the name.
  - None: "No identities found", padding 16, centered, 14px slate-500.
  - Selecting an item emits the selection, closes the dropdown and clears the search. There is no arrow-key navigation. **[FIXED]** (quirk 10.22) Esc closes it and clears the search (handled by the RBAC view's window key listener, so it also works while the search field has focus); it never navigates back.

### 7.8 Forbidden banner (`views/rbac/forbidden.rs`)
**[FIXED]** (quirk 10.15; it was never reachable in the original app). A "Permission Denied" analysis banner shown wherever a cluster view in this spec hits a Kubernetes 403.
- **Component**: a `ForbiddenBanner` entity created with `rbac::forbidden_banner(id, cx)` (the returned subscription re-renders the owner). The owner reports its current error with `set_error(Option<&str>)`. When the message parses as a 403 (`kore_core::state::rbac::is_forbidden_message`: `User "<u>" cannot <verb> resource "<r>"[ ... namespace "<ns>"]`), it calls `Core::rbac_analyze_forbidden(message)` and renders the result. Reporting the same message again is a no-op (periodic refreshes keep a dismissal); a different message re-analyzes and un-dismisses; `None` hides it. It renders nothing for non-403 errors, while the analysis runs, after Dismiss, or if the analysis fails. `visible()` lets owners skip the slot.
- **Analysis** (`K8sState::rbac_analyze_forbidden`): the user (`system:serviceaccount:<ns>:<name>` becomes a ServiceAccount identity), verb, resource and namespace from the message; the closest matching rules for that identity from the RBAC snapshot; a suggestion `Create a RoleBinding in <ns>` / `Create a ClusterRoleBinding in the cluster` `that grants the '<verb>' verb on '<resource>' to <identity>`. **[FIXED]** If the RBAC objects cannot be read (the denied identity often cannot), the analysis still succeeds, without closest rules.
- **Look**: full width, radius 8, 1px border `red-500/20`, bg `red-500/5`, padding 16, sections gap 12, fade-up 12 over 0.3s.
  - Row (top-aligned, gap 12): `shield-x` 20px red-400 (margin-top 2); "Permission Denied" 14px medium red-300 over (margin-top 4) the 12px mono slate-400 line `{identity} cannot {verb} {resource}[ in namespace "{ns}"]` (identity: SA `ns/name`, else the name; wraps); a "Dismiss" text button (12px slate-500, hover slate-300, automation id `<id>.dismiss`).
  - "CLOSEST MATCHING RULES" (10px medium uppercase slate-500, margin-bottom 6) over a denied rule chain (7.4), only when there are any.
  - Suggestion row (top-aligned, gap 8, `px 12, py 8`, radius 4, bg `amber-500/5`, 1px border `amber-500/15`): `lightbulb` 14px amber-400, the suggestion in 12px `amber-200/80`.
  - Automation id `<id>`, label = the identity line.
- **Where it appears** (ids):
  - Network Policies (`np.forbidden`): under the stats bar (`px 16`, padding-top 12) when a refresh fails with a 403 while a graph is shown; under the full error state otherwise (padding 16).
  - CRD Browser (`crd.forbidden`): at the top of the right panel (`px 16`, padding-top 16) for a 403 from listing CRDs, listing instances, fetching an instance or a failed delete. A later successful CRD or instance list clears it.
  - Resource Graph (`graph.forbidden`) and Helm Releases (`helm.forbidden`, helm's stderr carries the API message): under the error message of the error state (margin-top 8, width 560).
  - RBAC Simulator (`rbac.forbidden`): top of the content (7.2).
  - The Dashboard is not wired: its backend treats list errors as empty lists (1.1), so no 403 reaches the view.

### 7.9 Tab 4: Impersonation and the global banner
- Column, max-width 512, sections gap 20.
- Copy (exact text), in a column with gap 12:
  - Title "What does impersonation do?" (14px slate-300 medium).
  - Body (12px slate-400, line height 19): "Impersonation lets you pick a ServiceAccount, User, or Group and see what they are allowed to do. When active, the Permission Matrix tab automatically shows that identity's permissions instead of requiring you to re-select it each time."
  - Box: radius 8, 1px border `slate-700/50`, bg `slate-800/30`, padding 12, gap 8.
    - Header "WHILE IMPERSONATING YOU CAN" (10px uppercase tracking .05em slate-500 medium).
    - Bullets (12px slate-400, gap 6; each an emerald-400 "•" with margin-top 2, gap 8):
      - "Browse the Permission Matrix pre-filled for the selected identity"
      - "Run permission queries scoped to that identity"
      - "Quickly check "would this identity be able to...?" without leaving your current view"
    - Footer: padding-top 4, top border `slate-700/30`, 10px `amber-500/70`: "Read-only simulation — all actual API calls still use your real credentials."
- "Select an identity to impersonate" (12px slate-500 medium), then the picker (gap 12). It is bound to the same selected identity as the matrix.
- If an identity is selected:
  - Impersonating: "Exit Impersonation": `px 16, py 8`, radius 8, bg `amber-500/10`, 1px border `amber-500/30`, amber-300, 14px, hover bg `amber-500/20`. It clears impersonation. Automation id `rbac.impersonation.exit`.
  - Otherwise: "Enter Impersonation Mode": the same box with accent text, `PREFLIGHT_BORDER` border, no fill. It calls `AppState::set_impersonation(Some(Impersonation { label, identity }))`. Automation id `rbac.impersonation.enter`.
- **Effects**:
  - The **impersonation banner** (`banners::impersonation_banner`) renders at the top of the main column in **every** view.
  - Whenever `AppState::impersonation` changes to an identity, the RBAC view's selected identity syncs to it (and the matrix rebuilds). Opening the view while impersonating preselects that identity.
  - Nothing else is impersonated: API calls are unchanged. Impersonation is not persisted.
- **Banner**: full width, row gap 12, `px 16, py 8`, bg `amber-500/10`, bottom border `amber-500/20`. Automation id `banner.impersonation`.
  - Contents: `shield-alert` 16px amber-400; "Impersonation Mode:" 12px amber-300; `viewing as {label}` 12px mono amber-200; "(simulated — actual API calls use your real credentials)" 10px `amber-500/70`, margin-left 4.
  - Spacer, then "Exit": row gap 4, `px 8, py 2`, radius 4, 12px amber-400, hover bg `amber-500/10`, `x` 12px. Automation id `banner.impersonation.exit`.
  - It appears and disappears instantly.

---

## 8. Network Policies view (`view = "network-policies"`, `views/network_policy.rs`)

This is a **chat-plus-policy-list UI**. There is no graph drawing: the backend graph is used only for counts, the AI context and the policy list.

### 8.1 Data
- `Core::build_network_policy_graph(namespace: Option<String>)` → `NetworkPolicyGraph`:
  - `groups`: pods grouped by top-level owner. RS→Deployment is resolved, so the id is `Deployment/ns/name`; other owners `{Kind}/ns/name`; ownerless pods `Pod/ns/name`. **Sorted by id.** Each group: `name`, `namespace`, `kind`, `pod_count`, `labels`, `is_isolated_ingress`, `is_isolated_egress`, `matching_policies`, `pods[{name, namespace, labels, status}]`.
  - `external_cidrs[{id, cidr, except, from_policy}]`.
  - `edges[{source, target, direction, ports[{port, protocol, end_port}], policy_name, policy_namespace}]`.
  - `policies[{name, namespace, pod_selector (matchLabels map), policy_types, ingress_rule_count, egress_rule_count, affected_pod_count}]`.
- Refresh triggers:
  - View creation, and when the namespace or the current context changes.
  - **2s debounce after the resource store changes while the table kind is Pods** (i.e. after events from the running pods watch).
  - A **30s interval**.
  - The Refresh button.
- `Core::simulate_network_traffic(source_namespace, source_pod, dest_namespace, dest_pod, port: Option<i32>, protocol: Option<String>)` → `{allowed, summary, ingress_evaluation:{isolated, policy_results[{policy_name, policy_namespace, selects_pod, allows_traffic, reason, matching_rule_index}]}, egress_evaluation:{...}}`.
  - Allowed = (not egress-isolated OR some egress policy allows) AND (the same for ingress).
  - Summary: `Traffic ALLOWED from ns/a to ns/b`, or `Traffic DENIED from ns/a to ns/b: egress denied and ingress denied` (listing only the denied directions).
  - Reasons: "Does not select ingress pod", "No matching ingress rule", "Matched ingress rule N", and the egress equivalents.

### 8.2 States
Checked in this order:
1. Loading with no graph yet: centered column, gap 12: `loader-2` 32px accent + "Loading network policies..." 14px slate-400.
2. Error **with no graph loaded** (first load, or after a namespace/context change, which drops the previous graph): `shield-x` 40px red-400; "Failed to load network policies" 14px medium red-400; the message 12px slate-500, max-width 448, centered (gap 12); plus the Forbidden banner (7.8) under it for a 403. **[FIXED]** (quirk 10.16) A failed refresh while a graph is loaded keeps the last good graph and shows a non-destructive indicator in the stats bar instead (8.3); the error clears on the next successful refresh.
3. Empty (no groups and no policies): `inbox` 48px slate-600; "No network policies found" 14px medium slate-400; then 12px slate-500 `No NetworkPolicies in "{ns}"`, or "Select a namespace to view network policies".

Background refreshes do not show a spinner. The loaded view fades in (300ms).

### 8.3 Layout
- Root: full-size column, relative, clipped. Automation id `view.network-policies`.
- **Stats bar**: `px 16, py 8`, bottom border `slate-800/50`, space-between.
  - Left (gap 16), each item 10px mono slate-500: `shield` 12px slate-500 + "{n} policies" (gap 6); "{n} workloads" (= groups); "{n} edges". Automation ids `np.stats.policies|workloads|edges` (label = the text).
  - Right (row gap 8):
    - Refresh error indicator (while the last refresh failed and a graph is shown): `alert-circle` 12px red-400 + `Refresh failed: {error}` 10px red-400, truncated at 360px, gap 4. Automation id `np.error` (label = the error).
    - Refresh icon button: `px 8, py 4`, radius 6, bg `surface/80`, 1px border slate-800, `refresh-cw` 12px slate-400 (hover slate-300). Automation id `np.refresh`.
  - Under the stats bar: the Forbidden banner (7.8) for a 403 refresh error (`px 16`, padding-top 12).
- **Main row**: flex 1, clipped.
  - Chat column (flex 1).
  - Policy panel (300px, or 0 when collapsed). The width changes instantly.

**Chat column**
- Messages: flex 1, vertical scroll (6px scrollbar), `px 16, py 12`, gap 16.
  - **Empty** (no messages): centered column filling the area, padding-x 24, centered text.
    - 56px tile (radius 16, no fill, margin-bottom 16) with `bot` 28px in accent at 60% (**[FIXED]** quirk 10.2; was slate-100).
    - "Ask about your network policies" 14px slate-400, margin-bottom 4.
    - `"Can X talk to Y?", "Which pods are isolated?", "What does this policy do?"` 12px slate-600, margin-bottom 24 (literal quotes included).
    - If AI is unconfigured: the config warning (6.5) inside a box with margin-bottom 24, radius 8, 1px border `amber-500/30`, max-width 384, clipped.
    - Suggested question chips (8.4): wrap, centered, gap 8, max-width 512. Each: `px 12, py 6`, radius 8, 1px border slate-700, 12px slate-300, hover text accent; disabled (streaming or unconfigured) opacity .4. Automation ids `np.question.<i>`.
  - Messages: compact bubbles with copy on assistant messages. A user message that carries a simulation result is followed by a right-aligned **simulation card** (8.6, margin-top 8).
  - "Thinking..." indicator (`loader-2` 14px accent + 12px slate-500, gap 8) while streaming and the last message is not from the assistant (it disappears once the first chunk arrives). **[FIXED]** (quirk 10.16) While a tool call runs it shows a `wrench` 14px accent icon first and the status text instead ("Querying get cluster health..."). Automation id `np.tool-status` (label = the text).
  - Auto-scroll to the bottom **only if the list was within 150px of the bottom** when a message or chunk arrived.
- **Suggestions strip** (when there are messages): `px 16, py 8`, top border `slate-800/30`, a horizontally scrolling row (gap 6) with a 6px horizontal scrollbar under it when it overflows. Chips: `px 10, py 4`, radius 6, 1px border slate-800, 10px slate-400, no wrap, hover text accent, disabled opacity .4. Automation ids `np.strip.<i>`.
- **Input form**: `px 16, py 12`, top border slate-800, bg `surface/80`. Row gap 8.
  - When there are messages: a leading clear button (padding 8, radius 8, `trash-2` 16px slate-500, hover red-400 with bg `muted/50`; opacity .4 and inert while streaming). Automation id `np.clear`.
  - Input box styled like the AI Panel's (38px, bg background, slate-800 border, radius 8 / 4 + focus ring, opacity .5 when disabled; 14/20 slate-100 text). Placeholder: "Configure an AI provider to get started..." / "Waiting for response..." / "Ask about network policies...". Enter sends (only when configured). The input loses focus when a send starts. Automation id `np.input`.
  - Send button as in the AI Panel (padding 8, radius 8). Automation id `np.send`.
  - Disclaimer (margin-top 6, 10px slate-600, centered): "AI responses may be inaccurate. Simulation results are computed locally."
  - Focus: the input is focused 300ms after the view is created.

**Policy panel**
- Wrapper: relative, full height, width 300 or 0.
- Toggle tab: at left -24, top 12, 24×32, bg `surface/90`, 1px slate-800 border on top, bottom and left, top-left and bottom-left radius 6. Icon 12px slate-500 (hover slate-300) rotated -90°: open `chevron-down` (points right); closed `chevron-right` (points up). Automation id `np.panel.toggle`.
- Panel (when open): 300px wide, full height, glass (bg `rgba(16,24,40,.7)`, white/6 border on all sides), column, clipped.
  - Header: padding 12, bottom border `slate-800/50`.
    - Title: 12px medium slate-300, margin-bottom 8, gap 6: `shield` 14px accent, "Network Policies ({n})".
    - Filter field: full width, 26.5px tall, bg background, 1px border slate-800, radius 4 (focus ring when focused). `TextInput` 11px mono slate-300, line height 16.5, padding left/right 24 and top/bottom 4, placeholder "Filter policies..." slate-600. `search` 12px slate-500 at left 7. An `x` 12px clear button at right 5 when non-empty (slate-500, hover slate-300). Automation id `np.filter`.
    - Filter matches name or namespace (case-insensitive).
  - List: padding 8, gap 6, scrolls.
  - None: "No matching policies", 10px slate-600, centered, padding 16 vertical.
- **Policy card**: full width, padding 10, radius 8, 1px border, pointer. Automation id `np.policy.<name>`; text ids `.ns` ("ns: {ns}"), `.types` ("Ingress, Egress"), `.selector` ("all pods" or space-joined `k=v`), `.rules` ("1 ingress rule, 1 egress rule").
  - Unselected: border `slate-800/50`, bg `background/50`, hover border slate-700.
  - Selected: `PREFLIGHT_BORDER` border, no fill.
  - Click toggles selection by name. This is visual only.
  - Rows:
    1. Row gap 6, margin-bottom 6: `shield` 12px accent; name 11px medium slate-200, truncated.
    2. "ns: {ns}" 10px slate-500, margin-bottom 6.
    3. Tags (wrap, gap 4, margin-bottom 6): per policy type, a 9px medium pill (`px 6, py 2`, radius 4): "Ingress" = `emerald-500/15` + emerald-400; anything else (Egress) = `blue-500/15` + blue-400. Then a pod-count pill (9px, slate-800 bg, slate-400 text): "{n} pod(s)" (singular when n == 1).
    4. Selector chips (wrap, gap 4; 9px mono, `px 6, py 2`, radius 4, bg `slate-800/80`, slate-400): "k=v" each, sorted by key; if the selector is empty, a single italic "all pods".
    5. Counts (margin-top 6, gap 8, 9px slate-500): "{n} ingress rule(s)" and "{n} egress rule(s)", each only if > 0 (plural when > 1).

### 8.4 Suggested questions (`suggested_questions`, computed from the graph, at most 4, in this order)
1. If there are at least 2 groups: `Can {groups[0].name} talk to {groups[1].name}?` (groups sorted by id).
2. If any group is ingress- or egress-isolated: "Which pods are isolated and why?"
3. If there are external CIDRs: "What external traffic is allowed?"
4. If there are policies: `What does {policies[0].name} do?`
5. Always: "Summarize the network security posture".

The result is truncated to 4. Chips do nothing while streaming or unconfigured.

### 8.5 Send flow (`send` / `finish_send`)
1. Ignore the send if there is no graph, the text is blank, or a stream is running. (The Enter key and chips additionally require a configured provider.)
2. **Simulation detection** (`detect_simulation`): case-insensitive regex `can\s+(.+?)\s+(?:talk|reach|connect|access|communicate|send)\s+(?:to|with)\s+(.+?)[\s?]*$` (not anchored at the start). Each side (trimmed, lowercased) resolves to a group by: an exact group-name match, else a group name containing the query, else a group with a pod whose name contains the query.
   - If both sides resolve to different groups, `simulate_network_traffic` runs from the first pod of the source group to the first pod of the destination group (no port/protocol) **before** the message is added; the result is attached to the user message (failures are ignored).
3. Append the user message, clear the input, start streaming.
4. Build the messages:
   - A `system` message: "You are a Kubernetes network policy expert assistant. …" plus `build_graph_context(graph)` (a markdown summary: "# Network Policy Graph Context", then "## Policies (n)", "## Workload Groups (n)", "## External CIDRs (n)", "## Traffic Edges (n)" sections), plus the instruction to explain simulation results, plus "Available workloads: …" and "Total pods: N".
   - The last 10 conversation messages. User messages with a simulation get `\n\n[Simulation Result: {src} → {dst}: ALLOWED|DENIED. {summary}]` appended.
5. `ai_stream::start_chat(config, AIChatRequest { messages, session_id: "np-ai-…", namespace })`.
   - Chunks append to (or create) the assistant message. Done ends. Error appends `\n\n[Error: msg]` (or adds `[Error: msg]`); a failed call adds `[Error: msg]`. Chunks, done and errors clear the tool status. **[FIXED]** Status events set the tool status shown by the "Thinking..." indicator (they used to be ignored).

### 8.6 Simulation card (`render_sim_card`)
- max-width 85%, radius 8, 1px border, 12px, clipped. Automation id `np.sim.<i>` (label ALLOWED/DENIED); text ids `.route` ("{src} → {dst}"), `.summary`, `.egress` / `.ingress` ("Egress: Isolated"), `.eval.<k>` ("ingress {policy}: {reason}").
  - Allowed: border `emerald-500/30`, bg `emerald-500/5`.
  - Denied: border `red-500/30`, bg `red-500/5`.
- Header: `px 12, py 8`, space-between, gap 12.
  - Left (mono slate-300, gap 8): `{src}` → `arrow-right` 14px slate-500 → `{dst}`, both truncated (group names).
  - Right: a badge (`px 8, py 2`, radius 4, semibold 10px, gap 4). Allowed: bg `emerald-500/20`, emerald-400, `check-circle-2` 12px, "ALLOWED". Denied: red-500/20, red-400, `x-circle`, "DENIED".
- Summary: `px 12`, padding-bottom 8, slate-400, wrapped.
- Direction summary row (`px 12`, padding-bottom 8, gap 12). "Egress:" and "Ingress:" in slate-500 (gap 4), each followed by one of:
  - "Isolated" (amber-400) if `isolated`;
  - "Allowed" (emerald-400 with `check-circle-2` 10px, gap 2) if any selecting policy allows;
  - "Denied" (red-400, `x-circle` 10px) if there are selecting policies but none allows;
  - "No policy" (slate-500) otherwise.
- If any policies select either pod:
  - Toggle: full width, `px 12, py 6`, 10px slate-500, hover slate-300, top border `slate-800/30`, gap 4: `chevron-down` (collapsed) or `chevron-up` (expanded) 12px, then "Policy details ({n} evaluations)". Automation id `np.sim.<i>.details` (label = the text).
  - Expanded: a column (`px 12`, padding-bottom 8, gap 4) that fades in over 200ms. Egress evaluations first, then ingress. Each item: `px 8, py 4`, radius 4, 10px. Allows: bg `emerald-500/10` + emerald-400; denies: red equivalents. Text: "egress" or "ingress" (slate-500), then ` {policy}: {reason}`.

---

## 9. Data-flow summary

| View (module) | `Core` calls | Streams / observers |
|---|---|---|
| Dashboard (`dashboard.rs`) | `get_cluster_health()`; `get_cluster_health_multi_cluster()` (every 30s; both also list NodeMetrics) | observes `AppState` (`multi_cluster`) |
| Resource Graph (`graph.rs`) | `build_resource_graph(namespace)` | observes `AppState` (namespace) |
| CRD Browser (`crd_browser.rs`) | `list_crds()`, `list_crd_instances(group, version, plural, namespace)`, `get_crd_instance(group, version, plural, namespace, name)`, `delete_crd_instance(...)` | `AppEvent::Escape`; namespace |
| Helm Releases (`helm_releases.rs`) | `helm_available()`, `list_helm_releases(namespace)` | namespace; window keys |
| Helm Detail (`helm_detail.rs`) | `get_helm_values(release, ns)`, `get_helm_manifest(...)`, `get_helm_history(...)`, `get_helm_notes(...)`, `rollback_helm_release(release, ns, revision)` | `AppEvent::Escape` |
| AI Settings (`ai_settings.rs`) | `check_providers_availability(ollama_base_url)`, `get_api_key(provider)` (via the config model), `store_api_key(provider, key)`, `delete_api_key(provider)`, `list_ollama_models(base_url)`, `list_claude_models()`, `list_cursor_agent_models()`, `ai_test_connection_checked(config)`; writes `kore-ai-config` through `AiConfigModel::set` | observes `AiConfigModel` |
| AI Panel (`ai_panel.rs`) | `ai_diagnose(config, DiagnoseRequest)` | `Subscription<AIStreamEvent>` (session global); observes `AiConfigModel` |
| AI Chat (`ai_chat.rs`) | `ai_chat(config, AIChatRequest)` | `Subscription<AIStreamEvent>`; observes `AiConfigModel` |
| Network Policies (`network_policy.rs`) | `build_network_policy_graph(namespace)`, `simulate_network_traffic(src_ns, src_pod, dst_ns, dst_pod, port, protocol)`, `ai_chat` (+ `rbac_analyze_forbidden` for a 403) | `ResourceStore` changes while kind = Pods (2s debounce); 30s timer; `Subscription<AIStreamEvent>`; observes `AppState`, `AiConfigModel` |
| RBAC (`rbac/`) | `rbac_list_identities()`, `rbac_build_matrix(identity, namespace)`, `rbac_list_roles(namespace)`, `rbac_reverse_lookup(kind, name, namespace)`, `rbac_check_permission(identity, verb, resource, namespace)`, `rbac_who_can(verb, resource, namespace)`, `rbac_analyze_forbidden(message)` (also used by CRD, Graph, Helm and Network Policies via `ForbiddenBanner`) | observes `AppState` (namespace, impersonation); window keys |

Each task or subscription is held in a view field; dropping the view cancels it.

View-local state:
- Graph filters, CRD collapse state and Helm sort are not persisted.
- AI chat histories are in memory only. The AI Panel's history survives close and reopen (app-global session); the others are lost when leaving the view.
- RBAC impersonation lives in `AppState::impersonation` and is not persisted.

---

## 10. Quirks and fixes inventory

Numbers are stable (other docs and code comments cite them as "quirk 10.N").

1. **[QUIRK]** No accent tints: transparent tiles and pills, `#e5e7eb` borders on several buttons, no accent hover or focus borders (section 0.3). Affects nearly every view here.
2. **[FIXED]** The 32px header tiles' `bot` icons render at 18px (were 24px); empty-state `bot` icons are accent at 60% (were slate-100). (0.3, 6.6, 6.7, 8.3)
3. **[QUIRK]** `.glass()` replaces co-located bg, border and shadow (cards, toasts, the policy panel, popovers).
4. **[FIXED]** A focused text field keeps its corner radius; the 2px accent/40 ring follows it (radius + ring offset).
5. **[FIXED] Deliberate Fix 11.** API keys are loaded from the keychain into the shared config model, so OpenAI and Anthropic work in AI Chat, the AI Panel and Network Policies.
6. **[FIXED] Deliberate Fix 12.** AI Test Connection reports HTTP 4xx/5xx as "Failed - Retry" with `Connection failed: HTTP …` (`ai_test_connection_checked`).
7. **[FIXED] Deliberate Fix 8.** Helm history accepts the integer `revision` and lists revisions; Rollback works.
8. **[FIXED]** The Helm Notes tab shows `helm get notes` (`Core::get_helm_notes`), not the values. Text and history panels fill the padded content area instead of overflowing by 32px, so the bottom edge is visible. (5.1, 5.2)
9. **[FIXED]** Helm "Updated" parses Helm's list format (`2026-10-16 19:02:43.123456 +0300 IDT`) as well as RFC 3339 and formats both as "Oct 16, 07:02 PM"; sorting by Updated is chronological. (4.3)
10. **[FIXED]** Cluster-scoped CRD instances are fetched and deleted without a namespace (scope from the CRD, `crd::instance_namespace`); the delete message omits the namespace. (3.1, 3.2)
11. **[FIXED]** A graph ReplicaSet click opens the owning Deployment (an orphan ReplicaSet shows an info toast). A pan that moves more than 4px is not a click. (0.4, 2.8)
12. **[FIXED]** Dashboard node bars show live metrics-server usage against allocatable (humanized; "?" without metrics); Pending ages and warning "last seen" are relative ("5m", "3m ago"); Recent Warnings are the 20 most recent. (1.1, 1.6, 1.8, 1.9)
13. **[FIXED]** The chat bubble's copy button is fully visible (it sits in an unclipped wrapper). (6.4)
14. **[FIXED]** Markdown links open in the system browser; unsafe URL schemes lose their link.
15. **[FIXED]** RBAC query errors are shown ("Query failed" card). The Forbidden banner (7.8) renders for 403s in Network Policies, CRD Browser, Graph, Helm Releases and RBAC (not the Dashboard, whose backend swallows list errors).
16. **[FIXED]** Network Policies keep the last good graph when a refresh fails ("Refresh failed: …" in the stats bar, Forbidden banner for a 403); AI status events show as "Querying …" in the Thinking indicator. (8.2, 8.3, 8.5)
17. **[FIXED]** (Fix 4) CRD Esc navigation uses `AppEvent::Escape`, so it does not fire while typing in the CRD search or while the AI Panel (or another modal) is open.
18. **[FIXED]** AI Panel: closing mid-stream no longer leaves the input disabled; the stream finishes in the app-global session.
19. **[FIXED] Deliberate Fix 9.** Multi-cluster dashboard calls time out per context after 5s, so an unreachable context does not delay the others.
20. **[FIXED] Deliberate Fix 13.** The Ollama availability probe uses the configured Base URL; OpenAI and Anthropic endpoints can be redirected with `KORE_OPENAI_BASE_URL` / `KORE_ANTHROPIC_BASE_URL` (used by the harness mock).
21. **[FIXED]** The Resource Graph shows the hover tooltip (name, kind, status, namespace) above the hovered node. (2.7)
22. **[QUIRK]** The confirm dialog ignores Esc and backdrop clicks. **[FIXED]** The RBAC identity picker closes on Esc (7.7).
23. **[FIXED]** CRD JSON highlighting no longer rewrites the text (`"x:12"` stayed `"x: 12"` before). (3.2)
24. **[FIXED]** AI Chat shows "Thinking..." between send and the first chunk or tool status (there was no indicator). (6.7)

---

## 11. Harness test scenarios (black-box, kind cluster)

The implemented scenarios live in `harness/scenarios/30-cluster-*.yaml` (see `harness/README.md`). They run against the kind clusters `kind-kore-harness` and `kind-kore-harness-2` created by `kore-harness cluster up` / `cluster seed`. Navigation uses the sidebar labels (AI Chat, Dashboard, Graph, Network Policies, RBAC, Custom Resources, Helm) or Cmd+K followed by the view name; the namespace is selected with the sidebar namespace dropdown (spec 01). Interactive elements carry the automation ids listed above.

### 11.1 Fixtures (`harness/cluster/fixtures`, chart `harness/cluster/charts/kore-demo`)

- `00-namespaces.yaml`: namespaces `kore-demo` and `kore-rbac`.
- `10-workloads.yaml` (namespace `kore-demo`): Deployments `web` (labels `app: web`) and `logger`; Pods `multi-container`, `crashloop` (CrashLoopBackOff, restarts, BackOff warnings) and `unschedulable` (Pending, `Unschedulable`, FailedScheduling warnings); Service `web`; Ingress `web` → Service `web`; ConfigMap `web-config`; Secret `web-secret`; Job `one-shot`; CronJob `ticker`.
- `20-crd.yaml` / `21-crd-instances.yaml`: Namespaced CRD `widgets.kore.dev` (kind `Widget`, group `kore.dev`, version `v1`) with instances `blue-widget` (`spec.color: blue`, `size: 3`) and `red-widget` in `kore-demo`.
- `30-network-policies.yaml`: `default-deny-ingress` (Ingress, all pods) and `allow-logger-to-web` (Ingress + Egress for `app=web`; ingress from logger on port 80; egress only to kube-system DNS).
- `40-rbac.yaml`: ServiceAccounts `kore-rbac/viewer-bot` and `kore-rbac/deployer-bot`; Role `kore-demo/pod-reader` (pods get/list/watch) bound to viewer-bot by RoleBinding `viewer-bot-pod-reader`; ClusterRole `kore-deployer` (2 rules, e.g. deployments get/list/watch/create/update/patch) bound by ClusterRoleBinding `deployer-bot-kore-deployer` to deployer-bot, User `alice@kore.dev` and Group `kore-devs`.
- Helm: release `kore-demo` in `kore-demo` from the `kore-demo` chart (`kore-demo-0.1.0`, app version `1.0`) with an install and an upgrade, so history has a superseded revision 1 ("Install complete", values `message: hello-from-helm`) and a deployed revision 2 ("Upgrade complete", `message: hello-from-helm-v2`). For the "not available" case, launch with a `PATH` that excludes helm.
- Multi-cluster: the second cluster `kind-kore-harness-2` (with a `remote-web` Deployment). `harness/cluster/kubeconfig-unreachable.yaml` adds an unreachable context `kore-unreachable` for the Fix 9 scenario.
- metrics-server (installed in `kind-kore-harness`, not in `kind-kore-harness-2`): the dashboard node bars show its NodeMetrics usage; without it they show "?" (the second cluster in multi-cluster mode). It also feeds the AI `get_pod_metrics` tool.

### 11.2 AI without a paid key
- The harness runs a deterministic mock provider (`crates/kore-harness/src/mock_ai.rs`) on `127.0.0.1:47900`, speaking the Ollama (`/api/tags`, `/api/chat`), OpenAI (`/v1/models`, `/v1/chat/completions`) and Anthropic (`/v1/messages`) protocols. Port **47901** answers every request with `HTTP 503 Service Unavailable` / `mock unavailable` (Fix 12 scenario).
- Scenarios select the **Ollama** provider and set Base URL to `http://127.0.0.1:47900` (the availability probe follows the configured URL, Fix 13). OpenAI/Anthropic can be pointed at the mock with `KORE_OPENAI_BASE_URL` / `KORE_ANTHROPIC_BASE_URL`.
- Mock contract:
  - `GET /api/tags` → `{"models":[{"name":"mock-model"}]}`.
  - `POST /api/chat`, `"stream": false` (AI Chat, Network Policies):
    - last user text contains "boom" → HTTP 500 with body `boom`; the UI shows `**Error:** AI provider returned HTTP 500 Internal Server Error: boom`.
    - contains "health" and no tool result yet → `{"message":{"role":"assistant","content":"","tool_calls":[{"function":{"name":"get_cluster_health","arguments":{}}}]},"done":true}` (exercises the "Querying get cluster health..." pill).
    - otherwise → the markdown answer `mock_ai::MARKDOWN_ANSWER`:
      ````text
      ## Summary\n\nCluster **looks** fine.\n\n| Pod | Status |\n|---|---|\n| web | Running |\n\n`inline` and\n```\ncode block\n```
      ````
  - `POST /api/chat`, `"stream": true` (AI Panel diagnose): NDJSON chunks `Diag`, `nosis `, `OK — KORE-MOCK-STREAM`, then `{"done":true}`.

### 11.3 Scenarios

**DASH-1: Single-cluster dashboard** (`30-cluster-dashboard.yaml`). Click "Dashboard". Expect 6 cards titled CLUSTER HEALTH, POD HEALTH, NODE STATUS, RESTART HOTLIST, PENDING PODS, RECENT WARNINGS; the score ring with a label; the Pod Health legend with "Running", "Pending", "CrashLooping" counts and "Total"; the control-plane node "Ready" in green with CPU/Mem bars; `crashloop` in the Restart Hotlist with rank 1; `unschedulable` in Pending Pods with reason "Unschedulable" in amber; a "BackOff" or "FailedScheduling" warning.

**DASH-1b: Usage and ages** (`30-cluster-bug-dashboard-usage.yaml`, quirk 10.12 fixed). Node bars read like "108m / 12" and "1.7Gi / 13.6Gi" (metrics-server usage over allocatable); Pending Pods show "Nm"-style ages and warnings "N… ago" (no ISO timestamps). With Multi-Cluster on, `kind-kore-harness-2` (no metrics-server) shows "? / 12".

**DASH-2: Breakdown** (`30-cluster-dashboard-breakdown.yaml`). Click "How is this calculated?". Expect "Node health", "Pod health", "Base score +30.0", the penalty rows, "Total" equal to the ring number, and the footnote. Click "Hide breakdown": it collapses.

**DASH-3: Navigate** (`30-cluster-dashboard-navigate.yaml`). Clicking a pod in the Restart Hotlist, and one in Pending Pods, opens that pod's details view.

**DASH-4: Multi-cluster** (`30-cluster-multicluster.yaml`). Toggle Multi-Cluster in the sidebar and open Dashboard. Expect sections headed `kind-kore-harness` and `kind-kore-harness-2`, each with a pill like "NN - Healthy". (The Deployments table across all namespaces includes `remote-web`.) Toggle it off again.

**DASH-4b: Multi-cluster timeout** (`30-cluster-multicluster-timeout-fix.yaml`, Fix 9). With the unreachable context layered into `KUBECONFIG`, both reachable clusters appear within about 12s.

**DASH-5: Loading and error.** Point the kubeconfig at an unreachable server before opening Dashboard. Expect "Loading cluster health..." and then, if the call fails, the error text with a "Retry" link.

**GRAPH-1: Layout** (`30-cluster-graph.yaml`). Select `kore-demo` and click "Graph". Expect the badge "N nodes · M edges" at the top-left, "Fit" plus "NN%" at the top-right, the kind chips (Ingress, Service, Deployment, CronJob, ReplicaSet, Job, Pod) and the 7-kind legend. Rows top to bottom: Ingress `web`; Service `web`; Deployments and CronJob `ticker`; ReplicaSets and Jobs; Pods. Dashed edges Ingress→Service and Service→web pods; solid Deployment→RS→Pod edges.

**GRAPH-2: Hover.** Hover the Service node: its edges turn accent, unrelated nodes dim to 30%, the node gets a status-color glow, and the tooltip shows its name, kind, status and "ns: kore-demo" (`30-cluster-bug-graph-replicaset-tooltip.yaml`, quirk 10.21 fixed).

**GRAPH-3/4: Filters** (`30-cluster-graph-filter.yaml`). Click the "Pod" chip: it greys out and pod nodes dim except those 1 hop from a direct match (60%). Type a name in "Filter by name...": matches stay bright, neighbors at 60%, the rest nearly invisible; the layout does not move. The X clears the filter.

**GRAPH-5: Zoom and pan** (`30-cluster-graph.yaml`). Scroll up over the canvas: the percentage grows about 8% per notch and node names become readable. Drag the background: the content moves. Click "Fit": the graph is centered and the percentage is ≤150%.

**GRAPH-6: Click-through** (`30-cluster-graph-filter.yaml`). Click a Pod node: the Pod details view opens. Clicking a ReplicaSet node (`web-<hash>`) opens the `web` Deployment (`30-cluster-bug-graph-replicaset-tooltip.yaml`, quirk 10.11 fixed).

**GRAPH-7: Empty.** Select a namespace without workloads: "No resources found" and `No resources in namespace "x"`.

**CRD-1: Browse** (`30-cluster-crd.yaml`). Select `kore-demo` and click "Custom Resources". Expect "Custom Resources", "N CRDs discovered", the group "KORE.DEV" with "Widget". Type "wid" in "Search CRDs...": Widget remains. Click it: header "Widget", "kore.dev/v1 - Namespaced", the count, and a table (NAME, NAMESPACE, AGE) with `blue-widget` and `red-widget`.

**CRD-2: Detail** (`30-cluster-crd.yaml`). Click `blue-widget`. Expect "Back to list | blue-widget" and highlighted JSON (`"color": "blue"` green, `"size": 3` amber, keys in accent, `"kind": "Widget"`). Esc returns to the list; Esc again deselects the CRD; Esc again returns to the resource table.

**CRD-3: Delete** (`30-cluster-crd-delete.yaml`). On a row, click the trash icon. Expect the dialog "Delete Widget" with the name/namespace message. Click "Delete": toast "Resource deleted" and the row disappears. The fixture is re-applied.

**CRD-4: Cluster-scoped** (`30-cluster-bug-crd-cluster-scoped.yaml`, quirk 10.10 fixed). The scenario applies a temporary cluster-scoped CRD `gadgets.kore-bug.dev` (kind Gadget) with instance `bug-gadget`. The browser shows "kore-bug.dev/v1 - Cluster"; opening `bug-gadget` shows its JSON (no "Error fetching instance"); the trash icon asks `Are you sure you want to delete "bug-gadget"? …` (no namespace) and deletes it ("Resource deleted"). The CRD is deleted at the end.

**HELM-1: List** (`30-cluster-helm.yaml`). Click "Helm". Expect columns NAME through UPDATED, the row `kore-demo` (namespace `kore-demo`, revision 2, a green "deployed" badge with a pulsing dot, chart `kore-demo-0.1.0`, app version `1.0`). Click "REVISION" then "NAME": sort icons switch. Press ↓: the row highlights; Enter opens the detail.

**HELM-2: Detail tabs** (`30-cluster-helm.yaml`). Expect Back, "Release", "Namespace", the tabs and Copy. Values shows highlighted YAML (`message: hello-from-helm-v2`, `replicaCount: 1`). "Manifest" shows `# Source: kore-demo/templates/…` comment lines in slate with a ConfigMap and a Deployment. "Notes" shows "No notes available for this release." (the chart has no NOTES.txt; quirk 10.8 fixed). Esc returns to the list.

**HELM-2b: Notes and dates** (`30-cluster-bug-helm-notes-date.yaml`, quirks 10.8 / 10.9 fixed). A temporary release `kore-bug-notes` with a NOTES.txt is installed: its Notes tab shows the rendered notes ("KORE-HELM-NOTES for kore-bug-notes"), not the values; list and history Updated cells read like "Oct 16, 07:02 PM" (no raw `+0300 IDT` timestamps). The release is uninstalled at the end.

**HELM-3: History and rollback** (`30-cluster-helm-history-fix.yaml`, Fix 8). The History tab lists REVISION … ACTIONS with "superseded" and "deployed" rows, "Install complete" / "Upgrade complete", a "latest" pill on revision 2 and a "Rollback" button on revision 1. Click it: the yellow dialog "Rollback Helm Release" mentions "to revision 1". Confirm: the release values return to revision-1 content and a success toast appears.

**HELM-4: No helm.** Launch with helm missing from `PATH` and open Helm: "Helm Not Available".

**AI-1: Settings** (`30-cluster-ai-settings.yaml`). Open Settings. The "AI Provider" section shows 5 segments ("Claude Code" / "Cursor Agent" show "not found" if those CLIs are absent). Click "Ollama": Model and Base URL appear; point Base URL at the mock and the Model becomes a select with "mock-model". "Test Connection" shows "Connected" and the toast "Connection successful". Click "OpenAI" (no stored key): an "API Key" field appears, Test Connection is disabled, and AI Chat shows the config warning. Ends with Ollama → mock configured.

**AI-1b: Test Connection errors** (`30-cluster-ai-test-connection-fix.yaml`, Fix 12). Base URL `http://127.0.0.1:47901`: Test Connection shows "Failed - Retry" and a toast starting "Connection failed".

**AI-1c: Ollama probe** (`30-cluster-ai-ollama-probe-fix.yaml`, Fix 13). Base URL pointed at a closed port: after leaving and reopening Settings, Ollama shows "not found".

**AI-2: Chat with a tool** (`30-cluster-ai-chat.yaml`). In AI Chat expect "What can I help you with?" and 5 suggestion buttons. Click "Cluster health": the user message appears right-aligned; the pill "Querying get cluster health..." with a wrench and spinner appears; then the assistant bubble renders the markdown (H2 "Summary", bold "looks", a table with a header row, light-cyan inline code, a dark code block). Hovering the bubble reveals the copy affordance at its top-right; clicking it toasts "Copied to clipboard". Type "hello", Shift+Enter, "world", Enter: one user bubble with two lines.

**AI-3: Unconfigured.** With OpenAI and no key: AI Chat shows the amber "Configure an AI provider in Settings to use this feature", the placeholder "Configure an AI provider to get started...", and disabled suggestions.

**AI-4: AI Panel diagnose** (`30-cluster-ai-panel.yaml`). Open a pod's details and click the header "AI" button (or Cmd+K → "Ask AI"). The panel slides in with "AI Assistant", "Context: pods/kore-demo/<pod>" and the quick actions. Click "Diagnose": "Thinking..." appears and the text streams to "Diagnosis OK — KORE-MOCK-STREAM". Close (X or Esc) and reopen: the conversation is still there. The trash icon clears it.

**AI-5: Error path** (`30-cluster-ai-error.yaml`). Send a message containing "boom" in AI Chat: the bubble shows "**Error:** AI provider returned HTTP 500 …" with "Error:" in bold.

**RBAC-1: Matrix** (`30-cluster-rbac-matrix.yaml`). Select `kore-demo`, press Cmd+Shift+R (or click "RBAC"). Expect "RBAC Simulator" and tabs with Kbd 1–4. Open "Select identity...": sections list `kore-rbac/viewer-bot`, `kore-rbac/deployer-bot`, `alice@kore.dev`, `kore-devs`. Pick `kore-rbac/viewer-bot`: the legend and table appear; the pods row has get, list and watch green and the rest red. Click the pods/get cell: a popover "pods / get" with an "Allowed" pill shows RoleBinding / viewer-bot-pod-reader → Role / pod-reader (ns: kore-demo) → `pods [get, list, watch]`. Esc closes it. "Copy CSV" copies text starting `Resource,get,list,...` (no toast).

**RBAC-1b: User and Group** (`30-cluster-rbac-identities.yaml`). For `alice@kore.dev` (and `kore-devs`), the deployments/create cell's chain shows ClusterRoleBinding deployer-bot-kore-deployer → ClusterRole kore-deployer → `deployments [get, list, watch, create, update, patch]`.

**RBAC-2: Role Browser** (`30-cluster-rbac-roles.yaml`). Press `2`, type "kore-deployer" in "Filter roles..." and click it. Expect "ClusterRole/kore-deployer", "RULES (2)" and "SUBJECTS (3)" (deployer-bot, alice@kore.dev, kore-devs) each `via ClusterRoleBinding "deployer-bot-kore-deployer" (cluster-wide)`. Clicking a subject jumps to the Matrix tab for that identity.

**RBAC-3: Query check** (`30-cluster-rbac-query.yaml`). Press `3`. Pick `kore-rbac/viewer-bot`, verb "get", resource "pods", click "Check": a green "Allowed" card with "Allowed because of Role 'pod-reader' via RoleBinding 'viewer-bot-pod-reader'" and the chain. With verb "delete": a red "Denied" card `ServiceAccount "kore-rbac/viewer-bot" cannot delete pods in kore-demo` and "No matching rule found".

**RBAC-4: Who can** (`30-cluster-rbac-query.yaml`). "Who Can?", verb "get", resource "pods", Check: "N identities can get pods in kore-demo:" with a row `kore-rbac/viewer-bot` "via pod-reader".

**RBAC-4b: Picker Esc** (`30-cluster-bug-rbac-picker-esc.yaml`, quirk 10.22 fixed). With the identity picker open and its search focused, Esc closes it and the view stays on RBAC (no modals left open).

**RBAC-4c: Forbidden** (`30-cluster-bug-forbidden.yaml`, quirks 10.15 / 10.16 fixed, 7.8). The app runs with a generated token kubeconfig for `kore-rbac/kore-bug-bot` (ClusterRole `view`, an RBAC-read ClusterRole, and a Role granting only `get pods` in kore-demo). Network Policies: revoking `view` makes the next 30s refresh fail; "2 policies" and the cards stay, "Refresh failed: …" appears with the "Permission Denied" banner (`kore-rbac/kore-bug-bot cannot list … in namespace "kore-demo"`, "Create a RoleBinding in kore-demo …"); Dismiss hides the banner; re-granting clears the error. Custom Resources: the banner names `customresourcedefinitions`. RBAC: with RBAC read revoked, "Who Can?" shows "Query failed" with the error. All objects and the kubeconfig are removed.

**RBAC-5: Impersonation** (`30-cluster-rbac-impersonation.yaml`). Press `4`. The explanation is shown. Select `alice@kore.dev` and click "Enter Impersonation Mode": the amber banner "Impersonation Mode: viewing as alice@kore.dev" appears at the top and persists on Dashboard. The banner's "Exit" (or `i` in the RBAC view) removes it.

**NP-1: Policy list** (`30-cluster-netpol.yaml`). Select `kore-demo` and click "Network Policies". Expect "2 policies", "N workloads", "N edges"; the panel "Network Policies (2)" with `default-deny-ingress` (Ingress pill, "all pods") and `allow-logger-to-web` (Ingress + Egress pills, chip `app=web`, "1 ingress rule", "1 egress rule"). Typing in "Filter policies..." narrows the cards; the side tab collapses and expands the panel; the suggested question chips are shown.

**NP-2: Suggestions and simulation** (`30-cluster-netpol-simulate.yaml`, mock AI). Send "can logger talk to web": the user bubble is followed by a card "logger → web" with the red "DENIED" badge (every pod is ingress-isolated and the simulation carries no port, so the port-80 rule does not match), the Egress/Ingress summary and "Policy details (N evaluations)", which expands to the per-policy reasons. "can web talk to logger" is DENIED with egress denied too. The mock reply renders below each question.

**NP-2b: Tool status** (`30-cluster-bug-netpol-status.yaml`, quirk 10.16 fixed). A question containing "health" makes the mock call `get_cluster_health`: "Querying get cluster health..." shows until the answer renders.

**NP-3: Empty.** Select a namespace with no pods and no policies: "No network policies found".

Screenshot checkpoints: capture after each scenario's final step. For pixel diffing, wait at least 2s after the dashboard loads (the donut animation starts at 0.4s and lasts 1.5s) and at least 0.4s after any transition. Mask time-dependent text (ages, timestamps, pulsing dots, spinners).
