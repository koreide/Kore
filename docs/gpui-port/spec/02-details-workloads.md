# 02 — Details & Workloads

Scope: everything rendered after a resource row is opened (`AppView::Details`), plus the workload tools hosted inside those views.

The behavior was originally ported from the legacy Tauri/React app (git history at `3a66870`); this document describes the native GPUI app, and the code wins wherever the two disagree.

Modules covered (all under `crates/kore-app/src/`):
- `views/pod_details.rs`, `views/deployment_details.rs`, `views/resource_details.rs` (the three detail views);
- `views/detail_ui.rs` (shared detail primitives: `Kbd` chip, toolbar buttons, tabs bar, skeletons, info cells, `time_ago`, enter/fade animations);
- `views/describe_content.rs`, `ui/text_search_bar.rs`;
- `views/yaml_editor.rs`, `views/yaml_text_area.rs`;
- `views/pod_logs.rs`, `views/multi_pod_logs.rs`, `views/log_view.rs`;
- `views/exec_terminal.rs`, `views/terminal_model.rs`;
- `views/pod_metrics.rs`, `views/port_forward.rs`, `views/deployment_rollback.rs`, `views/debug_container.rs`, `views/events_timeline.rs`;
- shared UI: `ui/confirm_dialog.rs`, `ui/toast.rs`, `ui/native_controls.rs`, `ui/scrollbar.rs`, `ui/widgets.rs`, `ui/mono_text.rs`, `ui/input.rs`, `theme.rs`.

Backend: the `kore_core::Core` methods listed in §14, implemented in `crates/kore-core/src/state/{logs,multi_logs,exec,debug,metrics,yaml,rollback,resources,port_forward,event_store}.rs` and `constants.rs`.

> Conventions in this document
> - All sizes are logical px as rendered. Text sizes use the `KoreStyled` helpers in `theme.rs`: `text_xs_tw` = 12px/16px line height, `text_sm_tw` = 14px/20px, `text_base_tw` = 16px/24px, `text_lg_tw` = 18px/28px, `text_xl_tw` = 20px/28px, `text_2xl_tw` = 24px/32px; `text_10()` / `text_9()` set size only (line height inherits). Root text is 16px with line height 1.5.
> - "Caps" labels are `ui::text::caps(text, size)`: upper-cased with 0.05em tracking (GPUI has no letter-spacing, so it is emulated per glyph).
> - Radii: 2px, 4px, 6px, 8px, or full (pill/circle). Weights: medium = 500, semibold = 600, bold = 700. Borders are 1px unless stated.
> - Colors are written as hex plus the `theme.rs` token where one exists (`slate::S400` = `#94a3b8`, etc.). `X @ NN%` means color X at NN% alpha (`theme::hexa(X, 0.NN)`).
> - **Accent tints.** In the reference design, accent-opacity borders (`accent @ 30/50%`) render as the gray `theme::PREFLIGHT_BORDER` (`#e5e7eb`) and accent-opacity backgrounds are absent. The native app reproduces that look everywhere: wherever this document says "active style" or "gray accent border", the border is `#e5e7eb`, the text/icon is `theme::accent(cx)`, and there is no tinted background. Solid `accent` borders (e.g. the port-forward Start button) are real accent.
> - "Quirk" marks behavior kept from the reference design that looks unintended; each one is flagged so the owner can choose to change it. "Deliberate Fix N" marks behavior the native app intentionally corrected (see `harness/scenarios/*fix*.yaml`); the fixed behavior is the spec. Former quirks of this area that were defects (static header status, clipped card bottoms, overlapping wrapped log rows, numbers highlighted inside JSON strings, ×1000 plain memory quantities, the missing Events toggle, the `app=<name>` deployment-log selector) are fixed; their regression scenarios are `harness/scenarios/20-details-bug-*.yaml` (tag `bugfix`).
> - Nothing in this area is persisted: all detail-view state (tab, log options, port-forward list, search) lives in the view entity and is discarded when the route changes (root drops the view). Backend port forwards keep running.

---

## 0. Shared design tokens and primitives

### 0.1 Color tokens (resolved)

| Token | Value |
|---|---|
| `theme::BACKGROUND` | `#0b1221` |
| `theme::SURFACE` | `#101828` |
| `theme::MUTED` | `#1d2939` |
| `theme::accent(cx)` (user-configurable in Settings; `DEFAULT_ACCENT`) | `#58d0ff` |
| status running / pending / failed | `#10b981` (`emerald::E500`) / `#f59e0b` (`amber::A500`) / `#ef4444` (`red::R500`) |
| default text (`slate::S100`) | `#f1f5f9` |
| `slate::S200` … `S900` | `#e2e8f0` / `#cbd5e1` / `#94a3b8` / `#64748b` / `#475569` / `#334155` / `#1e293b` / `#0f172a` |
| `emerald::E300` / `E400` / `E500`; emerald-600 / 700 (hex) | `#6ee7b7` / `#34d399` / `#10b981`; `#059669` / `#047857` |
| green-300 / 400 / 500 (hex; `log_view::GREEN_400`) | `#86efac` / `#4ade80` / `#22c55e` |
| `red::R300` / `R400` / `R500` / `R600` / `R700`; red-800 | `#fca5a5` / `#f87171` / `#ef4444` / `#dc2626` / `#b91c1c`; `#991b1b` |
| `amber::A200` / `A400` / `A500`; amber-600 / 800 | `#fde68a` / `#fbbf24` / `#f59e0b`; `#d97706` / `#92400e` |
| yellow-400 / `misc::YELLOW500` / 600 / 700 | `#facc15` / `#eab308` / `#ca8a04` / `#a16207` |
| `misc::ORANGE400` / `ORANGE500` | `#fb923c` / `#f97316` |
| `misc::BLUE400`; blue-500 / 600 / 700 | `#60a5fa`; `#3b82f6` / `#2563eb` / `#1d4ed8` |
| indigo-400 (hex) | `#818cf8` |
| `theme::PREFLIGHT_BORDER` | `#e5e7eb` |

Common composites over `#0b1221`:
- `SURFACE @ 50%` gives `#0e1524`
- `SURFACE @ 30%` gives `#0c1423`
- black @ 40% gives `#070b14`
- `MUTED @ 30%` gives `#101928`
- black @ 60% (modal backdrop) gives `#04070d`

Additional surfaces:
- The reference design's full-window noise overlay (1.5% opacity) is not reproduced.
- Scrollbars (`ui::scrollbar`): 6px (`scrollbar::WIDTH`), transparent track, thumb `#334155` with radius 3px, hover `#475569`. They take layout space: scroll areas add 6px right padding while `Gutter::overflows()`. Minimum thumb length 20px.
- Focus ring (where used): `Theme::focus_ring()` = accent @ 40%.
- Native-looking form controls are drawn by `ui::native_controls`: checkbox (checked: fill = `SYSTEM_BLUE` `#3478f6` or the accent where noted, white checkmark; unchecked: `#1e1e1e` fill with a 1.5px `#9a9a9a` ring, radius 27% of size), radio (checked: fill + white center dot 36% of size; unchecked as the checkbox), `NativeSelect` (popup menu with a chevrons glyph), and the number-input stepper glyph. Checkboxes and radios in the debug modal use the accent; every other checkbox uses `SYSTEM_BLUE`.

Fonts (`theme::SANS`, `theme::MONO`):
- Sans: the system UI font (`.SystemUIFont` on macOS, Segoe UI on Windows, Inter elsewhere).
- Mono: `.AppleSystemUIFontMonospaced` (SF Mono) on macOS, Consolas on Windows, DejaVu Sans Mono elsewhere.
- Single-line mono text that must line up is drawn with `ui::mono_text::mono_text` at the 0.601em pitch (`theme::MONO_ADVANCE_EM`). Wrapped mono uses `mono_paragraph` or `describe_content::pitched_mono_size` (font scaled so columns match the pitch).

### 0.2 Animations

GPUI `with_animation` reproductions:
- **ping** (`detail_ui::ping_dot`): repeating 1s; a halo grows to 2× and fades from 0.75 to 0 over the first 75% of each cycle (ease-out cubic), over a solid dot.
- **spin** (`widgets::spinning_icon`): 1s linear rotation, repeating.
- **pulse**: 2s, opacity 1 → 0.5 → 1, repeating (metrics loading icon, debug-modal loading bug).
- **Skeleton shimmer** (`widgets::skeleton_bar`): a band from `#1e293b @ 40%` to `#334155 @ 40%` and back sweeps across the bar over 1.8s ease-in-out, repeating. Radius 4px.
- **Detail view enter** (`detail_ui::enter_anim`): opacity 0→1 and x offset +20px→0 over 300ms (ease-out). There is no exit animation; the view is dropped when the route changes.
- **Tab content switch** (`detail_ui::fade_in`): the new pane fades in (opacity 0→1, 200ms). The old pane is dropped immediately (no fade-out).
- **Tab underline** (`detail_ui::tabs_bar`): a 2px accent bar, rounded-full, spanning the full active tab width, positioned 1px below the bar (over its bottom border). It slides from the previous tab over 280ms with a spring-like ease (stiffness 500, damping 35 approximation, slight overshoot).

### 0.3 `Kbd` chip (`detail_ui::kbd_chip`, on `widgets::kbd`)

- Padding 6px horizontal, 2px vertical.
- Background `#1e293b @ 80%`, radius 4px.
- 10px mono text, `slate::S400` `#94a3b8`.
- Border `#334155 @ 50%`.
- Margin-left 6px.
- Inside the 14px detail buttons it takes the 20px line height, so the chip is 26px tall and sizes the button.

Used for `Esc`, `B` and `D`. The log search button uses the bare `widgets::kbd` with `keys::MOD_F` (`⌘F` on macOS, `Ctrl+F` elsewhere).

### 0.4 Standard bordered toolbar button (`detail_ui::button`, `log_view::tool_button`)

- Shape: horizontal flex, centered, gap 6px, radius 6px, 1px border. Padding and text size are set per use.
- `BtnStyle::Standard`: border `#1e293b` (`S800`), text `#cbd5e1` (`S300`). Hover: background `MUTED @ 30%`. The log/multi-log toolbar variant (`tool_button`) also turns the border `PREFLIGHT_BORDER` on hover.
- `BtnStyle::Active` (toggled): border `PREFLIGHT_BORDER`, accent text and icon, no background, no hover change.
- `BtnStyle::Danger`: border `#991b1b @ 50%`, text `#f87171`. Hover: border `#dc2626`, background `#ef4444 @ 15%`.
- Disabled / busy: opacity 0.5. Cursor not-allowed where noted.
- Icons do not inherit color: each icon is given the button's text color explicitly.

### 0.5 ConfirmDialog (`ui/confirm_dialog.rs`; spec 01 §10)

Used for: delete pod, delete deployment, restart deployment, delete generic resource, apply YAML, rollback revision. Opened with `AppState::confirm(ConfirmRequest { … })`; `on_confirm` must call `close_confirm` itself.

Backdrop:
- Covers the window, black @ 60%, 16px padding, centers the dialog. There is no blur.
- Clicking the backdrop does **not** close it. Esc does nothing while it is open (it counts as a modal, so Esc never navigates back underneath it; Deliberate Fix 4).

Dialog container:
- Full width up to 448px, radius 8px, shadow `0 20px 25px -5px black@10%, 0 8px 10px -6px black@10%`.
- Background: `SURFACE` `#101828` for danger and info; `#1b180d` for warning (the composite of the yellow tint over the dimmed page).
- Enter animation: opacity 0→1 and a 20px downward offset → 0, 250ms ease-out.
- Variant colors:

| Variant | Border | Icon bubble | Icon | Confirm button bg / hover / border |
|---|---|---|---|---|
| Danger | `#ef4444 @ 50%` | `#ef4444 @ 20%` | `#f87171` | `#dc2626` / `#b91c1c` / `#ef4444` |
| Warning | `#eab308 @ 50%` | `#eab308 @ 20%` | `#facc15` | `#ca8a04` / `#a16207` / `#eab308` |
| Info | `#3b82f6 @ 50%` | `#3b82f6 @ 20%` | `#60a5fa` | `#2563eb` / `#1d4ed8` / `#3b82f6` |

Inner layout (padding 24px):
- Row (items start, gap 16px, margin-bottom 16px):
  1. Icon bubble: padding 8px, rounded-full, containing Lucide `alert-triangle` 24px.
  2. Text column: title 18px/28px semibold `#f1f5f9`, margin-bottom 8px; message 14px `#cbd5e1`.
  3. Close button: `x` 20px `#94a3b8`, hover `#e2e8f0`. Closes the dialog.
- Footer (justify end, gap 12px, margin-top 24px):
  - Cancel: padding 16px × 8px, radius 4px, border `S800`, 14px `#f1f5f9`. Hover: border `S700`, background `MUTED @ 40%`.
  - Confirm: same size, white text, variant colors above.

Behavior: the confirm button runs `on_confirm`; whether the dialog closes depends on the caller (documented per use below).

### 0.6 Toasts (`ui/toast.rs`; spec 01 §11)

- Stack: bottom 16px, right 16px, vertical gap 8px, above everything.
- Each toast:
  - Padding 16px × 10px, radius 8px, max width 320px, 14px text `#e2e8f0`.
  - `.glass()`: background `#101828 @ 70%`, border white @ 6%, shadow `0 0 0 1px white@2%, 0 4px 24px black@30%`. The border is the glass border for every type.
  - Layout: `[icon 16px] message (shrink-to-fit) [x 14px #64748b]`, gap 10px.
- Types (`ToastKind`):

| Type | Icon |
|---|---|
| Success | `check-circle-2` `#34d399` |
| Error | `x-circle` `#f87171` |
| Info | `info` accent |

- Lifetime: auto-dismisses after 3000ms; click to dismiss.
- Animation: in from opacity 0 with a 16px offset, 200ms ease-out.

### 0.7 TextSearchBar (`ui/text_search_bar.rs`) — used by DescribeContent, YamlEditor and the log views

Container (positions itself; the owner places it in a relative container):
- Absolute at top 8px, right 24px. Horizontal flex, gap 4px.
- Background `SURFACE` `#101828`, border `#334155`, radius 8px, padding 12px × 6px.
- Shadow `0 10px 15px -3px black@10%, 0 4px 6px -4px black@10%`.

Contents, in order:
1. `search` icon 14px, `#64748b`.
2. Input (`ui::input::TextInput`): 160px × 16px, 12px `#e2e8f0`. Placeholder `Search...` in `#475569`.
3. If the query is non-empty and there are 0 matches: `No results`, 10px, `#f87171 @ 70%`, margin 4px horizontal.
4. If there are matches: `{current+1}/{count}`, 10px, `#64748b`, margin 4px horizontal.
5. `chevron-up` 14px button (previous).
6. `chevron-down` 14px button (next).
7. `x` 14px button (close), margin-left 4px.

Buttons 5–7 are `#94a3b8`, hover `#e2e8f0`.

Behavior:
- The owner computes matches from `SearchBarEvent::Query` and reports the position with `set_matches(current, count)`.
- Autofocuses the input 50ms after it appears.
- Enter emits Next; Shift+Enter emits Prev.
- Esc while the input is focused emits Close (closes the bar). Because the input is a registered text entry, the same Esc does **not** navigate back (Deliberate Fix 4).
- Esc when the input is not focused goes to the view's `AppEvent::Escape` handler (per view below).
- ⌘F / Ctrl+F while the bar is already open refocuses its input.

---

## 1. Placement inside the app shell

- `root.rs` routes `AppView::Details` by the current kind, keyed by `(kind, namespace, name)`:
  - `ResourceKind::Pods` creates `PodDetailsView`.
  - `ResourceKind::Deployments` creates `DeploymentDetailsView`.
  - Any other kind (services, nodes, events, configmaps, secrets, ingresses, jobs, cronjobs, namespaces) creates `ResourceDetailsView`.
- The app header stays visible above the detail view. Its breadcrumb reads `{KindLabel} › {name}` (`chevron-right` 14px `#475569`; name 14px medium `#f1f5f9`, max width 200px, truncated). The global search input is also shown; `/` focuses it. Spec 01 owns the header.
- The detail view fills the region below the header. The main column has 16px padding and a 12px gap between header and view.
- Global shortcuts that also apply in details (`root.rs::on_key`; ignored while a registered text entry has focus):
  - `h` calls `AppState::back()` (returns to the table). Not while a modal, palette or overlay is open.
  - `?` toggles the shortcut overlay.
  - `⌘⇧R` / `Ctrl+Shift+R` opens RBAC; `⌘R` refreshes resources.
  - Esc: root emits `AppEvent::Escape` only when no palette, overlay, modal, dropdown or text entry is active (Deliberate Fix 4). Each detail view subscribes to it (per view below).
- Detail views always open on their default tab: Logs for pods, Describe for everything else. Row actions (`logs`, `exec`, `port-forward`, `describe`, `scale`, `restart`) just open the details view; they do **not** preselect a tab or panel.
- Entering details: click a row, press `l` on the selected row, use a command-palette result, a pinned resource, a dashboard link, or a graph node (`AppState::open_resource`).
- Leaving details:
  - Back button, Esc or `h` calls `AppState::back()`: view becomes the table and `selected` is cleared.
  - Root drops the view entity. All its state is discarded and every stream it owns stops (dropping a `Subscription` cancels the backend stream). Backend port forwards keep running (§10.2).

`ResourceItem` fields used: `name`, `namespace`, `status`, `node`, `ip`, `ready` (for example `"3/3"`), `up_to_date`, `available`. Every view treats an empty namespace as `"default"`; this matters for nodes and namespaces.

---

## 2. PodDetailsView (`views/pod_details.rs`)

### 2.1 Layout skeleton

```
┌──────────────────────────────────────────────────────────────┬──────────────┐
│ HEADER  [← Esc]  pod-name  (● Running)  › details   [Debug B][Delete D][Ports]│ PORT FORWARD │
│ (collapsible info row: Namespace: x │ Node: y │ IP: z)         │  SIDEBAR     │
│ (debug banner when a debug container is attached)             │  (0 or 320px)│
├──────────────────────────────────────────────────────────────┤              │
│ TABS  Logs  Describe  YAML  Metrics  Events  Shell(●)          │              │
├──────────────────────────────────────────────────────────────┤              │
│ LOGS TOOLBAR (logs tab only)                                  │              │
├──────────────────────────────────────────────────────────────┤              │
│ CONTENT (flex-1, overflow hidden, relative)                   │              │
│ (delete error strip)                                          │              │
└──────────────────────────────────────────────────────────────┴──────────────┘
```

- Root: full size, horizontal flex, background `BACKGROUND` `#0b1221`, with the enter animation (§0.2). Automation id `view.details`.
- Left column: `flex_1`, `min_w_0`, vertical flex: header, tabs bar, content (`flex_1`, relative, overflow hidden), optional delete error strip.
- Right column: the port-forward sidebar host (§2.11).
- Each tab's content is a separate entity created when the tab is entered and dropped when it is left (`enter_tab`), so its streams stop. The Describe text and the log options are kept in the pod view and survive tab switches.

### 2.2 Header

Container:
- Bottom border `#1e293b`, background `SURFACE @ 50%` (composite `#0e1524`).

Main row (horizontal, centered, 16px horizontal / 10px vertical padding, 12px gap):

1. **Back button** (`BtnStyle::Standard`)
   - Padding 10px × 4px, radius 6px, border `S800`, 14px `#cbd5e1`. Hover: background `MUTED @ 30%`.
   - Content: `arrow-left` 14px, then `Kbd` "Esc". There is no "Back" text in the pod view.
   - Automation id `details.back`, label "Go back to resource list".
2. **Center block** (gap 10px, `min_w_0`, `flex_1`):
   - Pod name: mono 14px `#f1f5f9` (`mono_text`), clipped.
   - **StatusBadge** (§2.2.1).
   - **Details toggle**: `chevron-right` 14px plus the text `details`, 12px `#64748b`, hover `#cbd5e1` (icon too), gap 4px. The chevron is rotated 90° while expanded (no rotation animation).
3. **Action group** (only when the pod is not deleted; gap 6px). Each button: padding 10px × 4px, radius 6px, 1px border, 14px text, icon 14px, gap 6px.
   - **Debug button** (`bug`):
     - No debug container and not a static pod: active style (gray accent border, accent text and icon, no hover change). Label `Debug`, followed by `Kbd B`.
     - Debug container active: border `#10b981 @ 50%`, background `#10b981 @ 15%`, text and icon `#34d399`. Label `Stop Debug` (`Stopping…` while stopping, at opacity 0.5). Instead of the Kbd it shows an 8px `#34d399` ping dot.
     - Static pod (annotation `kubernetes.io/config.mirror` present) and no debug container: border `S800`, text and icon `#475569`, cursor not-allowed, no hover, disabled (`b` also does nothing). Automation label "Static pods do not support ephemeral debug containers".
     - Other automation labels: `Debug container`, and `Stop debug container "{name}"` when one is active. (Native hover tooltips are not shown.)
     - Ignored while stopping.
   - **Delete button** (`trash-2`): `BtnStyle::Danger`. Label `Delete` plus `Kbd D`. Opacity 0.5 and inert while deleting.
   - **Ports button** (`plug`), label `Ports`:
     - Default: `BtnStyle::Standard`.
     - Sidebar open, no active forwards: active style.
     - At least one active (LIVE) forward: emerald styling (as the active Debug button) plus an 8px `#34d399` ping dot after the label.

**Collapsible pod info row** (when "details" is expanded):
- Animation: opacity 0→1 over 150ms (no height animation).
- Row: horizontal, gap 24px, padding 16px × 8px, top border `#1e293b @ 50%`, 12px text.
- Three pairs: `Namespace:`, `Node:`, `IP:`. `Node` is the node name (`spec.nodeName`, from the describe call and the live watch); `IP` is the pod IP.
  - Label `#64748b`; value mono `#e2e8f0`; gap 6px.
- Separated by a 1px × 12px divider in `#1e293b`.
- Missing or empty values show `Unknown`.

**Debug banner** (when a debug container is attached):
- Row: padding 16px × 8px, top border `#1e293b @ 50%`, background `#10b981 @ 5%`, gap 8px.
- Contents:
  1. `bug` 14px `#34d399`.
  2. `Debug:` 12px medium `#34d399`.
  3. Container name 12px `#e2e8f0`.
  4. Image chip: 10px mono `#34d399 @ 70%`, background `#10b981 @ 10%`, padding 6px × 2px, radius 4px.

#### 2.2.1 StatusBadge

Shape:
- Pill: horizontal, gap 6px, padding 8px × 2px, rounded-full, 12px medium.
- Dot: 6px circle.

Variant is chosen from the lower-cased status (`resource_item::status_variant`, colors from `widgets::status_colors`):

| Variant | Statuses | Dot | Text | Background |
|---|---|---|---|---|
| running | running, ready, succeeded, completed, complete | `#34d399` **with ping halo** | `#34d399` | `#10b981 @ 10%` |
| pending | pending, containercreating, init, waiting | `#fbbf24` | `#fbbf24` | `#f59e0b @ 10%` |
| failed | failed, crashloopbackoff, error, imagepullbackoff, evicted, oomkilled | `#f87171` | `#f87171` | `#ef4444 @ 10%` |
| terminating | terminating | `#fb923c` | `#fb923c` | `#f97316 @ 10%` |
| deleted | (set locally, §2.12) | `#f87171` | `#f87171` | `#ef4444 @ 10%` |
| default | anything else | `#94a3b8` | `#cbd5e1` | `#64748b @ 10%` |

The badge text is the raw status string, for example `Ready`. Missing status shows `Unknown`. After deletion the badge shows `Deleted`. Automation id `pod.status`.

The status is live: the view watches pods in its namespace (`detail_ui::watch_item`, a `Core::start_watch` subscription owned by the view) and replaces its `ResourceItem` with the newest version of this pod, so the badge (and node/IP) follow the pod while the view is open (e.g. `Ready` → `NotReady` when its container exits). The watch stops when the view is dropped or the pod is marked deleted.

### 2.3 Tabs bar (`detail_ui::tabs_bar`)

- Hidden when the pod is deleted.
- Bar: horizontal, bottom border `#1e293b`.
- Tab button: padding 16px × 10px, 14px text, gap 6px.
  - Inactive: `#94a3b8`, hover `#e2e8f0`.
  - Active: accent, plus the sliding underline (§0.2).
- Automation ids `tab.<id>` (`tab.logs`, `tab.describe`, `tab.yaml`, `tab.metrics`, `tab.events`, `tab.shell`).
- Order and number keys:

| Key | Tab |
|---|---|
| 1 | Logs |
| 2 | Describe |
| 3 | YAML |
| 4 | Metrics |
| 5 | Events |
| 6 | Shell |

- The Shell tab shows an 8px accent ping dot after its label while a debug container is attached.

### 2.4 Keyboard (pod view, `keys::on_window_key` + `AppEvent::Escape`)

All view keys are ignored while a registered text entry has focus (search input, YAML text area, terminal, port inputs).
- **Esc** (`AppEvent::Escape`, so only when no modal/dropdown/text entry is active): if the log search is open, close it and clear the query; otherwise go back. With the ConfirmDialog or the debug modal open, Esc does not navigate back (Deliberate Fix 4); the debug modal closes itself on Esc (§11).
- **⌘F / Ctrl+F on the Logs tab**: open the log search (or refocus it after 50ms). On Describe and YAML, ⌘F is handled by DescribeContent / YamlEditor; those handlers exist only while their tab is shown.
- **`d`/`D`**: open the delete confirm. Conditions: not deleted, no confirm open, no modal open, debug modal closed. Modifiers are not checked, so ⌘D also triggers it.
- **`b`/`B`**: same conditions as `d`. If a debug container is active, stop it; else, if not a static pod, open the debug modal.
- **1–6** (no modifiers, no shift): switch tab.
- The terminal is a registered text entry, so while it has focus Esc, digits, `h`, `b`, `d` all go to the shell (Esc is sent as `\x1b`) and never navigate or switch tabs.

### 2.5 Logs tab (`views/pod_logs.rs`, `views/log_view.rs`)

#### 2.5.1 Logs toolbar

- Row: horizontal, gap 8px, padding 16px × 6px, bottom border `#1e293b @ 50%`, background `SURFACE @ 30%`, 12px text.
- Left side, in order:
  1. **Container select** (`NativeSelect`): only when the pod has more than one container, counting init containers.
     - Padding 8px × 1px, radius 6px, border `S800`, background `SURFACE @ 60%`, 12px `#cbd5e1`.
     - Options: `All containers` (value `""`, the default), then `init:{name}` for each init container, then each regular container name (from `Core::describe_pod`).
  2. **Previous** checkbox chip (`log_view::check_chip`): a 12px `SYSTEM_BLUE` checkbox and the text "Previous", gap 6px. Padding 8px × 4px, radius 6px, border `S800`, 12px `#cbd5e1`. Hover: border `PREFLIGHT_BORDER`, background `MUTED @ 30%`.
  3. **Auto-scroll** checkbox chip: same style, checked by default.
- Spacer (`flex_1`).
- Right side, in order. Each `tool_button`: padding 8px × 4px, 12px text, 14px icon.
  1. **Copy** (`copy` + "Copy").
  2. **Download** (`download` + "Download").
  3. **Search** (`search` + Kbd `⌘F`/`Ctrl+F`, no text label). Active style while the search is open.
- The container / Previous / Auto-scroll options are stored in the pod view (`LogOptions`) and restored when the Logs tab is re-entered.

#### 2.5.2 Content card

- Card (`log_view::content_card`): background black @ 40%, border `S800`, radius 8px, absolutely placed with a 16px inset on every side (top/left/right/bottom), so the whole card, bottom border and rounded corners included, is visible.
- States:
  - **Loading skeleton** (`log_view::log_skeleton`): shown while loading and the buffer is empty.
    - 12 bars, each 12px tall, 10px gap, padding 16px.
    - Widths: 80, 65, 90, 45, 75, 55, 85, 40, 70, 60, 88, 50 (% of the inner width).
    - Opacity `1 - i*0.06`.
    - Visible only until the log subscription is established.
  - **Empty**: `No logs available`, padding 16px, 12px mono `#64748b` (automation id `logs.empty`). Shown after the stream connects with no lines yet.
  - **Lines**: a virtualized variable-height GPUI `list` (`ListState`, 400px overdraw). A single-line row is 20px (`log_view::ROW_H`: 1px top padding + the 19px line); a wrapped line takes `1 + 19 × rows` px, so it never overlaps the next row. Measured heights are cached; appends splice only the changed last row and the new rows (`pod_logs::append_rows`), and a width change re-measures. Text 12px mono with a 19px line height (`log_view::LINE_H`). A 6px painted scrollbar on the right (`log_view::list_scrollbar`, draggable). Automation id `logs.list`.

#### 2.5.3 Log row

Row:
- Horizontal. Hover background white @ 2%.
- Search matches: current match row `#fbbf24 @ 10%`; other matching rows `#fbbf24 @ 5%`.

Line-number gutter (`log_view::gutter_cell`):
- Width 48px, right-aligned, padding-right 12px, 1px top padding; it spans the full (wrapped) row height.
- Right border `#1e293b @ 50%`.
- Text `#475569`; `#64748b` on row hover.
- Shows `index+1`.

Text cell:
- Padding-left 12px, 1px top padding, `flex_1`, `min_w_0`; text wraps (`mono_paragraph`) at the 0.601em pitch and the row grows with it (minimum 20px).

Line color (`log_view::level_color(line, true)`; first match wins; tested on the upper-cased line with word boundaries):

| Word | Color |
|---|---|
| `ERROR`, `FATAL` or `PANIC` | `#f87171` |
| `WARN` or `WARNING` | `#fbbf24` |
| `INFO` | `#60a5fa` |
| `DEBUG` or `TRACE` | `#64748b` |
| none of the above | **`#4ade80`** |

Word boundaries mean `ERRORS`, `INFORMATION` and `x_INFO` do not match.

Search highlight inside text: every case-insensitive occurrence is drawn with background `#fbbf24 @ 30%` and text `#fde68a`.

Data handling (`log_view::LogBuffer`):
- The buffer behaves as one string split on `\n`.
- Because every streamed line ends with `\n`, there is always a trailing empty row with its own line number. For example, 200 lines render as 201 rows.

#### 2.5.4 Log stream behavior

- The stream runs while the Logs tab is shown and the pod is not deleted. It restarts (buffer cleared) when the selected container or `Previous` changes, and a new one starts each time the Logs tab is entered.
- Start sequence:
  1. Drop the old subscription (cancels its backend stream), clear the buffer, set loading.
  2. `Core::start_pod_logs_stream(namespace, pod, container: Option<String>, previous: Some(bool))` returns `Subscription<PodLogPayload>`. The `init:` prefix is stripped from the container; `None` means all containers.
  3. Drain the subscription in a `cx.spawn` loop, coalescing up to 2048 payloads per UI update.
- Backend behavior (`state/logs.rs`):
  - Tail of 200 lines (`DEFAULT_LOG_TAIL_LINES`).
  - `follow` is true unless `previous` is set.
  - All-containers mode streams **regular containers only** (not init) in parallel. When the pod has more than one container, every line is prefixed `[{container}] `.
  - Each call returns an independent subscription; dropping it stops that stream. `Core::stop_pod_logs_stream`, a context switch and shutdown cancel every log stream.
- Payload `PodLogPayload { logs: Option<String>, error: Option<String>, append: Option<bool> }`:
  - `error`: append `\n[ERROR] {error}\n` to the buffer (rendered red by the ERROR rule).
  - `logs` with `append: true`: append to the buffer.
  - `logs` without `append`: replace the buffer.
  - `previous` with no previous instance (single-container mode): the backend sends `No previous logs — this container has not been restarted.\n` (replace). In all-containers mode this case is silently skipped and the view shows "No logs available".
- Start failure: the buffer is set to `Error starting log stream: {err}`.
- **Auto-scroll**: when on, every buffer change scrolls the last row to the bottom. Scrolling up manually does not turn it off; only the checkbox does. Re-checking it scrolls to the end immediately.
- **Copy**: writes the whole buffer to the clipboard. Toast `Copied to clipboard` (success).
- **Download**: writes `{podName}-{ISO timestamp with ':' and '.' replaced by '-'}.log`, for example `nginx-abc-2026-10-15T10-11-12-123Z.log`, into the user's Downloads folder (`dirs::download_dir()`, else `~/Downloads`; created if missing), with no dialog. Toast `Logs downloaded` (success). A write failure is logged and shows the error toast `Failed to download logs: could not write {file}`.

#### 2.5.5 Log search overlay

`TextSearchBar` in logs mode (§0.7), with these differences:
- Placeholder `Search logs...`.
- There is **no** "No results" label; the counter appears only when there are matches.

Behavior:
- Matches are row indexes whose text contains the query (case-insensitive).
- Typing resets the index to 0 but does **not** scroll.
- Enter / Shift+Enter (or the chevrons) moves to the next or previous match, wraps around, and scrolls that row to the center of the list (the row is first scrolled to the top so it is laid out, then centered from its measured bounds in the same frame).
- Esc in the input, the X, or Esc via §2.4 closes the overlay and clears the query.
- The overlay sits at the top-right of the content area (top 8px, right 24px), over the card.

### 2.6 Describe tab

- Card: background `SURFACE @ 30%`, border `S800`, radius 8px, absolutely placed with a 16px inset on every side (fully visible, as the logs card), overflow hidden.
- Data: `Core::describe_pod(namespace, pod)`, fetched once when the view opens (not refetched on tab switch). The pod JSON is pretty-printed with 2-space indentation (`resource_details::pretty`; keys sorted by kore-core).
- Loading: DescribeSkeleton (`detail_ui::describe_skeleton`), padding 16px, vertical gap 12px:
  - Bar 16px tall × 128px wide.
  - A group indented 16px with 8px gaps: three 12px bars at 192px, 224px and 160px.
  - Another 16px bar × 112px with 16px top margin.
  - An indented group: 12px bars at 256px, 208px, 176px and 240px.
- Error: text `Error fetching pod details: {err}`, rendered through DescribeContent.
- The same describe call extracts the container names (init containers prefixed `init:`) and the static-pod flag. On success it then calls `Core::list_debug_containers` and restores the **last running** debug container into the debug state (banner and button).
- The DescribeContent entity exists only while the Describe tab is shown (its ⌘F handler is window-level).

#### 2.6.1 DescribeContent (`views/describe_content.rs`)

- Scroll area: full size, vertical scroll, padding 16px (plus 6px on the right while the scrollbar shows), mono at `pitched_mono_size(12)` (the font scaled so wrapped columns match the 0.601em pitch) with a 19px line height, wrapping.
- Base text color `#f1f5f9`; this covers braces, brackets, commas and array scalars.
- Empty content shows `No details available`.
- JSON highlighting (`highlight_nodes`, a JSON-aware tokenizer; the text is shown exactly as given):

| Token | Color |
|---|---|
| Object keys `"key"` before `:` (quotes included) | accent |
| String values after `: ` | `#34d399` |
| Number values after `: ` | `#fbbf24` |
| `true` / `false` after `: ` | `#818cf8` |
| `null` after `: ` | `#64748b` |

  Array elements (not preceded by `:`) stay default color. Text is shown literally. String literals are opaque tokens: digits or colons inside a string (`"2026-10-15T10:41:26Z"`, `"nginx:1.27"`) are never highlighted as numbers and nothing is inserted. Numbers include a leading `-`, decimals and exponents.
- Search: ⌘F / Ctrl+F opens the TextSearchBar.
  - Matching is case-insensitive, per highlighted token (a match never spans a key/value boundary).
  - Current match background `#fbbf24 @ 35%`; other matches `#fbbf24 @ 15%`. Text keeps its syntax color.
  - The current match is scrolled to the vertical center (instant), including on every query change.
  - Next and previous wrap around.
  - The index resets when the query or the content changes. Close clears the query.
- Automation id `describe`.

### 2.7 YAML tab

`YamlEditor::new(app, ResourceKind::Pods, ns, name)` filling the content area (§5).

### 2.8 Metrics tab

`PodMetrics` filling the content area (§7).

### 2.9 Events tab

`EventsTimeline` with kind `"pods"` (§9).

### 2.10 Shell tab

- `ExecTerminal` filling the content area (§8).
- The container passed to the terminal (`pod_details::shell_container`), in priority order:
  1. Active debug container name.
  2. Selected log container (with `init:` stripped).
  3. First non-init container.
  4. `None`.
- The terminal is recreated (new session) whenever that resolved container changes while the Shell tab is shown, e.g. when a debug container becomes ready.
- While the Shell tab is active, the port-forward sidebar is **dropped** (§2.11).

### 2.11 Port-forward sidebar host

- Rendered only when the pod is not deleted and the active tab is not Shell.
- Wrapper: `flex_none`, full height, overflow hidden, width 320px when open and 0 when closed. Toggling animates width 0↔320px over 200ms ease-in-out with opacity = width/320. Before the first toggle there is no animation (closed, width 0).
- The `PortForwardPanel` entity (§10) is created with the view and stays alive while the sidebar is collapsed, so its list is kept, except when switching to Shell, which drops it; leaving Shell creates a fresh, empty one.
- The pod view observes the panel's active (LIVE) count to color the Ports button.
  - After a re-creation via the Shell tab the list is empty and the count is 0, even though backend forwards are still alive (they are not re-listed).

### 2.12 Deleted state

- Trigger (`check_deleted`): the view observes the shared `ResourceStore`. When the store's settled (not loading) query is a single-cluster pods list for this pod's namespace (or `*`), and the pod was present in it once and is now gone, the view marks itself deleted. This requires the pods table watch to be the store's current query (the normal path when the pod was opened from the Pods table).
- Effects:
  - Header actions hidden.
  - Badge shows `Deleted` (red).
  - Tabs, toolbar and port sidebar hidden.
  - Every tab entity (logs, YAML, metrics, events, shell, ports) and the debug modal are dropped, stopping their streams.
- Content: card centered horizontally and vertically. Enter animation: opacity 0→1 and y offset 20px→0 over 300ms.
  - Card: margin 16px, max width 448px, padding 32px, background `SURFACE @ 30%`, border `#ef4444 @ 50%`, radius 8px, centered column.
  - `alert-circle` 48px `#f87171`, margin-bottom 16px.
  - Title `Pod No Longer Exists`: 20px/28px semibold `#f87171`, margin-bottom 8px.
  - Body (`#cbd5e1`, centered, margin-bottom 16px): `The pod {podName} in namespace {namespace} has been deleted.` The name and namespace are mono accent.
  - Button `Go Back` (`BtnStyle::Standard`, text `#f1f5f9`): padding 16px × 8px, 14px. Calls back.
- Deleting from this view: Delete (or `d`), then confirm "Delete Pod" (danger).
  - Message: `Are you sure you want to delete pod "{pod}" in namespace "{ns}"? This action cannot be undone.`
  - Buttons: Delete / Cancel.
  - Calls `Core::delete_resource(ResourceKind::Pods, namespace, name)`.
  - On success: the dialog closes and the view stays. No toast. Once the watch drops the pod, the Deleted state appears. With a controller-managed pod (Deployment) the replacement pod has a new name, so this view still shows Deleted.
  - On failure: error toast with the message, the dialog stays open, and a strip appears below the content: margin 16px horizontal and 8px bottom, padding 8px, background `#ef4444 @ 10%`, border `#ef4444 @ 50%`, radius 4px, 12px `#f87171` text `Failed to delete pod: {msg}`.

### 2.13 Debug container flow (pod view side)

- **Open**: Debug button or `b` creates a fresh `DebugContainerModal` entity (§11).
- **On ready** (`DebugEvent::Created`):
  - Set debug state `{name, image}` and drop the modal.
  - Toast `Debug container "{name}" is ready` (success).
  - **Switch to the Shell tab.** The terminal attaches to the debug container with `/bin/sh` by default.
- **Cancel** (`DebugEvent::Cancelled`): drop the modal.
- **Stop**: button or `b` while active calls `Core::stop_debug_container(namespace, pod, container)` (the backend writes `exit` to the container's stdin and waits up to 10s for it to stop).
  - Success: clear the state and toast `Debug container stopped`.
  - Failure: toast `Failed to stop debug container: {msg}` (error).

---

## 3. DeploymentDetailsView (`views/deployment_details.rs`)

### 3.1 Layout

- Root: full size, vertical flex, background `#0b1221`, with the enter animation (§0.2). Automation id `view.details`.
- **Header**: padding 16px, bottom border `S800`, background `SURFACE @ 50%`.

Header row 1 (space-between, centered, margin-bottom 16px):
- **Left: Back button** (`resource_details::back_button`)
  - `BtnStyle::Standard`, padding 12px × 6px, radius 6px, 14px `#cbd5e1`, gap 8px.
  - Content: `arrow-left` 16px, text `Back`, `Kbd Esc`.
- **Right group** (gap 8px):
  1. **Scale stepper**:
     - Wrapper: border `S800`, radius 6px, overflow hidden, inner gap 4px.
     - Minus button (`minus` 14px `#cbd5e1`): padding 8px × 6px, hover background `MUTED @ 30%`.
     - Number field: 48px × 32px, left and right borders `S800`, transparent, mono 14px `#f1f5f9`, the number centered in the space left of a `chevrons-up-down` 12px `#94a3b8` glyph pinned 3px from the right (the number-input spin glyph, always shown). ↑/↓ while focused step by ±1 (minimum 0). On blur the field re-renders the parsed value.
     - Plus button (`plus` 14px), same style as Minus.
     - Minus clamps at 0. Typing parses a leading integer; non-numeric gives 0; negative gives 0.
     - Minus and Plus are dimmed (opacity 0.5) and inert while scaling.
  2. **Scale** button:
     - Active style (gray accent border, accent text, no background), padding 12px × 6px, 14px.
     - Label `Scale`; `Scaling...` while busy, at opacity 0.5.
  3. **Restart** button:
     - `BtnStyle::Standard`, padding 12px × 6px, gap 8px, 14px. Hover: border `#f59e0b @ 50%`, background `#f59e0b @ 10%`.
     - Content: `refresh-cw` 16px (spinning while restarting, at opacity 0.5) and the text `Restart`.
  4. **Delete** button: `BtnStyle::Danger`, padding 12px × 6px, gap 8px, `trash-2` 16px, text `Delete`. There is **no** Kbd and **no** `d` shortcut in this view.

Header row 2 (info grid): 5 columns when the window is at least 768px wide, else 2; gap 16px. Each cell (`detail_ui::info_cell`):
- Label: 10px caps, `#64748b`, line height 20px, margin-bottom 4px.
- Value: mono 12px `#f1f5f9`, clipped.
- Cells, in order:
  1. `Name`
  2. `Namespace`
  3. `Ready` (`ready` or `-`)
  4. `Up-to-date` (`up_to_date` or `-`)
  5. `Available` (`available` or `-`)

**Tabs**: same styling as the pod view (§2.3). Order and number keys:

| Key | Tab |
|---|---|
| 1 | Describe (default) |
| 2 | YAML |
| 3 | Rollback |
| 4 | Logs |
| 5 | Events |

**Content**: `flex_1`, relative, overflow hidden, with the tab fade (§0.2). Every tab entity is recreated on entry and dropped on exit.
- **Describe**: card as in the pod view (`SURFACE @ 30%`, border, radius 8px, inset 16px on every side).
  - Re-fetched with `Core::describe_resource(ResourceKind::Deployments, namespace, name)` **every time the Describe tab is activated**.
  - Skeleton: a 16px × 128px bar, then an indented group of 12px bars at 192px and 224px.
  - Error: `Error: {err}`.
  - Content: DescribeContent.
- **YAML**: YamlEditor with `ResourceKind::Deployments`.
- **Rollback**: DeploymentRollback (§6).
- **Logs**: MultiPodLogs for the Deployment (`MultiPodLogs::for_deployment`): it fetches the Deployment (`Core::describe_resource`) and builds the label selector from its real `spec.selector` (`multi_pod_logs::label_selector_string`). See §4.
- **Events**: EventsTimeline with kind `"deployments"`.

### 3.2 Behavior

- **Replica field**:
  - Initial value: the second number of `ready` (`"x/y"` gives `y`), else 0.
  - Each time Describe loads, it is overwritten with `spec.replicas`.
- **Scale**: `Core::scale_deployment(namespace, name, replicas)` (merge patch of `spec.replicas`).
  - Success toast: `Scaled to {n} replicas`.
  - Failure: error toast with the message.
- **Restart** (Deliberate Fix 16: the backend patch works): opens ConfirmDialog (warning).
  - Title `Restart Deployment`.
  - Message `This will initiate a rolling restart of deployment "{name}". Pods will be recreated.`
  - Buttons: Restart / Cancel.
  - Calls `Core::restart_deployment(namespace, name)`, which stamps `kubectl.kubernetes.io/restartedAt` on the pod template.
  - Success: close the dialog and toast `Rolling restart initiated`.
  - Failure: error toast; the dialog stays open.
- **Delete**: ConfirmDialog (danger).
  - Title `Delete Deployment`.
  - Message `Are you sure you want to delete deployment "{name}" in namespace "{ns}"? This action cannot be undone.`
  - Calls `Core::delete_resource(ResourceKind::Deployments, …)`.
  - Success: close, toast `Deployment deleted`, then **navigate back**.
  - Failure: error toast; the dialog stays open.
- **Keyboard**:
  - Esc (`AppEvent::Escape`) always goes back. It is not delivered while the replica field, a search input or another text entry has focus, or while a modal is open. A focused search input closes its bar instead. An open but unfocused Logs-tab search is not closed first.
  - Keys 1–5 (no modifiers) switch tabs unless a text entry has focus.
  - The global `h` also goes back.

---

## 4. MultiPodLogs (`views/multi_pod_logs.rs`) — Deployment Logs tab

Created with `(namespace, deployment)`. The label selector is built from the Deployment's `spec.selector` before the stream starts:
- `matchLabels` as `k=v` (keys sorted), then `matchExpressions`: `In` → `k in (a,b)`, `NotIn` → `k notin (a,b)`, `Exists` → `k`, `DoesNotExist` → `!k`; comma-separated (e.g. `team=kore,tier=bugsel`).
- A missing/empty selector shows the error `Validation error: Deployment has no pod selector`; a failed fetch shows the formatted error.

There is no Stop button.

Deliberate Fix 5: lines are delivered through the `Subscription` returned by the stream call, so they actually appear (colored pod prefixes, a pod count, a working filter). The reference design never left the skeleton because of an event-channel name mismatch.

### 4.1 Toolbar

- Row: space-between, gap 8px, padding 16px × 8px, bottom border `S800`, background `SURFACE @ 50%`.
- Left (gap 8px):
  - `{namespace}/{labelSelector}`, for example `default/app=nginx` (just `{namespace}/` until the selector is resolved): 12px mono `#94a3b8`.
  - `{n} pod` or `{n} pods`, plus ` ({k} hidden)` when any pods are hidden: 10px `#475569`. `n` counts distinct pods seen in received lines.
- Right (gap 8px, 14px text). Buttons are `tool_button`s with padding 12px × 6px, 14px icons, gap 6px.
  1. **Filter** (`filter` icon): toggle. Active style while open.
  2. **Copy** (`copy` icon). Copies the visible lines as `{i+1} [{pod}] {line}` joined by `\n`. Toast `Copied to clipboard`.
  3. **Auto-scroll** chip: 14px `SYSTEM_BLUE` checkbox plus the text, padding 12px × 6px, gap 8px. Checked by default.

### 4.2 Filter panel

- Shown when Filter is active **and** at least one pod is known. No animation.
- Panel: bottom border `S800`, background `SURFACE @ 30%`, padding 16px × 8px, wrapping row, centered, gap 8px.
- Contents, in order:
  1. `PODS` caps label: 10px `#64748b`, margin-right 4px.
  2. `All` text button: 10px accent, hover accent @ 80%, margin-right 4px. Shows every pod.
  3. `None` text button: same style, margin-right 8px. Hides every pod.
  4. One chip per pod, sorted alphabetically:
     - Padding 8px × 4px, radius 4px, border, 12px mono, gap 6px.
     - Visible pod: border `S700`, background `#1e293b @ 50%`, text `#e2e8f0`.
     - Hidden pod: border `S800`, no background, text `#475569`.
     - Contents: 12px `SYSTEM_BLUE` checkbox, an 8px dot in the pod's color, then the name truncated at 160px.
     - Click toggles the pod.

### 4.3 Log area

- Card: the logs card (§2.5.2), inset 16px on every side.
- A variable-height GPUI `list` (rows wrap). The buffer is capped at 10,000 entries (oldest dropped).
- States:
  - Loading (until the stream is connected): the same 12-bar skeleton as the pod logs.
  - Error: padding 16px, `#f87171` 12px mono, showing the error string, for example `Validation error: No pods found matching the label selector`.
  - All entries hidden: `All pods are hidden. Adjust filters to see logs.`
  - No entries: `No logs available` (padding 16px, `#64748b`, 12px mono).
- Row (horizontal, 12px mono / 19px, hover white @ 2%, search tinting as in pod logs):
  1. Gutter: 48px, right-aligned, padding-right 12px, right border `#1e293b @ 50%`, `#475569` (hover `#64748b`). Shows the visible index + 1.
  2. Pod name: padding 8px horizontal, semibold, color = the pod's hex color, truncated at 164px.
  3. `|` separator in `#334155`.
  4. Line: padding-left 8px, wrapping. Lines of a multi-container pod start with `[{container}] ` (added by the backend).
     - Level colors as in pod logs **except there is no INFO→blue rule**: ERROR/FATAL/PANIC `#f87171`, WARN `#fbbf24`, DEBUG/TRACE `#64748b`, otherwise `#4ade80`.
  - Search highlights apply to both the pod name and the line.
- A 6px scrollbar thumb is painted over the right edge while the list overflows (6px reserved).
- Auto-scroll: scroll to the end whenever the visible entries change (new lines, filter changes) while the checkbox is on. Toggling the checkbox rebuilds the list.
- Search: ⌘F / Ctrl+F (window-level, only while this tab exists).
  - Same overlay as pod logs (placeholder `Search logs...`, no "No results").
  - Matches when the line **or** the pod name contains the query.
  - Enter / Shift+Enter navigates (wrapping) and scrolls the match into view. Esc in the input closes it.
- Pod colors are assigned by the backend in pod-list order, cycling through:

  `#58d0ff`, `#22c55e`, `#f59e0b`, `#ef4444`, `#a78bfa`, `#ec4899`, `#06b6d4`, `#84cc16`

### 4.4 Data flow

1. `Core::stream_multi_pod_logs(namespace, label_selector, None)` returns `MultiPodLogStream { pods: Vec<PodColor>, lines: Subscription<MultiPodLogLine> }`. The backend lists the matching pods and follows **every regular (non-init) container** of each one with a tail of 50 lines. When a pod has more than one container, each line is prefixed `[{container}] ` (as in single-pod all-containers mode, §2.5.4).
2. A successful call clears loading (an idle stream shows `No logs available`). Each `MultiPodLogLine { pod, color, line }` is appended (coalesced up to 2048 per update). The view takes pod names and colors from the lines; `pods` is not used.
3. If no pods match, the call fails with `Validation error: No pods found matching the label selector`, which shows as the error state.
4. The multi-pod stream is independent of single-pod streams. Dropping the tab drops the subscription, which stops the stream; `Core::stop_pod_logs_stream` and context switches also cancel it.

---

## 5. YamlEditor (`views/yaml_editor.rs`, `views/yaml_text_area.rs`) — the YAML tab of every detail view

### 5.1 Layout

Root: full size, vertical flex. Automation id `view.yaml`.

**Toolbar** (space-between, padding 16px × 8px, bottom border `S800`, background `SURFACE @ 50%`):
- Left (gap 6px):
  1. `YAML` caps label: 10px `#64748b`, margin-right 8px.
  2. Mode toggle button: padding 10px × 6px, radius 6px, 12px, icon 14px, gap 6px.
     - Read-only state: `lock` icon and `Read-only`. Border `S800`, text and icon `#94a3b8`, no hover change.
     - Editing state: `pencil` icon and `Editing`, active style.
- Right (gap 6px). All buttons: padding 10px × 6px, 12px, icon 14px.
  1. **Diff**: only when editing with changes. Standard style. Icon `git-compare-arrows`, or a spinning `loader-2` while loading (opacity 0.5, inert).
  2. **Reset**: only when editing with changes. `rotate-ccw` icon. Hover: border `#f59e0b @ 50%`, background `#f59e0b @ 10%`.
  3. **Copy**: always shown. `copy` icon, standard style.
  4. **Apply**: only when editing with changes. Border `#047857 @ 50%`, background `#10b981 @ 10%`, text `#34d399`. Hover: background `#10b981 @ 20%`, border `#10b981`. Icon `save`, or a spinning `loader-2` while applying (opacity 0.5, inert).

**Body**: `flex_1`, relative, overflow hidden. It contains the TextSearchBar overlay and one of the following panes:

- **Skeleton**: card (black @ 40%, border `S800`, radius 8px, inset 16px on every side). Contains 12 bars (`detail_ui::bars_skeleton`) with widths 85, 60, 75, 40, 90, 50, 70, 55, 80, 45, 65, 35 (%), styled like the log skeleton.
- **Editor** (same card, fully visible) containing `YamlTextArea`, one element that paints gutter, search marks, selection, text and caret, so the gutter cannot drift (Deliberate Fix 15):
  - **Line-number gutter**: width = digits × 7.212px + 29px (8px number padding each side, 12px right padding, 1px right border `#1e293b @ 50%`). Numbers right-aligned, 12px mono `#475569`, one per text line on the same 19px rows as the text, scrolled with it vertically. Not selectable.
  - **Text**: 12px mono at the 0.601em pitch (7.212px per column), 19px line height (`yaml_text_area::LINE_H`), 16px padding, no wrapping (horizontal scroll), **no syntax highlighting**.
    - Read-only: text `#94a3b8`. Text can still be selected and copied; edits are ignored.
    - Editing: text `#cbd5e1`; caret 1.5px wide accent bar (row height − 4px), shown only while focused.
    - Selection: `#3b82f6 @ 40%`.
    - Painted scrollbars: 6px `#334155` thumbs (radius 3px) on the right and bottom when the content overflows.
  - The text area is a registered text entry (global shortcuts and tab digits are inactive while it has focus). Mouse: click places the caret, drag selects, shift-click extends, double-click selects a word, triple-click a line; wheel scrolls (shift+wheel scrolls horizontally).
  - Key bindings (key context `KoreTextArea`): arrows / shift+arrows; Home/End and ⌘←/⌘→ (line start/end, with shift to select); ⌘↑/⌘↓ (document start/end); ⌥←/⌥→ (word, with shift to select); PageUp/PageDown; ⌘A/Ctrl+A select all; ⌘C/⌘X/⌘V (and Ctrl variants) copy/cut/paste; Enter newline; Backspace/Delete; ⌘Z / ⌘⇧Z (and Ctrl variants) undo/redo (200 steps; edits within 400ms coalesce). **Tab inserts 2 spaces** at the selection (replacing it). IME composition is supported.
- **Search highlight layer** (painted behind the text when the search is open and has matches):
  - Current match: `#fbbf24 @ 35%` with a 1px `#fbbf24 @ 50%` border.
  - Others: `#fbbf24 @ 15%`.
  - Radius 2px; the full row height.
- **Inline diff pane** (replaces the editor):
  - Card: black @ 40%, border `S800`, radius 8px, inset 16px on every side; vertical scroll with a 6px scrollbar.
  - Header: padding 16px × 8px, bottom border `#1e293b @ 50%`.
    - Left: `Diff: {a} additions, {d} deletions`, 12px `#94a3b8`.
    - Right: `Close diff` with `x` 14px, 12px `#64748b`, hover `#cbd5e1`.
  - Lines: 12px mono, 19px line height. Each row is horizontal:
    1. Gutter: 48px, right-aligned, padding-right 12px, right border `#1e293b @ 50%`. Shows the row index + 1 (a sequential counter, not file line numbers).
    2. Sign column: 20px, centered: `+`, `-` or a space.
    3. Content: padding-left 8px, wrapping.
  - Row colors:

| Row | Background | Gutter | Sign | Text |
|---|---|---|---|---|
| insert | `#10b981 @ 10%` | `#059669` | `#34d399` | `#6ee7b7` |
| delete | `#ef4444 @ 10%` | `#dc2626` | `#f87171` | `#fca5a5` |
| equal | none | `#475569` | `#334155` | `#94a3b8` |

  - Values from the backend include their trailing `\n`; it is stripped, so there is no extra blank line.
- **"Unsaved changes" pill**: shown when editing with changes and no diff open.
  - Absolute at bottom 24px, right 24px (inside the card, 8px above its bottom border).
  - 10px `#fbbf24` text on `#f59e0b @ 10%`, border `#f59e0b @ 30%`, rounded-full, padding 10px × 4px.

### 5.2 Behavior

- **Load** (on creation; each tab entry creates a new editor):
  - Starts read-only, no diff, no search.
  - `Core::get_resource_yaml(kind, namespace, name)`. The backend strips `metadata.managedFields`.
  - On error: error toast, and the text becomes `# Error fetching YAML: {msg}`.
- **Toggle edit**:
  - Read-only → Editing.
  - Editing → Read-only **discards changes** (text reverts to the original, the diff closes).
- **Reset**: reverts to the original text and closes the diff. Stays in editing mode.
- **Copy**: copies the current text. Toast `YAML copied to clipboard` (success).
- **Diff**: `Core::diff_resource_yaml(kind, namespace, name, new_yaml)` returns `Vec<DiffLine { tag: "equal"|"insert"|"delete", value }>`. The backend re-fetches the live YAML and line-diffs it with `similar`. Errors raise an error toast.
- **Apply**: opens ConfirmDialog (warning).
  - Title `Apply YAML Changes`.
  - Message: `Are you sure you want to apply the modified YAML to {kind without trailing s} "{name}" in namespace "{ns}"? This will update the live resource.`
  - Buttons: Apply / Cancel.
  - On confirm: close the dialog, then `Core::apply_resource_yaml(kind, namespace, name, yaml)` (server-side apply).
    - Success: toast with the result string (`Applied successfully`; `YAML applied successfully` if the backend returns an empty string); original = current text; diff closed; back to read-only.
    - Failure: error toast; stays in editing mode.
- **Search**: ⌘F / Ctrl+F (window-level, while the editor exists) opens TextSearchBar.
  - Input debounced 200ms; after the debounce the first match is scrolled to.
  - Case-insensitive substring positions in the text, including overlapping matches (none when lower-casing changes byte lengths, i.e. some non-ASCII text).
  - Next and previous wrap around **and close any open diff**.
  - The current match's line is scrolled so it sits about 1/3 from the top: `scroll_y = line*(content_height/lines) - viewport_height/3` (clamped at 0).
  - Closing clears the search and the marks.

---

## 6. DeploymentRollback (`views/deployment_rollback.rs`)

- **Data**: `Core::list_deployment_revisions(namespace, name)` returns `Vec<DeploymentRevision { revision, name (ReplicaSet name), created (RFC3339), image, change_cause, replicas }>` (the deployment's ReplicaSets by ownerReference), sorted by revision descending. Index 0 is treated as **current**.
- **States**:
  - **Loading**: padding 16px, vertical gap 12px. Three skeleton blocks 80px tall, radius 8px, opacities 1, 0.8 and 0.6.
  - **Error**: horizontal, gap 12px, padding 16px, 14px `#f87171`. `alert-circle` 16px plus `Failed to load revisions: {err}`.
  - **Empty**: centered column, padding 48px vertical, gap 8px, `#94a3b8`. `history` icon 32px `#475569` and `No revisions found` at 14px.

**Header** (padding 16px horizontal, 16px top, 8px bottom; gap 8px):
- `history` icon 16px accent.
- `Revision History`: 14px medium `#e2e8f0`.
- Count pill: 10px, background `S800`, text `#94a3b8`, padding 6px × 2px, rounded-full.

**Timeline** (scroll area with padding 16px horizontal and 16px bottom, 6px scrollbar; items 8px apart). Automation id `view.rollback`.
- Vertical line: absolute at left 15px, from top 8px to bottom 8px, 1px wide, `S800`.
- Each item:
  - Padding-left 36px.
  - Entry animation: opacity 0→1 and y 12px→0 over 0.2s, staggered by `i*0.03`s. Replays after every fresh fetch.
  - **Dot**: absolute at left 11px, top 14px. 9px circle with a 2px border.
    - Current: accent border, no fill.
    - Others: border `#475569`, fill `SURFACE`.
  - **Card**: border, radius 8px.
    - Current: border `PREFLIGHT_BORDER`, no background.
    - Others: border `S800`, background `SURFACE @ 30%`. Hover: border `S700`, background `SURFACE @ 50%`.
- **Card main row**: full-width, padding 12px × 10px, horizontal, centered, gap 12px:
  1. `chevron-right`, or `chevron-down` when expanded: 14px `#64748b`.
  2. `hash` 12px `#64748b`, then the revision number in mono 12px medium (accent if current, else `#cbd5e1`). For the current revision, a `CURRENT` tag follows: 9px caps, medium, accent text, padding 6px × 2px, radius 4px, no background.
  3. `container` 12px `#64748b`, then the image in mono 12px `#94a3b8`, truncated, `flex_1`. Shows `-` if empty.
  4. Relative time (`detail_ui::time_ago`) on the right: 10px `#64748b`. Format: `{s}s ago`, `{m}m ago`, `{h}h ago` or `{d}d ago`.
- Clicking the row toggles expansion. Only one revision can be expanded at a time.
- **Expanded section** (no animation):
  - Padding 12px horizontal, 4px top, 12px bottom. Top border `#1e293b @ 50%`.
  - Grid with 2 columns, 16px column gap, 8px row gap, margin-bottom 12px. Each cell:
    - Label: 10px caps, `#64748b`, margin-bottom 2px.
    - Value: a 12px `#64748b` icon plus 12px text, gap 6px.
  - Cells:

| Label | Icon | Value |
|---|---|---|
| Image | `container` | mono `#e2e8f0` (`-` if empty) |
| Created | `clock` | mono `#e2e8f0`, local time as `dd/mm/yyyy, HH:MM:SS` (`Invalid Date` if unparsable) |
| Replicas | `users` | mono `#e2e8f0` |
| Change Cause | `message-square` | `#cbd5e1`, or `No annotation` |

  - Buttons (gap 8px; padding 10px × 6px, radius 6px, 12px, 14px icons):
    - **View YAML** (`file-code-2`): standard style. While that revision's YAML is open: label `Hide YAML`, active style.
    - **Rollback** (`rotate-ccw`): only for non-current revisions. Border `#92400e @ 50%`, text `#fbbf24`. Hover: border `#d97706`, background `#f59e0b @ 10%`.
  - **Inline YAML viewer**:
    - Margin-top 8px, radius 6px, border `S800`, background black @ 40%, overflow hidden.
    - Header: padding 12px × 6px, bottom border `S800`, background `SURFACE @ 30%`.
      - Left: `REPLICASET YAML`, 10px caps, `#64748b`.
      - Right (gap 4px): icon buttons `copy` (12px) and `x` (12px), each `#64748b` with hover `#cbd5e1` and 4px padding.
    - Body while loading: centered spinning `loader-2` 16px accent, padding 32px vertical.
    - Body when loaded: max height 256px, scrollable (6px scrollbar), padding 12px, mono at `pitched_mono_size(12)` with a 19px line height, wrapping.
- **YAML highlighting** (rollback viewer only, `highlight_yaml`), per line. Default text `#f1f5f9`.
  - Comment lines (`^\s*#`): `#64748b`.
  - `key: value` lines (`^(\s*)([\w.-]+)(:)(.*)`): key in accent, colon in default color, then a single space and the trimmed value:
    - quoted value: `#34d399`
    - number: `#fbbf24`
    - `true`/`false`: `#818cf8`
    - `null`/`~`: `#64748b`
    - anything else non-empty: `#34d399`
  - `- item` lines: the `- ` in `#64748b`, the rest in `#34d399`.
- **Fetching YAML**: `Core::get_revision_yaml(namespace, rs_name)`. Clicking the same revision's button again (or the viewer's X) closes the viewer.
  - Errors show as the text `Error fetching YAML: {msg}`.
  - Copy toast: `YAML copied to clipboard`.
- **Rollback** (Deliberate Fix 16: the backend patch works): ConfirmDialog (warning).
  - Title `Rollback Deployment`.
  - Message `Roll back "{deployment}" to revision {n}? This will replace the current pod template with the one from this revision.`
  - Confirm text `Rollback` (`Rolling back...` while busy; further clicks are ignored).
  - Calls `Core::rollback_deployment(namespace, deployment, rs_name)`.
  - Success: toast `Rolled back to revision {n}`, close the dialog, re-fetch. Kubernetes renumbers the restored ReplicaSet to max+1 a moment later, so the view re-polls every 500ms (up to 20 times) until the restored ReplicaSet is at the top; the list then shows a new current revision with the restored image.
  - Failure: error toast; the dialog stays open with its label back to `Rollback`.

---

## 7. PodMetrics (`views/pod_metrics.rs`) — Pod Metrics tab

### 7.1 Data

- On creation, `Core::describe_pod` is called to obtain limits. When it returns, polling restarts (immediate fetch).
- Polling: `Core::get_pod_metrics(namespace, pod)` runs immediately and then **every 5000ms** (a `cx.spawn` loop with `backend::sleep`). Dropping the tab stops it.
- Usage: the sum over `containers[].usage.cpu` and `.memory`, parsed with `parse_quantity`.
- Limits (`pod_limits`):
  - If `status.allocatedResources` exists, use it.
  - Otherwise sum each container's `limits` (falling back to `requests`) for cpu and memory.
  - If that is still 0: cpu limit = usage×2 (so the gauge shows **50.0%**), or 1000m if usage is 0. Memory limit = usage×2, or 512Mi.
- Quantities are parsed per resource type (`parse_base_quantity` gives the base unit):
  - Plain numbers, including decimals and exponents (`0.5`, `12e6`), are the base unit.
  - Binary suffixes `Ki Mi Gi Ti Pi Ei`: × 1024ⁿ. Decimal suffixes `n u m k M G T P E`: × 10⁻⁹ … 10¹⁸.
  - Unparsable or empty gives 0.
  - **CPU** (`parse_cpu`) is in millicores: base × 1000 (`"2"` = 2000, `"0.5"` = 500, `"250m"` = 250, `"1500000n"` = 1.5).
  - **Memory** (`parse_memory`) is in bytes: the base value (`"1024"` = 1024 B, `"1Mi"` = 1048576, `"1G"` = 10⁹).
- History: each successful poll appends `{ cpu: usage/limit*100, memory: usage/limit*100, label: local time HH:MM:SS }`. Points older than 120s are dropped, which keeps about 24 points.
- Error mapping (`map_error`, on the formatted error string):
  - Contains `404` or `not found`: shows `Metrics Server not detected` if the string mentions `Metrics Server` or `metrics.k8s.io`, otherwise `Pod metrics not found: {err}`.
  - Otherwise contains `Metrics Server`: `Metrics Server not detected`.
  - Otherwise: `Error fetching metrics: {err}`.
  - On a kind cluster without metrics-server the result is **`Metrics Server not detected`**.
  - An error clears the current metrics. Polling continues and recovers automatically.

### 7.2 Visuals

- **Loading** (first fetch pending): centered `#94a3b8` text, `activity` icon 20px pulsing, gap 8px, `Loading metrics...` (16px).
- **Error**: centered column, `#94a3b8`.
  - `activity` icon 48px at opacity 50%, margin-bottom 16px.
  - Error text 18px.
  - Second line: 14px `#64748b`, margin-top 8px: `Make sure the Metrics Server is installed in your cluster`.
- **Loaded**: scroll container with padding 16px and 24px vertical gaps.
  - **Gauge row**: two cards side by side (gap 16px).
  - Each gauge card: background `SURFACE @ 30%`, border `S800`, radius 8px, padding 16px. Automation ids `metrics.cpu` / `metrics.memory`.
    - Title (`CPU Usage` / `Memory Usage`): 14px semibold `#cbd5e1`, margin-bottom 4px.
    - Subtitle: 12px `#94a3b8`. Format `{used} / {limit}`.
      - CPU uses `format_cpu`: `0 m`; `{n} m` with 0 decimals below 1000; else `{x.xx} cores`.
      - Memory uses `format_bytes`: 1024-based B/KB/MB/GB/TB with 2 decimals, for example `12.34 MB`; `0 B` for zero.
    - Margin-bottom 16px after the subtitle.
    - Gauge box: 128px tall, full width, painted on a canvas as a 100×100 viewBox scaled to fit and centered (≈102px circle). Progress starts at 12 o'clock and runs clockwise.
      - Track: circle r=40, stroke `#1e293b @ 30%`, width 8.
      - Progress: same circle. Stroke `#ef4444` above 80%, `#f59e0b` above 60%, otherwise `#10b981`. Round caps. Sweep = min(p,100)/100 of the circle. No transition animation.
      - Center label: `{p:.1}%`, 24px bold accent.
  - **Trend card**: only when history has at least one point. Same card style.
    - Title `Usage Trend (Last 2 Minutes)`: 14px semibold `#cbd5e1`, margin-bottom 16px.
    - Chart painted on a canvas, full width × 200px, with 5px margins, a 60px Y-axis column and a 30px X-axis band:
      - Grid: dashed (3/3) `#334155` horizontal lines at 0/25/50/75/100 and vertical lines at each point.
      - Y axis: line and 6px ticks in `#94a3b8`, labels 0/25/50/75/100 at 10px `#94a3b8`; a `%` label rotated −90° at the inside left.
      - X axis: line and ticks in `#94a3b8`; labels are the HH:MM:SS times at 10px, thinned from the right so they never overlap (5px minimum gap), the last one clamped inside the chart.
      - Lines (need at least 2 points): monotone cubic curves, 2px, no dots. CPU `#10b981`, memory `#3b82f6`. No legend, no draw-in animation.
      - Hover tooltip: a vertical `#cccccc` cursor line at the nearest point, and a box (background `#0f172a`, border `#334155`, radius 4px, padding 10px) with the time in `#94a3b8` and rows `CPU % : {raw float}` (`#10b981`) and `Memory % : {raw float}` (`#3b82f6`), 16px text, values unformatted. It sits right of the cursor, flipping left when it would overflow.

---

## 8. ExecTerminal (`views/exec_terminal.rs`, `views/terminal_model.rs`) — Pod Shell tab

### 8.1 Layout

Root: full size, vertical flex. Automation id `view.exec-terminal`.

**Shell bar** (horizontal, gap 8px, padding 16px × 8px, bottom border `#1e293b @ 50%`, background `SURFACE @ 30%`):
1. `SHELL:` label: 10px `#64748b`.
2. Two buttons, `sh` (for `/bin/sh`) and `bash` (for `/bin/bash`). Padding 8px × 2px, radius 4px, 12px, 1px border. Automation ids `exec.shell.sh` / `exec.shell.bash`.
   - Selected: accent text, border `PREFLIGHT_BORDER`, no background.
   - Unselected: `#94a3b8` text, hover `#e2e8f0`, border `S800`.
   - Default selection: `sh`.
3. Status, right-aligned, 12px (automation id `exec.status`):
   - connecting: `#fbbf24`, spinning `loader-2` 12px plus `Connecting...`.
   - connected: `#34d399` `Connected`.
   - error: `#f87171`, `alert-circle` 12px plus the error message (or `Error`).
   - closed: `#94a3b8` `Session ended`.

**Terminal host**: `flex_1`, padding 4px, background `#0b1221`, overflow hidden, I-beam cursor, filled by the painted grid. It is focusable (click to focus) and a registered text entry. Automation id `exec.terminal`.

### 8.2 Terminal configuration

VT emulation is an `alacritty_terminal::Term` (`terminal_model::TermModel`); `exec_terminal.rs` paints its grid.
- Font: `theme::MONO` at 13px (`FONT_SIZE`). Cells are 7.8px × 15px (`CELL_W` × `CELL_H`); glyphs are placed one per cell.
- Scrollback 1000 lines (`SCROLLBACK`); the mouse wheel scrolls it. Typing jumps back to the bottom and clears the selection.
- Cursor: block, accent fill with the character under it drawn in the background color, blinking every 600ms while focused. Unfocused: no cursor is drawn.
- Bold brightens ANSI colors 0–7; DIM is drawn at 50% blended with the background; inverse, underline and strikeout are honored.
- Theme:

| Slot | Value |
|---|---|
| background | `#0b1221` |
| foreground | `#e2e8f0` |
| cursor | current accent |
| selection | accent @ 20% (`0x33` alpha) |
| black / brightBlack | `#1e293b` / `#475569` |
| red / brightRed | `#f87171` / `#fca5a5` |
| green / brightGreen | `#4ade80` / `#86efac` |
| yellow / brightYellow | `#fbbf24` / `#fde68a` |
| blue / brightBlue | `#60a5fa` / `#93c5fd` |
| magenta / brightMagenta | `#c084fc` / `#d8b4fe` |
| cyan / brightCyan | `#22d3ee` / `#67e8f9` |
| white / brightWhite | `#e2e8f0` / `#f8fafc` |

  Indexes 16–255 use the standard 256-color cube and gray ramp.
- **Grid sizing** (`terminal_model::grid_size`): the grid is the whole number of cells that fit the host's content box (minimum 2 × 1), recomputed on every layout. **Deliberate Fix 7:** after the session starts and whenever the size changes, `Core::resize_exec(session_id, cols, rows)` resizes the remote TTY, so `stty size` reports the real rows and columns.
- Selection: drag selects; double-click selects a word (semantic); triple-click a line. ⌘C copies the selection. ⌘V pastes the clipboard (bracketed when the shell enabled bracketed-paste mode).
- Clicking (without dragging) on a URL opens it in the default browser.
- Keys are translated to standard VT byte sequences (`terminal_model::key_bytes`: Enter `\r`, Backspace `\x7f`, Tab / Shift+Tab, Esc `\x1b`, arrows honoring application-cursor mode and modifiers, Ctrl combinations, Alt as an ESC prefix). Printable text arrives through the platform input handler (IME supported). ⌘ combinations other than C/V are not sent.

### 8.3 Session lifecycle

1. On creation, and whenever the shell changes (or the pod view recreates the terminal for a new container, §2.10), start a fresh session: drop the old session task, stop the old session (`Core::stop_exec`), and reset the terminal model (the screen clears).
2. `Core::exec_into_pod(namespace, pod, container: Option<String>, shell: Some(path))` returns `ExecSession { session_id: "{ns}/{pod}/{random}", stdout: Subscription<Vec<u8>>, exit: oneshot Receiver<ExecExit> }`. The backend runs `[shell]` with stdin, stdout and a TTY.
3. Set status to connected **as soon as the call resolves**, before the shell has actually attached, and send the current size (Fix 7).
4. Stdout bytes (coalesced) are fed to the terminal model. Replies the model generates (e.g. device-status reports) are written back to stdin.
5. When the exit receiver resolves (or stdout ends, then exit is awaited), with `ExecExit { reason, error }`:
   - `error`: status becomes error, and `\r\n\x1b[31m{error}\x1b[0m\r\n` is written (red).
   - Otherwise: status becomes closed, and `\r\n\x1b[33mSession ended.\x1b[0m\r\n` is written (yellow).
6. Input goes through a channel to `Core::send_exec_bytes(session_id, bytes)` (raw bytes, coalesced).
7. Dropping the terminal (tab change, pod view drop) drops the session task and its subscription.
8. If `exec_into_pod` itself fails: status becomes error with the formatted error.

Typing `exit` ends the session, shows "Session ended." and the status text `Session ended`. There is no automatic reconnect; switching shells or re-entering the tab starts a new session.

---

## 9. EventsTimeline (`views/events_timeline.rs`) — Events tab of every detail view

### 9.1 Data

- **Live mode** (default): `Core::list_events_for_resource(kind, namespace, name)`.
  - The backend filters on `involvedObject.name={name},involvedObject.namespace={ns}` and ignores kind.
  - Deliberate Fix 10: the listed events are also written to the SQLite event store, so History has them even if the Events table was never opened.
  - Results are sorted by `lastTimestamp`, falling back to `metadata.creationTimestamp`, newest first.
  - Fetched once per mode or range change. There is **no live updating** despite the name, and the relative times do not tick.
- **History mode** (Deliberate Fix 10: events are stored, so this lists real history):
  - Calls `Core::query_stored_events(since: now-range RFC3339, until: now RFC3339, namespace)`, where namespace is `None` if it is `*`. The store is scoped to the current context.
  - Results are filtered to rows whose `involved_object` string **contains** `name`.
  - Each `StoredEvent` maps to `{ reason, message, count, time: last_seen, type: event_type }`, sorted newest first.
  - Ranges: `1h`, `6h`, `24h`, `7d` (default `1h`).

### 9.2 Visuals

- **Loading**: padding 16px, gap 12px. Four skeleton blocks 64px tall, radius 8px, opacities 1, .85, .7 and .55.
- **Error**: horizontal, gap 12px, padding 16px, 14px `#f87171`. `alert-circle` 16px plus `Failed to load events: {err}`.
- **Empty**: centered column, padding 48px vertical, gap 8px, `#94a3b8`. `clock` icon 32px `#475569` and `No events found` at 14px. Automation id `events.empty`.
  - The Live/History toggle row (and the History range group) is rendered above the empty state (padding 16px top/left/right; the empty block fills the rest, centered), so an empty mode never traps the user. The error state also keeps the toggle above its message.
- **List**: scroll area (6px scrollbar), padding 16px, 8px vertical gap. Automation id `view.events`.
  - **Toggle row** (gap 8px, margin-bottom 4px, so 12px to the first card):
    - `Live` (`radio` icon 12px) and `History` (`history` icon 12px) buttons: padding 12px × 6px, radius 6px, 12px, border, gap 6px.
      - Active: active style (gray accent border, accent text and icon).
      - Inactive: border `S700`, text `#94a3b8`. Hover: `#e2e8f0` text, border `#475569`.
    - In History mode a range group follows (margin-left 8px, gap 4px). Buttons `1h`, `6h`, `24h`, `7d`: padding 8px × 4px, radius 4px, 10px mono.
      - Active: accent text, no background.
      - Inactive: `#64748b`, hover `#cbd5e1`.
  - **Event card**: border, radius 8px, padding 12px, hover background white @ 2%. Contents horizontal, items start, gap 12px:
    - Icon: 16px, margin-top 2px.
    - Body:
      - Line 1 (gap 8px, margin-bottom 4px):
        - Reason (or `Unknown`): 12px medium `#e2e8f0`.
        - If `count > 1`: pill `x{count}`, 10px, background `S800`, text `#94a3b8`, padding 6px × 2px, rounded-full.
        - Right-aligned relative time (`{n}s/m/h/d ago`, or `-`): 10px `#64748b`.
      - Line 2: message (or `-`), 12px `#94a3b8`, wrapping.
    - Type styling:

| Type | Border | Background | Icon |
|---|---|---|---|
| Normal (and unknown) | `#3b82f6 @ 30%` | `#3b82f6 @ 5%` | `info` `#60a5fa` |
| Warning | `#f59e0b @ 30%` | `#f59e0b @ 5%` | `alert-triangle` `#fbbf24` |
| Error | `#ef4444 @ 30%` | `#ef4444 @ 5%` | `info` `#f87171` (no dedicated icon) |

---

## 10. PortForwarding (`views/port_forward.rs`) — pod sidebar

### 10.1 Layout

Panel (`PortForwardPanel`): width 320px, full height, left border `S800`, background `SURFACE @ 50%`, vertical flex. Enter animation on creation: opacity 0→1 and a −20px left offset → 0, 300ms ease-out. Automation id `view.port-forward`.

1. **Header**: padding 16px, bottom border `S800`.
   - `plug` icon 20px accent, then `Port Forwarding` at 18px semibold `#f1f5f9` (gap 8px, margin-bottom 8px).
   - Subtitle `Forward local ports to pod ports`: 12px `#94a3b8`.
2. **Form**: padding 16px, bottom border, 16px vertical gap.
   - **`Pod Port`**: label 12px `#94a3b8`, margin-bottom 6px.
     - Field: full width, background `#0b1221`, border `S800` (accent while focused), radius 4px; `TextInput` with padding 12px × 8px, 14px `#f1f5f9`, placeholder `#64748b`; a number-stepper glyph (`#cbd5e1`) 12px from the right. Automation id `pf.podport`.
     - Default `80`, placeholder `80`.
   - **`Local Port`**: same label style, followed by `(leave blank for auto)` in `#64748b` (4px gap).
     - Same field. Default `8080`, placeholder `8080`. Automation id `pf.localport`.
   - Both fields are dimmed (opacity 0.5) while starting. Editing either one clears the error.
   - **Error box** (fades in over 200ms from a −10px offset): horizontal, gap 8px, padding 8px, background `#ef4444 @ 10%`, border `#ef4444 @ 50%`, radius 4px, 12px `#f87171`. `alert-circle` 16px plus the message. Automation id `pf.error`.
   - **Start button**: full width, padding 16px × 8px, border accent (solid), no background, radius 4px, 14px accent, centered, gap 8px. Automation id `pf.start`.
     - Content: `plug` 16px plus `Start Forward`. While starting: spinning `loader-2` 16px plus `Starting...`.
     - Disabled while starting or when Pod Port is empty (opacity 0.5, cursor not-allowed).
3. **List**: `flex_1`, scrollable, padding 16px, 12px gap.
   - Empty: `No active port forwards`, centered, padding 32px vertical, 14px `#64748b`.
   - **Forward card**: padding 12px, radius 4px, border. Enter animation: opacity 0→1 over 200ms. No exit animation.
     - active: background `#22c55e @ 10%`, border `#22c55e @ 50%`.
     - connecting: background `#eab308 @ 10%`, border `#eab308 @ 50%`.
     - error: background `#ef4444 @ 10%`, border `#ef4444 @ 50%`.
     - **Top row** (space-between, margin-bottom 8px):
       - Status indicator (gap 8px):
         - active: an 8px `#22c55e` dot with a ping halo, then `LIVE`.
         - connecting: spinning `loader-2` 12px `#facc15`, then `CONNECTING`.
         - error: `alert-circle` 12px `#f87171`, then `ERROR`.
         - The label is 12px semibold in `#4ade80`, `#facc15` or `#f87171`.
       - Stop button: `x` 16px `#f87171`, padding 4px, radius 4px. Hover: background `#ef4444 @ 20%`, icon `#fca5a5`. Automation id `pf.stop.<port>`, label "Stop port forward".
     - **Rows** (12px, 6px gap, labels `#94a3b8`, gap 8px):
       - `Local:` followed by `127.0.0.1:{localPort}`:
         - When active: an accent mono link with an `external-link` 12px icon. Hover: accent @ 80% with underline. Click opens `http://127.0.0.1:{port}` in the default browser (`cx.open_url`).
         - Otherwise: `#cbd5e1` mono text.
       - `Pod:` followed by `{podName}:{podPort}` in `#cbd5e1` mono.
       - Status message: 12px `#64748b`, margin-top 4px. One of `Connecting...`, `Connected`, or the error text.

### 10.2 Behavior

- **Validation** (`port_forward::validate`):
  - Pod port must be an integer from 1 to 65535 (leading-integer parse). Otherwise the error is `Pod port must be a valid number between 1 and 65535`.
  - Local port blank or 0: pick a random port in 49152–65535 not used by an active forward in this list. After 100 failed attempts the error is `Unable to find an available port. Please specify a port manually.`
  - Local port outside 1–65535: `Local port must be a valid number between 1 and 65535`.
  - Local port already used by an active forward in this list: `Port {n} is already in use by another port forward`.
- **Start**:
  1. Add a `connecting` card with id `{ns}/{pod}/{local}/{podPort}`.
  2. `Core::start_port_forward(namespace, pod, local_port, pod_port)` returns JSON `{ localPort, podPort, status }`. The backend binds `127.0.0.1:{local}` and proxies each connection through a `kubectl port-forward` subprocess. **Deliberate Fix 6:** kubectl gets `--context` for the context selected in Kore (and Kore's kubeconfig), not kubectl's current-context.
  3. Success: the card becomes active with message `Connected` (using the returned local port). Toast `Port forward active on :{port}`. Inputs reset to 80 and 8080.
  4. Failure: the card becomes error with the message (for example `Port forward error: Port 8080 is already in use`), and the same text appears in the form error box.
- **Stop**: `Core::stop_port_forward(namespace, pod, local_port, pod_port)`, then remove the card. Errors go to the form error box (the card stays). Error cards use the same stop call.
- The list is local to the panel entity; there is no backend query to restore it (§2.11).

---

## 11. DebugContainerModal (`views/debug_container.rs`)

A fresh modal entity is created on every open and dropped when it reports `Created` or `Cancelled`, so its state (selected image, custom text, checkbox, target, error) does **not** persist between opens. While it exists it registers itself as an open modal (`AppState::set_modal("debug-container", true)`).

**Shell**: painted above everything (`deferred`, window-anchored).
- Backdrop: the whole window, black @ 60%, padding 16px, centers the dialog. Clicking it cancels, unless loading. No blur.
- Dialog: full width up to 448px, background `SURFACE` (no tint), border `PREFLIGHT_BORDER`, radius 8px, shadow as the ConfirmDialog, padding 24px. Automation id `debug`. No enter animation.
- **Esc** closes the modal (cancel), unless loading or the target select's popup is open; it never navigates back (Deliberate Fix 4).

**Normal form**:
- **Header** (items start, gap 16px, margin-bottom 20px):
  - Bubble: padding 8px, rounded-full, no background, containing `bug` 24px accent.
  - Title `Debug Container`: 18px semibold `#f1f5f9`.
  - Subtitle (12px `#94a3b8`, margin-top 4px): `Inject an ephemeral container into {podName}`, with the pod name in mono accent.
  - Close button: `x` 20px `#94a3b8`, hover `#e2e8f0`.
- **`DEBUG IMAGE`** caps label: 12px `#94a3b8`, margin-bottom 8px.
- **Image options** (6px vertical gap). Each option is a radio row: padding 10px, radius 6px, border, gap 12px.
  - Selected: border `PREFLIGHT_BORDER`, no background.
  - Unselected: border `S800`. Hover: border `S700`, background `MUTED @ 20%`.
  - Row contents: a 14px accent radio; the label (14px `#e2e8f0`) and the description (12px `#64748b`, margin-left 8px) on a shared baseline; the image name right-aligned (10px mono `#475569`, truncated at 140px).
  - Options (automation ids `debug.image.<label>`):

| Label | Image | Description |
|---|---|---|
| `netshoot` (default) | `nicolaka/netshoot` | `Network troubleshooting` |
| `busybox` | `busybox:latest` | `Minimal Unix utilities` |
| `ubuntu` | `ubuntu:latest` | `Full Ubuntu environment` |
| `curl` | `curlimages/curl` | `cURL for HTTP debugging` |

  - A fifth option `Custom` has only a label. When it is selected, a text field appears below:
    - Margin-top 4px, radius 6px, border `S800` (`PREFLIGHT_BORDER` while focused), background `#0b1221`; input padding 12px × 8px, 14px `#e2e8f0`.
    - Placeholder `e.g. alpine:latest` in `#475569`.
- **Share process namespace**: a 14px accent checkbox plus `Share process namespace` (14px `#cbd5e1`), gap 8px, with a 16px bottom margin on the group.
  - Help text below: 10px `#64748b`, margin-top 4px, indented 20px: `Target a container to share its PID namespace (see processes with ps)`.
- **`TARGET CONTAINER`** select: only when sharing is checked and the pod has containers.
  - Label: 12px caps `#94a3b8`, margin-bottom 6px.
  - `NativeSelect`: full width, padding 12px × 8px, radius 6px, border `S800`, background `#0b1221`, 14px `#e2e8f0`.
  - Options: `Select a container...` (empty value), then the regular containers (no init), from `Core::describe_pod`.
- **Error box**: margin-bottom 16px, padding 10px, background `#ef4444 @ 10%`, border `#ef4444 @ 50%`, radius 6px, 12px `#f87171`.
- **Footer** (justify end, gap 12px, margin-top 24px):
  - `Cancel`: same as the ConfirmDialog cancel button.
  - `Create & Connect`: padding 16px × 8px, radius 4px, border `PREFLIGHT_BORDER`, no background, 14px accent, with a `bug` 16px accent icon (gap 8px). Disabled (opacity 0.5, not-allowed) when the resolved image is empty (blank custom text).

**Loading state** (replaces the form):
- Centered column with 40px vertical padding.
- Bubble: padding 12px, rounded-full, no background, margin-bottom 16px, containing `bug` 32px accent **pulsing**.
- `Creating debug container...`: 18px semibold `#f1f5f9`, margin-bottom 8px.
- `Waiting for container to start. This may take up to 30 seconds.`: 14px `#94a3b8`, centered, margin-bottom 12px.
- Image chip: 12px mono, accent @ 80% text, no background, padding 12px × 4px, radius 4px.

**Submit**:
- An empty image gives the error `Please specify a container image`.
- Target = the selected container only when sharing is on and a target is chosen.
- Call `Core::add_debug_container(namespace, pod, image, target_container, Some(["/bin/sh"]))`.
  - The backend patches the `ephemeralcontainers` subresource with container name `debugger-{8 hex}`, stdin and tty on.
  - It then polls up to 30×1s until the container is Running and returns the name.
- Backend error texts (prefixed `Validation error: ` by the error's Display):
  - `Ephemeral containers not supported by this cluster (requires K8s 1.25+)`
  - `RBAC: insufficient permissions to create ephemeral containers`
  - `Static pods do not support ephemeral containers. Try a non-static pod instead.`
  - `Pod does not support ephemeral containers: …`
  - `Debug container failed: ErrImagePull: …` (also `ImagePullBackOff`, `InvalidImageName`)
  - `Debug container terminated unexpectedly`
  - `Timed out waiting for debug container to start (30s)`
- Failure: back to the form with the error box.
- Success: emit `DebugEvent::Created { container, image }` (§2.13).

---

## 12. ResourceDetailsView (`views/resource_details.rs`) — every other kind

### 12.1 Layout

Root: full size, vertical flex, background `#0b1221`, enter animation. Automation id `view.details`.

Header: padding 16px, bottom border `S800`, background `SURFACE @ 50%`.
- Row 1 (space-between, margin-bottom 12px):
  - Back button: same as the deployment view (`arrow-left` 16px, `Back`, `Kbd Esc`).
  - Delete button: `BtnStyle::Danger`, padding 12px × 6px, gap 8px, `trash-2` 16px, `Delete`, `Kbd D`.
- Row 2: a 3-column grid with a 16px gap of `info_cell`s (label 10px caps `#64748b`; value mono 12px `#f1f5f9`):
  1. `Name`
  2. `Namespace` (shows `default` when absent, for example for nodes and namespaces)
  3. `Kind`: the plural kind with its first letter capitalized (`KindExt::header_label`), for example `Services`, `Configmaps`

Tabs (same style as the pod view):

| Key | Tab |
|---|---|
| 1 | Describe (default) |
| 2 | YAML |
| 3 | Events |

**Describe**:
- Card: vertical flex, `SURFACE @ 30%`, border `S800`, radius 8px, inset 16px on every side (fully visible), overflow hidden.
- Data: `Core::describe_resource(kind, namespace, name)`, re-fetched every time the tab is activated. A watchdog re-issues the request if no result arrives within 4s (up to 3 retries); stale results are ignored.
- Skeleton: like the pod view's, but the second group has only 2 bars (256px and 208px).
- Error: `Error: {err}`.
- Content: DescribeContent, scrolling inside a `flex_1 min_h_0` area.

**Secrets only: Reveal bar** (above the content, when `data` has keys):
- Bar: padding 16px horizontal, 12px top and bottom; bottom border `#1e293b @ 50%`; wrapping; gap 8px.
- `REVEAL:` label: 10px `#64748b`, margin-right 4px.
- One toggle chip per key (in key order): padding 8px × 4px, radius 4px, 12px, border, gap 4px. Automation id `reveal.<key>`.
  - Hidden key: `eye` icon 12px. Border `S700`, text and icon `#94a3b8`, hover border `#475569`.
  - Revealed key: `eye-off` icon. Active style (gray accent border, accent text and icon).
- Values in `data`:
  - Hidden keys show `"***"`.
  - Revealed keys show the base64-decoded value as a byte string (each byte one character; on decode failure the raw value is shown).
  - The describe JSON re-renders immediately on toggle.
  - The YAML tab still shows raw base64.

### 12.2 Behavior

- **Esc** (`AppEvent::Escape`): back.
- **`d`/`D`**: opens the delete confirm when no confirm or modal is open and no text entry has focus. Modifiers are not checked.
- **1–3** (no modifiers): switch tabs. `4` and above do nothing.
- **Delete**: ConfirmDialog (danger).
  - Title `Delete {Kind singular}` (`KindExt::singular_label`: first letter upper-cased, one trailing `s` removed). Examples: `Delete Service`, `Delete Configmap`, `Delete Ingresse` (sic), `Delete Node`.
  - Message `Are you sure you want to delete "{name}" in namespace "{ns}"? This action cannot be undone.`
  - Calls `Core::delete_resource(kind, namespace, name)`.
  - Success: close, toast `Resource deleted`, then back.
  - Failure: error toast; the dialog stays open.

---

## 13. Unused components (in scope, not rendered anywhere)

The reference design contained two components that were never rendered. The native app has no equivalent; they are recorded here only in case they are wanted later.

- **Log panel**: a 192px-tall glass box (black @ 40% over the glass fill) with a `S800` border, radius 8px, scrollable, 12px mono with a 19.5px line height, padding 12px × 8px, showing wrapped log text. **Not used; not ported.**
- **Side-by-side diff viewer**: a two-pane YAML diff using an LCS line diff. **Not used; not ported.** (The YAML tab uses the inline diff of §5.1.) Reference design:
  - **Header**: padding 16px × 10px, bottom border, background `SURFACE @ 50%`.
    - Left: `columns-2` 16px accent plus `YAML Diff` (14px medium `#cbd5e1`).
    - Right (12px, gap 12px): `-{removed}` `#f87171`, `+{added}` `#4ade80`, `{total} line(s)` `#64748b`.
  - **Title row**: two halves. Left title (default `Previous`) on `#ef4444 @ 3%` with a right border; right title (default `Current`) on `#22c55e @ 3%`. Both 10px caps, medium, `#64748b`, padding 16px × 6px.
  - **Panels**: two equal scroll panes on black @ 40% with synced vertical scrolling.
    - Line row: min height 20px.
      - Gutter: 40px, 10px mono, right border.
      - Sign column: 16px, 10px.
      - Content: 12px mono, no wrapping.
    - Removed lines on the left: background `#ef4444 @ 15%`, text `#fca5a5`, gutter `#ef4444 @ 10%` with `#f87171 @ 70%`, sign `-` in `#f87171 @ 60%`.
    - Added lines on the right: background `#22c55e @ 15%`, text `#86efac`, gutter `#22c55e @ 10%` with `#4ade80 @ 70%`, sign `+` in `#4ade80 @ 60%`.
    - The opposite side shows an empty placeholder row tinted `#ef4444 @ 3%` or `#22c55e @ 3%`.
  - **Collapsing**: runs of more than 7 unchanged lines collapse, leaving 3 lines of context on each side. The collapsed bar is full width, padding 12px × 4px, background `#1e293b @ 30%` (hover 50%), 10px `#64748b`, top and bottom borders, with `chevron-right` and `... {n} unchanged lines ...`. Clicking expands it.
  - **Footer** (when there are collapsible sections): `Expand all` (`chevron-down`) and `Collapse all` (`chevron-right`), 10px, centered, background `SURFACE @ 30%`.

---

## 14. Backend contract summary (this area)

All calls go through `backend::core(cx)` (`kore_core::Core`); each is an async method run on Core's runtime. Errors are `K8sError` values; `backend::format_error` gives their Display text, typically `Kubernetes error: …`, `Validation error: …` or `Port forward error: …`.

| Purpose | `Core` method | Arguments | Returns |
|---|---|---|---|
| Pod JSON | `describe_pod` | `namespace, pod_name` | `serde_json::Value` (Pod) |
| Any JSON | `describe_resource` | `kind: ResourceKind, namespace, name` | `Value` |
| Delete | `delete_resource` | `kind, namespace, name` | `()` |
| Pod logs | `start_pod_logs_stream` | `namespace, pod_name, container: Option<String>, previous: Option<bool>` | `Subscription<PodLogPayload>` |
| Stop all log streams | `stop_pod_logs_stream` | — | `()` |
| Multi-pod logs | `stream_multi_pod_logs` | `namespace, label_selector, tail_lines: Option<i64>` | `MultiPodLogStream { pods: Vec<PodColor>, lines: Subscription<MultiPodLogLine> }` |
| Metrics | `get_pod_metrics` | `namespace, pod_name` | `Value` (PodMetrics: `containers[].usage.{cpu,memory}`) |
| Exec start | `exec_into_pod` | `namespace, pod_name, container: Option<String>, shell: Option<String>` | `ExecSession { session_id, stdout: Subscription<Vec<u8>>, exit: oneshot::Receiver<ExecExit> }` |
| Exec input | `send_exec_bytes` (also `send_exec_input` for strings) | `session_id, data` | `()` |
| Exec resize (Fix 7) | `resize_exec` | `session_id, cols: u16, rows: u16` | `()` |
| Exec stop | `stop_exec` | `session_id` | `()` |
| YAML get | `get_resource_yaml` | `kind, namespace, name` | `String` |
| YAML apply | `apply_resource_yaml` | `kind, namespace, name, yaml` | `String` (`"Applied successfully"`) |
| YAML diff | `diff_resource_yaml` | `kind, namespace, name, new_yaml` | `Vec<DiffLine { tag, value }>` |
| Revisions | `list_deployment_revisions` | `namespace, name` | `Vec<DeploymentRevision>` |
| Revision YAML | `get_revision_yaml` | `namespace, rs_name` | `String` |
| Rollback (Fix 16) | `rollback_deployment` | `namespace, deployment_name, rs_name` | `()` |
| Scale | `scale_deployment` | `namespace, name, replicas: i32` (≥ 0) | `()` |
| Restart (Fix 16) | `restart_deployment` | `namespace, name` | `()` |
| Events (live) | `list_events_for_resource` | `kind: String, namespace, name` | `Vec<Value>` (Event objects; also stored, Fix 10) |
| Events (stored) | `query_stored_events` | `since, until (RFC3339), namespace: Option<String>` | `Vec<StoredEvent>` |
| Port forward start (Fix 6) | `start_port_forward` | `namespace, pod_name, local_port: u16, pod_port: u16` | `Value` `{localPort, podPort, status}` |
| Port forward stop | `stop_port_forward` | same | `()` |
| Debug add | `add_debug_container` | `namespace, pod_name, image, target_container: Option<String>, command: Option<Vec<String>>` | container name `String` |
| Debug list | `list_debug_containers` | `namespace, pod_name` | `Vec<Value>` (`{name, image, targetContainer?, running}`) |
| Debug stop | `stop_debug_container` | `namespace, pod_name, container_name` | `()` |

| Stream | Item | Consumer |
|---|---|---|
| `Subscription<PodLogPayload>` | `{logs: Option<String>, error: Option<String>, append: Option<bool>}` | PodLogs (§2.5.4) |
| `MultiPodLogStream.lines` | `MultiPodLogLine {pod, color, line}` | MultiPodLogs (§4.4) |
| `ExecSession.stdout` / `.exit` | raw bytes / `ExecExit {reason: Option<String>, error: Option<String>}` | ExecTerminal (§8.3) |
| `ResourceStore` (pods watch, `state/resources.rs`) | settled pod rows | PodDetailsView deletion detection (§2.12) |

Dropping a `Subscription` cancels its backend stream; a context switch cancels every stream in Core.

---

## 15. Harness test scenarios (kind cluster)

Automated versions of these checks live in `harness/scenarios/20-details-*.yaml` (run against the `$H cluster seed` fixtures in namespace `kore-demo`; the `*-fix-*` scenarios cover Deliberate Fixes 5, 6, 7, 10, 15 and 16). The manual setup below is equivalent:

```
kind create cluster
kubectl create deployment nginx --image=nginx --replicas=2      # pods labelled app=nginx
kubectl run multi --image=busybox --overrides='{"spec":{"initContainers":[{"name":"init1","image":"busybox","command":["sh","-c","echo init-done"]}],"containers":[{"name":"a","image":"busybox","command":["sh","-c","while true; do echo INFO tick-a; sleep 2; done"]},{"name":"b","image":"busybox","command":["sh","-c","while true; do echo ERROR tick-b; sleep 3; done"]}]}}'
kubectl run logger --image=busybox -- sh -c 'i=0; while true; do echo "WARN line $i"; i=$((i+1)); sleep 1; done'
kubectl create configmap demo --from-literal=key=value
kubectl create secret generic demo-secret --from-literal=password=hunter2
```

No metrics-server is installed unless a scenario says so. Start each scenario from the Pods table in namespace `default`.

### Pod details — header and tabs
1. **Open pod**: click the `logger` row (or select it with j/k and press `l`).
   - The header shows the `Esc` chip, `logger`, a green pill `Running` with a pulsing dot, `› details`, and the buttons `Debug B`, `Delete D`, `Ports`.
   - Tabs read `Logs Describe YAML Metrics Events Shell`, with Logs underlined in accent.
2. **Details expand**: click `details`. A row appears with `Namespace: default │ Node: kind-control-plane │ IP: 10.244.x.x`. Click again and it collapses.
   - **Live status**: with the view open, a pod whose container exits changes its badge (e.g. `Ready` → `NotReady`) without leaving the view (scenario `20-details-bug-live-status`).
3. **Tab keys**: press `2`, `3`, `4`, `5`, `6`, then `1`. Each press activates Describe, YAML, Metrics, Events, Shell and Logs in turn, and the underline slides. (Click outside the terminal before pressing `1` after Shell, since a focused terminal receives digits.)
4. **Back**:
   - Press `Esc` on the Logs tab with no search open: the Pods table returns.
   - Re-open, press `h`: the table returns.
   - Re-open, click the back button: the table returns.

### Logs
5. **Streaming**: open `logger`. Within a few seconds amber `WARN line N` rows appear with line numbers in a left gutter, and new lines keep arriving. The last row is empty (trailing newline). The card's bottom border and corners are visible 16px above the bottom edge (scenario `20-details-bug-card-bottom`).
   - **Wrapped lines**: a line longer than the card wraps onto several visual rows and pushes the following rows down; nothing overlaps (scenario `20-details-bug-log-wrap`, which also checks scroll frame time).
6. **Auto-scroll off**: uncheck `Auto-scroll` and wait 5s. The view no longer jumps to the bottom. Re-check it and the view jumps to the newest line.
7. **Search**:
   - Press `⌘F`, type `line 1`. The counter shows `1/N`; matching rows are amber-tinted and the term is highlighted.
   - Press Enter: the counter goes to `2/N` and the view scrolls.
   - Press Shift+Enter: it goes back to `1/N`.
   - Press Esc: the overlay closes and the view stays on the pod.
8. **Copy**: click `Copy`; toast `Copied to clipboard` appears at the bottom right. The clipboard contains log text.
9. **Download**: click `Download`; toast `Logs downloaded`. A file `logger-<timestamp>.log` exists in the Downloads folder.
10. **Container selector**: open `multi`.
    - The select reads `All containers`. Its options are `All containers`, `init:init1`, `a`, `b`.
    - The default view shows lines prefixed `[a] ` (blue, `INFO tick-a`) and `[b] ` (red, `ERROR tick-b`).
    - Select `b`: only `ERROR tick-b` lines remain, without prefix.
    - Select `init:init1`: one line `init-done`.
11. **Previous**: on `logger`, check `Previous`. The text shows `No previous logs — this container has not been restarted.`
12. **Single-container pod hides the select**: on `logger` there is no container dropdown, only the Previous and Auto-scroll chips.

### Describe
13. **Describe**: press `2`. The pod JSON appears with keys in the accent color (`"apiVersion"`), strings in green, and numbers in amber. Timestamps read exactly `"2026-10-15T10:41:26Z"` in green (scenario `20-details-bug-describe-timestamps`).
14. **Describe search**:
    - Press `⌘F`, type `containerPort`: the counter shows `1/k` and the current match has a stronger amber background.
    - Type `zzzz`: red `No results`.
    - Press Esc (input focused): the search closes and the view stays on the pod.

### YAML
15. **YAML read-only**: press `3`.
    - The toolbar shows `YAML [🔒 Read-only]` and `Copy`.
    - The editor shows YAML with no `managedFields`, and typing does not change the text.
    - The gutter numbers line up with the text rows (Deliberate Fix 15; scenario `20-details-fix-yaml-gutter`).
16. **Edit / diff / reset**:
    - Click `Read-only`: it changes to `Editing` (accent).
    - Add a label line under `metadata.labels` (for example `    foo: bar`). The `Diff`, `Reset` and `Apply` buttons appear, plus an amber `Unsaved changes` pill.
    - Click `Diff`: a pane shows `Diff: 1 additions, 0 deletions` with a green `+` row.
    - Click `Close diff`, then `Reset`: the text is restored and the extra buttons disappear.
17. **Apply**:
    - Edit to add a label, click `Apply`. A yellow dialog `Apply YAML Changes` appears; click `Apply`.
    - Toast `Applied successfully`. The mode returns to `Read-only`.
    - `kubectl get pod logger --show-labels` shows the new label.
18. **Toggle discards**: enter Editing, change text, click `Editing`. The text reverts and the mode is Read-only.
19. **YAML search**: press `⌘F`, type `image`. Amber boxes appear behind the matches and the counter reads `1/k`.

### Metrics
20. **No metrics-server**: press `4`. The tab briefly shows `Loading metrics...`, then a large Activity icon, `Metrics Server not detected`, and `Make sure the Metrics Server is installed in your cluster`.
21. **With metrics-server** (optional; install it with `--kubelet-insecure-tls`):
    - Two cards appear: `CPU Usage` (`N m / M m`) and `Memory Usage` (`x.xx MB / y.yy MB`), each with a ring gauge and a centered percentage. With no limits set, both show `50.0%`.
    - A memory limit given as plain bytes (`200000001`) shows `/ 190.73 MB`; a CPU limit `1` shows `/ 1.00 cores` (scenario `20-details-bug-metrics-bytes`).
    - After 5–10s the `Usage Trend (Last 2 Minutes)` chart appears with a green line and a blue line.

### Events
22. **Events**: press `5`. Cards appear for `Scheduled`, `Pulled` (or `Pulling`), `Created` and `Started` with blue Info icons and `Ns ago`/`Nm ago` times, plus `Live` and `History` toggles at the top. On a resource with no events, `No events found` shows below the toggles and both modes stay reachable (scenario `20-details-bug-events-toggle`).
23. **History** (Deliberate Fix 10): click `History`. Range buttons `1h 6h 24h 7d` appear with `1h` active, and the stored events for the pod are listed (for a pod created while the app runs: `Scheduled`, `Started`, …). If nothing is stored for the range, the empty state `No events found` shows below the toggle, and `Live` switches back.

### Shell
24. **Exec**:
    - Press `6`. The bar shows `SHELL: [sh] bash`, then `Connecting...` followed by green `Connected`. A prompt appears in the terminal.
    - Click the terminal, type `echo hello` and Enter: `hello` is printed.
    - `stty size` prints the real rows and columns of the tab (Deliberate Fix 7).
    - The port sidebar is not visible on this tab.
25. **Exit**: type `exit` and Enter. The yellow text `Session ended.` appears in the terminal, and the status reads `Session ended`.
26. **Switch shell**: click `bash` on the `logger` (busybox) pod. The terminal clears and a new session starts. With busybox, bash is missing, so expect immediate `Session ended.` or a red error.

### Port forward
27. **Open sidebar**: on an nginx pod, click `Ports`. A 320px panel slides in with `Port Forwarding`, `Pod Port` = 80 and `Local Port` = 8080, and the empty text `No active port forwards`.
28. **Start forward**:
    - Set Local Port to `18080`, click `Start Forward`.
    - A card shows `CONNECTING`, then `LIVE` (green, pulsing). It reads `Local: 127.0.0.1:18080 ↗`, `Pod: <pod>:80`, `Connected`.
    - Toast `Port forward active on :18080`. The `Ports` header button turns green with a pulsing dot.
    - `curl 127.0.0.1:18080` returns the nginx page, also when kubectl's current-context differs from the context selected in Kore (Deliberate Fix 6).
29. **Validation**:
    - Pod Port `70000`: error box `Pod port must be a valid number between 1 and 65535`.
    - Local Port `18080` again while active: `Port 18080 is already in use by another port forward`.
30. **Auto port**: clear Local Port and start. The card shows a port between 49152 and 65535.
31. **Stop**: click the card's `X`. The card disappears, and the Ports button loses its green state once no active forwards remain.

### Debug container
32. **Open modal**: on an nginx pod press `b`.
    - The modal `Debug Container` reads `Inject an ephemeral container into <pod>`.
    - Options `netshoot` (selected), `busybox`, `ubuntu`, `curl`, `Custom`; a `Share process namespace` checkbox; `Cancel` and `Create & Connect` buttons.
    - Press Esc: the modal closes and the view stays on the pod. Press `b` to reopen (it starts fresh with `netshoot` selected).
33. **Custom validation**: select `Custom` with an empty input. `Create & Connect` is disabled (dimmed).
34. **Create**:
    - Select `busybox`, click `Create & Connect`. The loading state shows `Creating debug container...` and the chip `busybox:latest`.
    - When ready, the modal closes and the view switches to Shell. Toast `Debug container "debugger-xxxxxxxx" is ready`.
    - A green banner `Debug: debugger-xxxxxxxx [busybox:latest]` appears. The header button reads `Stop Debug` with a green dot, and the Shell tab has an accent pulsing dot.
    - The terminal shows a busybox prompt.
35. **Restore on re-open**: click outside the terminal, press Esc (goes back), then re-open the same pod. The banner and `Stop Debug` reappear once the describe call loads.
36. **Stop debug**: click `Stop Debug`. Toast `Debug container stopped`; the banner disappears and the button reads `Debug`.
37. **Target container**: open the modal, check `Share process namespace`. A `TARGET CONTAINER` select appears listing `nginx`.

### Delete pod
38. **Delete with key**: on `logger` press `d`.
    - Red dialog `Delete Pod`: `Are you sure you want to delete pod "logger" in namespace "default"? This action cannot be undone.`
    - Press Esc: nothing happens (the dialog stays, the view stays). Click `Cancel`: the dialog closes.
    - Press `d` again and click `Delete`.
    - After the watch drops the pod, the view shows `Pod No Longer Exists` with the red alert icon, the badge reads `Deleted`, and the tabs and actions are hidden.
    - Click `Go Back`: the table returns.

### Deployment details
39. **Open deployment**: switch to Deployments and open `nginx`.
    - Header: `← Back Esc`; a stepper showing `2`; `Scale`; `Restart`; `Delete`.
    - Grid: `NAME nginx`, `NAMESPACE default`, `READY 2/2`, `UP-TO-DATE 2`, `AVAILABLE 2`.
    - Tabs: `Describe YAML Rollback Logs Events`, with Describe active and the JSON shown.
40. **Scale**: click `+` (the field shows 3), then `Scale`. Toast `Scaled to 3 replicas`; `kubectl get deploy nginx` shows 3 desired.
41. **Restart** (Deliberate Fix 16): click `Restart`. Yellow dialog `Restart Deployment`; click `Restart`. Toast `Rolling restart initiated`, the dialog closes, and the pod template gains `kubectl.kubernetes.io/restartedAt`.
42. **Rollback timeline** (Deliberate Fix 16):
    - Run `kubectl set image deploy/nginx nginx=nginx:1.25`, then press `3`.
    - `Revision History` shows count 2 (or more after restarts). The top card `#N CURRENT` shows image `nginx:1.25`; an older card shows `nginx`.
    - Click the older card: it expands with Image, Created, Replicas, `Change Cause: No annotation`, `View YAML` and `Rollback`.
    - Click `View YAML`: a `REPLICASET YAML` box appears with highlighted YAML, and the button reads `Hide YAML`.
    - Click `Rollback`, then `Rollback` in the dialog. Toast `Rolled back to revision N`; the dialog closes, the list refreshes and the top card's image is `nginx`.
43. **Deployment logs tab** (Deliberate Fix 5): press `4`.
    - The toolbar reads `default/app=nginx` and `2 pods`, with `Filter`, `Copy` and `Auto-scroll`.
    - Lines appear with colored pod-name prefixes and `|` separators.
    - `Filter` lists both pods with colored dots; `None` shows `All pods are hidden. Adjust filters to see logs.` and `(2 hidden)`; `All` restores; unchecking one pod hides its lines and shows `(1 hidden)`.
44. **Deployment logs, real selector**: on a deployment whose pods lack an `app=<name>` label (selector e.g. `team=kore,tier=bugsel`) with two containers, the Logs tab reads `default/team=kore,tier=bugsel` and shows lines from both containers, prefixed `[c-alpha] ` / `[c-beta] ` (scenario `20-details-bug-deployment-selector`). A selector that matches no pods shows the red mono error `Validation error: No pods found matching the label selector`.
45. **Deployment delete**: click `Delete`, then `Delete`. Toast `Deployment deleted` and the view returns to the table.

### Generic resource details
46. **ConfigMap**: open the `demo` configmap.
    - Header shows `NAME demo`, `NAMESPACE default`, `KIND Configmaps`, and `Delete D`.
    - Tabs `Describe YAML Events`; the describe JSON shows `"data": {"key": "value"}`.
47. **Secret reveal**:
    - Open `demo-secret`. A `REVEAL:` bar shows the chip `👁 password`, and the JSON shows `"password": "***"`.
    - Click the chip: it turns accent with the eye-off icon, and the JSON shows `"password": "hunter2"`. Click again to hide.
48. **Generic delete dialog**: on a service press `d`. Title `Delete Service`. Confirm: toast `Resource deleted`, then back to the table.
49. **Node**: open a node. The namespace cell shows `default`; Describe shows the node JSON; Events lists node events if any (namespace `default`).
50. **Generic tab keys**: `1`/`2`/`3` switch Describe, YAML and Events. `4` does nothing.
