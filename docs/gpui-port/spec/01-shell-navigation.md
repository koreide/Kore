# 01 — App Shell & Core Navigation

Behavior and visual spec of the native app's shell: boot, layout, routing, global keyboard,
sidebar, header, resource table, palette, overlays, dialogs, banners and Settings. The behavior
was originally ported from the legacy Tauri/React app (in git history at commit `3a66870`).

Implementation (all paths under `crates/kore-app/src/`):
`main.rs` (window), `root.rs` (boot gating, layout, routing, overlays, global keys), `keys.rs`
(key normalisation, text-entry focus registry), `theme.rs` (tokens), `state/app_state.rs`
(`AppState`, navigation API, update check, pins, confirm), `state/resources.rs` (`ResourceStore`:
list + watch, sort, search, restart history), `state/resource_item.rs` (`ResourceItem`, age,
status variants), `state/settings.rs`, `state/pins.rs`, `state/storage.rs`, `state/ai_config.rs`,
`state/types.rs` (`AppView`, `KindExt`), `views/{sidebar, header, resource_table,
connection_setup, banners, command_palette, shortcut_overlay, settings, ai_settings}.rs`,
`ui/{searchable_dropdown, toast, confirm_dialog, text_search_bar, widgets, scrollbar, input,
native_controls, text, mono_text, command_score}.rs`. Backend calls go through
`backend::core(cx)` to `kore_core::Core` (§19).

Read `docs/gpui-port/CONVENTIONS.md` for the view constructor contract, the navigation API,
keyboard rules, theme helpers and automation ids. This spec gives the rendered values (px, hex,
`theme.rs` token names) and the behavior; if this spec and the code
disagree, the code is the source of truth and this spec should be fixed.

> **Accent tints.** The design deliberately has **no accent-opacity tints**: where an element is
> "accent-highlighted" it gets accent text/icons only, and where it has an accent-ish border, that
> border is the light gray **`theme::PREFLIGHT_BORDER` `#e5e7eb` (1px)**. This applies to: the
> active Filter button, active Multi-Cluster toggle, label chips, the update banner's bottom
> border and "Update" button, Settings "Add"/"OK" buttons, the AI "Test Connection" button, the
> ConnectionSetup "Retry" button and selected context row, and the boot spinner ring. Elements
> that are "selected" (logo "K" tile, sidebar count pill, highlighted dropdown row, AI segmented
> selection, sidebar Settings gear) have **no background tint**. Keep it this way unless the
> design is intentionally changed (harness screenshots compare against it).
>
> **Focus ring.** A focused single-line text box gets a 2px accent@40% ring
> (`Theme::focus_ring()` = accent with alpha 0.4, as a 2px-spread box shadow) and **keeps its
> normal corner radius** (header search 8px, dropdown search 4px; the borderless label-filter
> input gets 4px). The palette input and the Settings / AI text boxes draw
> `views::ai_chat::focus_ring(accent)` instead: a 2px accent@40% border inset −3px around the box
> (Settings / AI boxes: radius = box radius + 3px, palette 6px); the box keeps its radius.

---

## 0. Design tokens

### 0.1 Colors (resolved)

| Token (`theme.rs`) | Value |
|---|---|
| `BACKGROUND` | `#0b1221` |
| `SURFACE` | `#101828` |
| `MUTED` | `#1d2939` |
| accent (`theme::accent(cx)`) | default `#58d0ff` (`DEFAULT_ACCENT`); user-selectable (Settings, `ACCENT_CHOICES`): `#58d0ff` Cyan, `#a78bfa` Violet, `#34d399` Emerald, `#fb923c` Orange, `#f87171` Red. Stored in the `Theme` global; set at startup from settings and on every settings change (`Theme::set_accent` + window refresh). Never hard-code it. |
| `PREFLIGHT_BORDER` | `#e5e7eb` (light-gray 1px border, see header note) |
| `PREFLIGHT_PLACEHOLDER` | `#9ca3af` (default input placeholder) |
| `GLASS_BG` | `rgba(16,24,40,0.7)` |
| `GLASS_BORDER` | `rgba(255,255,255,0.06)` |
| `GLASS_OVER_DIM` | `#0d1320` (opaque fill for overlay panels; glass composited over the dimmed backdrop) |

Palette modules:
`slate::S100 #f1f5f9`, `S200 #e2e8f0`, `S300 #cbd5e1`, `S400 #94a3b8`, `S500 #64748b`, `S600 #475569`,
`S700 #334155`, `S800 #1e293b`, `S900 #0f172a`;
`emerald::E300 #6ee7b7`, `E400 #34d399`, `E500 #10b981`; `amber::A200 #fde68a`, `A300 #fcd34d`,
`A400 #fbbf24`, `A500 #f59e0b`; `red::R300 #fca5a5`, `R400 #f87171`, `R500 #ef4444`, `R600 #dc2626`,
`R700 #b91c1c`; `misc::ORANGE400 #fb923c`, `ORANGE500 #f97316`, `BLUE400 #60a5fa`, `YELLOW500 #eab308`,
`PINK400 #f472b6`, `PINK500 #ec4899`.

Translucent colors are written `theme::hexa(0xRRGGBB, a)`; this spec writes them as
`rgba(r,g,b,a)`, e.g. `hexa(0x1e293b, 0.5)` = `rgba(30,41,59,.5)`, `hexa(0x1d2939, 0.3)` =
`rgba(29,41,57,.3)`, `hexa(0x101828, 0.6)` = `rgba(16,24,40,.6)`, `hexa(0x334155, 0.6)` = `rgba(51,65,85,.6)`.

### 0.2 Effects

* **Glass** (`KoreStyled::glass()`): bg `GLASS_BG`, 1px `GLASS_BORDER` on all four sides, shadow
  `theme::glass_shadow()` = `0 0 0 1px rgba(255,255,255,.02)` + `0 4px 24px rgba(0,0,0,.3)`.
  Over `#0b1221` it composites to ≈ `#0f1626`. Glass always overrides any bg/border of the element.
* **No backdrop blur** (GPUI has none). Full-window overlays use a darker plain tint instead
  (palette/shortcut overlay black@0.9, up-to-date modal black@0.85, confirm dialog black@0.6),
  and overlay panels use the opaque `GLASS_OVER_DIM` fill so nothing behind shows through.
* **No noise overlay.**
* **Scrollbars** (`ui::scrollbar`): 6px (`scrollbar::WIDTH`), transparent track, thumb `#334155`
  radius 3px, `#475569` when hovered or dragged, min thumb height 20px, draggable. They take layout
  space: owners reserve a 6px gutter when `Gutter::overflows()`.
* **Focus ring**: see header note.
* **Skeleton shimmer** (`widgets::skeleton_bar`): base `rgba(30,41,59,.4)`, a gradient band
  `rgba(30,41,59,.4)` → `rgba(51,65,85,.4)` sweeping left→right, 1.8s ease-in-out, repeating,
  radius 4px.
* **Hover transitions** are instant (no color fade).
* **Spinner** (`widgets::spinner(id, size, ring, arc)`): round, 2px ring in `ring`, an arc in
  `arc` rotating 360° per 1s linear. `widgets::spinning_icon` rotates a Lucide icon the same way.
* **Ping halo** (`widgets::ping`, running status dot): 1s repeating; during the first 75% the halo
  scales 1→2 and fades .75→0 (ease-out), then stays invisible.
* Shadows used: confirm dialog `0 20px 25px -5px rgba(0,0,0,.1)` + `0 8px 10px -6px rgba(0,0,0,.1)`;
  TextSearchBar `0 10px 15px -3px rgba(0,0,0,.1)` + `0 4px 6px -4px rgba(0,0,0,.1)`.

### 0.3 Typography

* Sans (`theme::SANS`): macOS `.SystemUIFont` (SF Pro), Windows `Segoe UI`, Linux `Inter`.
* Mono (`theme::MONO`): macOS `.AppleSystemUIFontMonospaced` (SF Mono), Windows `Consolas`,
  Linux `DejaVu Sans Mono`.
* Root (`root.rs`): 16px, line-height 1.5 (relative), color `slate::S100`, bg `BACKGROUND`, sans.
* Sizes (`KoreStyled`): `text_9()` / `text_10()` / `text_11()` (size only; line height inherited),
  `text_xs_tw()` 12/16, `text_sm_tw()` 14/20, `text_base_tw()` 16/24, `text_lg_tw()` 18/28,
  `text_xl_tw()` 20/28, `text_2xl_tw()` 24/32. Elements with no size inherit 16px/24px (notably
  the SearchableDropdown trigger and rows, and palette group headings).
* Weights: medium 500, semibold 600, bold 700.
* Letter spacing / uppercase: `ui::text::caps(text, size)` = uppercase + 0.05em tracking (all
  "UPPERCASE" headings in this spec); `ui::text::tracked(text, size, em)` for other trackings
  (0.025em dropdown labels, −0.025em ConnectionSetup "Kore").
* **Mono pitch**: GPUI draws SF Mono at its own 0.618em advance, but the design is laid out at a
  **0.601em** pitch (`theme::MONO_ADVANCE_EM`, 7.21px at 12px). Single-line mono text that must
  line up (table cells, context pill) uses `ui::mono_text::mono_text(text)` (`.truncate(max)` for
  ellipsis); wrapped mono uses `ui::mono_text::mono_paragraph`. Column-width math uses the same
  constant.

### 0.4 Spacing / radius

All values in this spec are logical px. Radii in use: 3 (scroll thumb), 4, 6, 8, 12 and fully
rounded. Max widths in use: 320, 384, 448, 512, 576.

### 0.5 Window

`main.rs`: one window titled "Kore", 1400×900 centered, resizable, native decorations,
`app_id` `dev.kore.desktop`. On macOS the titlebar is transparent over a `#0b1221` window
background (`platform::style_window`) and the Dock icon is set for bare binaries. Closing the last
window quits; on quit `Core::shutdown()` cancels watches/streams/port-forwards (bounded 2s wait).
In `--features test-harness` builds, `KORE_CONTROL_SOCKET` starts the Layer 2 control server, and `KORE_BACKGROUND` opens the window without focusing or activating the app (CONVENTIONS "Automation ids").

---

## 1. Boot sequence & top-level gating

`main.rs` loads `Settings` (§17), initialises the `Theme` accent from it, creates `Core`
(`backend::init`, which also enriches `PATH` so `kubectl`/`helm` resolve), opens the window with
`root::KoreApp`, which creates `AppState`, `Sidebar`, `Header`, `ResourceTable` and calls
`AppState::boot`.

1. `Core::get_connection_status()` → `ConnectionStatus {connected, error, error_kind,
   kubeconfig_path, contexts_available[], current_context}`. If the call itself fails, the status
   is treated as `connected: false, error_kind: "unknown"` with the error text.
2. While `AppState::connection` is `None`: full-window `BACKGROUND`, centered `spinner` 24px,
   ring `PREFLIGHT_BORDER`, arc accent (automation id `boot.spinner`).
3. If not connected: render **ConnectionSetup** (§12) only (no sidebar, global shortcuts off).
   `contexts_available` is still stored in `AppState::contexts`.
4. If connected: `Core::list_contexts()`. Empty → contexts cleared, nothing else (no context:
   header pill "No context", table stays empty). Otherwise the initial context
   (`app_state::initial_context`) is: the single favorite context if the favorites
   (`Core::load_favorites("kore-favorite-contexts")`, filtered to existing contexts) contain
   **exactly one** context; else kubeconfig's **`current-context`** (`ConnectionStatus::
   current_context` from step 1) if it is in the list; else the first context. Then
   `switch_context_raw(name)`:
   * bumps a context sequence counter (stale async results are dropped), clears the namespace
     list and the resource store;
   * `Core::switch_context(name)`; on error only logs (context stays unset); on success sets
     `context`, emits `AppEvent::ContextChanged`, loads namespaces (step 5) and refreshes the
     resource list (§7.1);
   * then `Core::get_connection_status()` again and stores it (if still current).
5. Namespaces (`load_namespaces`): list cleared, `Core::list_namespaces()`. If it returns a
   non-empty list: favorites `Core::load_favorites("kore-favorite-namespaces")` filtered to
   existing names or `"*"`; next namespace (`app_state::next_namespace`) = the Settings
   **"Default Namespace per Context"** entry for the new context if it is `"*"` or in the list,
   else the single favorite if exactly one, else keep the current one if it is `"*"` or still
   present, else `"default"`; the list is replaced and
   `set_namespace(next)` runs if it changed (which refreshes the table again; the earlier refresh
   is superseded). If the call fails or returns empty → fallback list
   `["default","kube-system","kube-public","kube-node-lease"]` (namespace unchanged). Initial
   namespace state `"default"`, initial list = the fallback list.
6. Initial view: `AppView::from_setting(settings.default_view)`: `table`, `dashboard`, `graph`,
   `crds`, `helm`, `settings`, `network-policies`, `rbac` map to that view; anything else
   (including the default `"chat"`) → **AI Chat**. Initial kind: Pods.
7. Accent: `settings.accentColor` parsed as `#RRGGBB`; invalid → `#58d0ff`.
8. Update check (§13.5) starts in parallel.

### 1.1 Error handling (no error boundary)

There is no error-boundary screen. Backend errors are returned as `Result`s and surface as
logs, empty states, inline errors or toasts (per feature); a panic in the UI terminates the app.

---

## 2. Global layout (connected state)

```
┌──────────────────────────────────────────────────────────────────────────┐
│ [update banner, full width]                                              │
├──────────┬───────────────────────────────────────────────────────────────┤
│ Sidebar  │ main (flex-1, column, overflow hidden)                        │
│ 256px    │  [RBAC impersonation banner, if active]                       │
│          │  ┌ content: padding 16px, column, gap 12px ───────────────────┐│
│          │  │ header (only in table/details views) or empty 0-height div ││
│          │  │ section (flex-1, overflow hidden, relative) — active view  ││
│          │  └────────────────────────────────────────────────────────────┘│
└──────────┴───────────────────────────────────────────────────────────────┘
overlays (painted above, in this order): AI panel, Command palette, Shortcut overlay,
ConfirmDialog, Toasts
```
Root (`KoreApp::render`): full window, `overflow hidden`, bg `BACKGROUND`, text `slate::S100`,
sans 16px / 1.5. Even when the header is hidden, the 12px gap remains above the view (top inset =
16 + 0 + 12 = 28px). A left mouse-down anywhere while a text input has focus blurs it (focus
returns to the root), like clicking outside an input.

### 2.1 View routing (`AppState::view: AppView`)

`AppView = Table | Details | Dashboard | Graph | Crds | Helm | HelmDetail | Settings | Chat |
NetworkPolicies | Rbac` (serialized kebab-case: `table`, `details`, …, `helm-detail`,
`network-policies`). `root.rs` derives a route key and renders:

| view | Rendered |
|---|---|
| chat | `views::ai_chat::AiChatView` (uses `AppState::namespace`; `"*"` = none) |
| dashboard | `views::dashboard::DashboardView` (reads `multi_cluster`) |
| graph | `views::graph::GraphView` |
| network-policies | `views::network_policy::NetworkPolicyView` |
| rbac | `views::rbac::RbacView` |
| crds | `views::crd_browser::CrdBrowserView` |
| helm | `views::helm_releases::HelmReleasesView` |
| helm-detail (+release) | `views::helm_detail::HelmDetailView` (back → helm); without a release renders nothing |
| settings | `views::settings::SettingsView` (§17) |
| table | the persistent `ResourceTable` entity (skeleton while loading, §7) |
| details | pods → `PodDetailsView`; deployments → `DeploymentDetailsView`; others → `ResourceDetailsView` (spec 02). Details without a selection renders the table. |

Every routed view except the table is created when its route key is entered and dropped when it
changes (state resets on re-entry). Details views are keyed by `(kind, namespace, name)`,
helm-detail by `(namespace, name)` of the release. Transitions are an **instant swap** (no
enter/exit motion). Overlays are created when `palette_open` / `shortcuts_open` / `ai_panel`
become set and dropped when cleared.

### 2.2 State transitions (exact)

All via `AppState` (CONVENTIONS "Navigation API"); each notifies observers and emits `AppEvent`s.

| Action | Method | Effects |
|---|---|---|
| Sidebar resource kind click | `set_kind(k)` | kind=k, view=table, selected=None, labels=[], label bar hidden. Search text **kept**. Refreshes only if the kind changed or labels were set. |
| Sidebar view click / palette view | `set_view(v)` | view=v, selected=None; v=helm also clears the helm release. |
| Context switch (sidebar dropdown or palette) | `switch_context(name)` | selected=None, search="", view=table, labels=[] (label bar visibility unchanged), then §1 step 4 switch. |
| Namespace select | `set_namespace(ns)` | namespace=ns only (view unchanged — a details view stays). No-op if unchanged; else refresh. |
| Row select (click/Enter/`l`) | `open_resource(None, row)` | selected=row; view=details iff row.name non-empty && (row.namespace non-empty OR kind ∈ {nodes, events, namespaces}). |
| Back (`h`, Esc, Back buttons) | `back()` | helm-detail → view=helm, release=None; else view=table, selected=None. |
| Palette "Go to X" / kind quick filter | `go_to_kind(X)` | kind=X, view=table, selected=None (labels NOT cleared); refresh if the kind changed. |
| Palette search result / recent | `open_resource(Some(k), item)` | kind=k (refresh if changed), selected=item, view=details. |
| Pinned item click | `open_resource(kind, {name, namespace})` | as above (unknown kind string → current kind kept). |
| Dashboard navigate | `open_resource(Some(Pods), {name, namespace})` | pod details. |
| Graph navigate | `open_resource(Some(k), {name, namespace})` | kind map: Deployment→deployments, ReplicaSet→deployments, Service→services, Ingress→ingresses, Job→jobs, CronJob→cronjobs, other→pods. |
| Row action Delete | `request_delete(kind, row)` | ConfirmDialog (§10). |
| Row action logs/exec/port-forward/describe/scale/restart | `open_resource(None, row)` | details view; no specific tab is pre-selected. |

---

## 3. Keyboard shortcuts (global, actual behavior)

Global keys are handled in `root.rs::on_key`, registered with `keys::on_window_key` (runs before
the focused element; returning `true` consumes the key). They are inactive while not connected.
"In input" = a registered text-entry surface has focus (`keys::text_entry_focused`: every
`ui::input::TextInput`, an open `NativeSelect`, the exec terminal, the YAML editor). "Modal" =
`AppState::modal_open()`: palette, shortcut overlay, confirm dialog, or any surface registered with
`set_modal` (open dropdowns `dropdown:<id>`, selects `select:<id>`, AI panel, debug-container
dialog, Settings up-to-date modal, RBAC pickers). Mod = ⌘ on macOS, Ctrl elsewhere (either is
accepted); hint labels come from `keys::{MOD, SHIFT, MOD_K, MOD_R, MOD_F}`.

Evaluated in this order:

| Key | Where active | Behavior |
|---|---|---|
| Mod+K (no Shift) | everywhere, even in inputs | toggle Command Palette. |
| Mod+Shift+R | not in input | view=rbac (**Deliberate Fix 17**: does not also refresh). In an input the key is not handled. |
| Mod+R | everywhere, even in inputs | `refresh_resources()` (clears list, skeleton ≥600ms, refetch, restart watch) and emits `AppEvent::Refresh`. |
| Esc | palette open | close palette (consumed). |
| Esc | shortcut overlay open | close overlay (consumed). |
| Esc | confirm dialog open | cancel it (`dismiss_confirm`; ignored while its action runs), consumed. |
| Esc | in input or modal | not consumed by root: goes to the focused input/modal (closes a dropdown, a TextSearchBar, the AI panel, a select…); **no back-navigation** (**Deliberate Fix 4**). |
| Esc | otherwise | emits `AppEvent::Escape`; Settings, pod/deployment/resource details, helm detail and the CRD browser navigate back (pod details first closes an open log search). |
| any plain key | in input | goes to the input (**Deliberate Fix 1**: `/`, `?`, `j`… are typeable everywhere). |
| `/` | not in input | consumed; focuses the header search if view ∈ {table, details} and no modal. |
| `?` | not in input | if the overlay is open → close; else if the palette is closed → open Shortcut Overlay. |
| — | palette/overlay/modal open | nothing below fires. |
| `h` | view=details | `back()`. |
| `j` | view=table | highlight = min(i+1, n−1) (from −1 → 0). No scroll-into-view. |
| `k` | view=table | highlight = max(i−1, 0). |
| `l` | view=table, valid index | open the highlighted row. |
| ↓ / ↑ | view=table, not loading, ≥1 row | move highlight (↑ from −1 → 0) and scroll the row into view (`ScrollStrategy::Nearest`). |
| Enter | view=table, not loading, valid index | open the highlighted row. |
| `d` | view=table, valid index | `request_delete(kind, highlighted row)`: the same "Delete <Kind>" ConfirmDialog as the row menu's Delete (§10). (Pod and generic resource details bind `d` to their Delete — spec 02; deployment details do not.) |
| 1–6, `b`, Mod+F, `i` | view-local | detail tabs, debug container, TextSearchBar (§16) in logs/describe/YAML, RBAC impersonation — specs 02/03. |

**Deliberate Fix 2**: `j`/`k`/`l`, ↑/↓ and Enter all index the same list — the visible rows
(search-filtered **and sorted**, `AppState::visible_rows`) — so `l` opens exactly the highlighted
row. `selected_row_index` lives in `AppState`, is **not reset** on kind/namespace change, and the
table clamps it to `rows.len()−1` (or 0) when it exceeds the row count.

---

## 4. Sidebar (`views/sidebar.rs`)

Container: width **256px** (`sidebar::WIDTH`), full height, column, glass (bg `GLASS_BG`, 1px
`GLASS_BORDER` on all sides, glass shadow).

### 4.1 Branding block
Padding 16px top/left/right, 12px bottom; bottom border 1px `rgba(30,41,59,.5)`. Row, gap 12px,
items centered:
* Tile 32×32, radius 8px, no background, centered "K" 14/20 bold accent.
* Column: "Kore" 14/20 semibold `slate::S100`; "Kubernetes Desktop" 10px `slate::S500`.

### 4.2 Dropdowns block
Padding 16px, column gap 16px, non-shrinking. Contains:
1. `SearchableDropdown` id `sidebar.context`, label **"Context"**, items = `AppState::contexts`,
   placeholder "Select context...", favorites key `kore-favorite-contexts`. Select →
   `switch_context`.
2. `SearchableDropdown` id `sidebar.namespace`, label **"Namespaces"**, items =
   `AppState::namespaces`, all-option `("All Namespaces", "*")`, placeholder "Select namespace...",
   favorites key `kore-favorite-namespaces`. Open → `refresh_namespaces()` (`Core::list_namespaces`,
   replaces the list if non-empty). Select → `set_namespace`.
3. **Multi-Cluster toggle** (`sidebar.multi-cluster`): full width, row gap 8px, px 12 py 6,
   radius 6px, 12/16, 1px border.
   * `layers` 14px · label "Multi-Cluster" (flex-1, left) · dot 8×8 round.
   * Off: border `slate::S800`, text/icon `slate::S500`, dot `slate::S700`; hover text/icon
     `slate::S300`, border `slate::S700`.
   * On: text/icon accent, border `PREFLIGHT_BORDER`, dot accent.
   * Click → `toggle_multi_cluster()` (refreshes the table). Session-only.

### 4.3 Scrollable nav (flex-1, vertical scroll with 6px scrollbar gutter, horizontal padding 16px)
Section heading: `caps(label, 10)`, 10px medium `slate::S500`, margin-bottom 6px. Sections
margin-bottom 12px. Items column gap 2px.

Nav item: full width, px 12 py 6, radius 6px, row gap 10px, 14/20 → **32px tall**; icon 16px;
label flex-1 left.
* Inactive: text/icon `slate::S400`; hover text/icon `slate::S200` + bg `rgba(29,41,57,.3)`.
* Active: text/icon accent, no hover change.
* **Active indicator**: one shared bar, 2px wide, fully rounded, accent, at the nav column's left
  edge, top = item top + 2px, height = item height − 4px. When the active item changes it
  **slides** from the previous item's position/height to the new one over 450ms with a spring
  (stiffness 500, damping 35), across sections. Hidden when no nav item is active (Settings,
  helm-detail).
* Active resource item shows a right pill: 10px medium, px 6 py 2, fully rounded, text accent,
  no bg — value = number of rows currently shown (after the search filter).

Sections and items (exact order, Lucide icon; automation ids `sidebar.view.<view>` /
`sidebar.kind.<kind>`):
* **VIEWS** (active iff `view == id`): AI Chat `message-circle` (chat) · Dashboard
  `layout-dashboard` · Graph `git-branch` · Network Policies `shield` · RBAC `shield-check` ·
  Custom Resources `puzzle` (crds) · Helm `package` (helm; NOT active during helm-detail).
* **PINNED** (only if ≥1 pin): heading = `star` 12px `slate::S500` + "Pinned" (gap 4px), then the
  pinned rows (§9) — a single heading. Section headings carry automation ids
  `sidebar.section.<pinned|workloads|network|config|cluster>` (role `heading`).
* **WORKLOADS**: Pods `boxes` · Deployments `server` · Jobs `timer` · CronJobs `calendar`.
* **NETWORK**: Services `activity` · Ingresses `globe`.
* **CONFIG**: ConfigMaps `file-text` · Secrets `lock`.
* **CLUSTER**: Nodes `cpu` · Namespaces `folder-open` · Events `zap`.

Resource items are active iff `kind == id && view ∈ {table, details}`.

### 4.4 Footer
Padding 16px, top border 1px `rgba(30,41,59,.5)`; row space-between.
* Left (gap 8px, 11px `slate::S500`): `command` icon 12px, then a kbd with `keys::MOD_K` ("⌘K" on
  macOS, "Ctrl+K" elsewhere; px 4 py 2, bg `rgba(30,41,59,.8)`, radius 4px, 10px mono
  `slate::S400`, line height 12px, no border) + " Command Palette". Not clickable.
* Right (gap 4px):
  * Heart button (`sidebar.sponsor`): p 6, radius 6px, `heart` 16px `slate::S500`; hover icon
    `#f472b6` + bg `rgba(236,72,153,.1)`. Opens `https://github.com/sponsors/eladbash` in the
    system browser (`platform::open_url`: `open` / `xdg-open` / `cmd /C start`).
  * Settings button (`sidebar.settings`): p 6, radius 6px, `settings` (gear) 16px. Active
    (view=settings): icon accent, no bg. Inactive `slate::S500`, hover `slate::S300` + bg
    `rgba(29,41,57,.3)`. Click → `set_view(Settings)`.

---

## 5. SearchableDropdown (`ui/searchable_dropdown.rs`)

Wrapper relative. Label: 12/16, uppercase, tracking 0.025em, `slate::S400`, margin-bottom 8px.

Trigger (`<id>.trigger`, combobox): full width, px 12 py 8, radius 4px, 1px border, **16/24 text**
→ 42px tall. Row: [filled star 12px `#eab308` if the selected item is a favorite] + truncated
value … `chevron-down` 16px (accent with a selection, else `slate::S100`), rotated 180° while open.
* With selection: border accent, text accent.
* No selection: border `slate::S800`, hover `slate::S700`, text inherits `slate::S100`, shows the
  placeholder.
* The displayed value is the item's label: when namespace is `"*"` the trigger shows **"All
  Namespaces"** (the all-option label), otherwise the raw value.
* Click toggles. Opening resets highlight and scroll, focuses the search input, emits `Open`
  (namespace dropdown → refresh namespaces) and registers modal `dropdown:<id>`.

Panel (`<id>.panel`, when open): painted above everything (`deferred`), absolute at 70px below
the dropdown's top (24 label + 42 trigger + 4 gap), full trigger width, radius 6px, max height
320px, column, glass. Enter animation 150ms ease-out: opacity 0→1, y −4→0. No exit animation.
* Search row: padding 8px, bottom border `slate::S800`. Box height 34px, bg `rgba(15,23,42,.5)`,
  1px border `slate::S700` (focused: accent + focus ring), radius 4px. Input padding 32px left/right,
  6px top/bottom, 14/20 `slate::S100`, placeholder "Search..." `PREFLIGHT_PLACEHOLDER`. `search`
  icon 16px `slate::S400` at left 8px, vertically centered.
* **Ghost autocomplete**: if the first sorted item starts with the query (case-insensitive) and
  isn't equal to it, the remaining suffix is drawn in `slate::S600` right after the typed text.
  **Tab** accepts it (query = full item).
* List: scrollable with a 6px scrollbar gutter. Empty → "No items found" (px 12 py 8, 14/20
  `slate::S500`).
* Row (`<id>.item.<value>`): full width, px 12 py 8, **16/24 text**, row space-between gap 8px.
  Left: [filled star 12px `#eab308` if favorite] + label (truncate). The first case-insensitive
  match of the query is drawn accent, semibold. Right: star toggle (`<id>.star.<value>`), p 4,
  radius 4px, hover bg `slate::S700`; star 12px — favorite: filled `#eab308`; else outline
  `slate::S500`, hover `#eab308`. Clicking the star toggles the favorite without selecting and
  persists via **`Core::save_favorites(key, values)`** (`~/.kore/favorites.json`, values sorted).
* Row colors: highlighted (mouse-hovered or keyboard index) or selected → text accent, no bg;
  else `slate::S200`, hover bg `rgba(30,41,59,.5)`.
* "All Namespaces" row = value `*`, label "All Namespaces".
* **Sort**: favorites first; then (only with a query) items starting with the query; then
  case-insensitive alphabetical. Filter = case-insensitive substring of the value.
* Favorites are loaded when the dropdown is constructed via **`Core::load_favorites(key)`**.
* Close: select an item, mouse-down outside panel and trigger, or Esc — all reset the query,
  highlight and ghost and blur the input.
* **Keyboard (Deliberate Fix 3)**, from inside the search input: ↓/↑ move the highlight
  (clamped, scrolled into view), Enter selects the highlighted item (the first match by default),
  Esc closes, Tab accepts the ghost completion. Mouse hover also moves the highlight.
* Side effect of favorites: if exactly one favorite context/namespace exists, it is auto-selected
  on startup / context switch (§1).

---

## 6. Main header (`views/header.rs`, only when view ∈ {table, details})

Column, gap 8px. Row 1: flex, items centered, gap 12px.

1. **Breadcrumb** (`header.breadcrumb`; non-shrinking, gap 8px, 14/20):
   * Kind label = `KindExt::header_label()` (kind id with first letter uppercased): "Pods",
     "Deployments", "Services", "Nodes", "Events", **"Configmaps"**, "Secrets", "Ingresses",
     "Jobs", **"Cronjobs"**, "Namespaces" — `slate::S400`, medium.
   * Table view: count pill (`header.count`) — 10px medium, bg `rgba(30,41,59,.8)`, `slate::S400`,
     px 6 py 2, fully rounded; value = visible (filtered) row count.
   * Details view (with selection): `chevron-right` 14px `slate::S600` + resource name
     `slate::S100` medium, truncated at max 200px.
   * Table view with a palette status quick filter (§14): chip (`header.status-filter`, label
     `Status: <Running|Failed|Pending>`) — row gap 4px, px 8 py 2, fully rounded, 12/16, accent text,
     1px `PREFLIGHT_BORDER` (like the label chips); `filter` 12px + "Status: Failed" + `x` 12px
     (`header.status-filter.clear`, accent, hover `#f87171`) → clears the filter.
2. **Search box** (`header.search`): flex-1, max width 448px, horizontally centered. Box height
   34px, bg `rgba(16,24,40,.6)`, 1px border `slate::S800` (unchanged on focus), radius 8px (4px +
   focus ring while focused). Input padding 40px left/right, 6px top/bottom, 14/20 `slate::S100`,
   placeholder "Search..." `slate::S600`. `search` icon 16px `slate::S500` at left 12px; kbd "/"
   at right 12px.
   Filtering: case-insensitive substring match on **name OR namespace OR status**. Search persists
   across kind changes; cleared on context switch. Visible in details view too (no effect on
   detail content).
3. **Right cluster** (gap 8px, non-shrinking):
   * **Filter toggle** (`header.filter`, table view only): px 8 py 6, radius 8px, 1px border,
     `filter` icon 14px only. Inactive: border `slate::S800`, icon `slate::S400`, hover
     `slate::S200`. Active (bar shown OR ≥1 label): icon accent, border `PREFLIGHT_BORDER`.
     Click toggles the bar.
   * **Context pill** (`header.context`): row gap 8px, px 12 py 6, radius 8px, 1px `slate::S800`,
     bg `rgba(16,24,40,.6)`, 12/16 `slate::S300`; 6px dot `#34d399` (`#f87171` if disconnected);
     context name in mono (`mono_text`, truncated at 140px), or "No context". Not clickable.
   * **AI button** (`header.ai`): row gap 6px, px 8 py 6, radius 8px, 1px `slate::S800`,
     `slate::S400`, 12/16; `sparkles` 14px + "AI" (label hidden below 640px window width). Hover:
     text and icon accent. Click → `open_ai_panel` with `{kind, namespace (or "default"), name}`
     of the selection in details view, else an empty request (spec 03).
   * Kbd `keys::MOD_K` (static).
   * **Kbd style** (`widgets::kbd`): px 6 py 2, bg `rgba(30,41,59,.8)`, radius 4px, 10px mono
     `slate::S400`, line height 12px, 1px border `rgba(51,65,85,.5)`.
4. Row 2: **LabelFilterBar** (§8), shown when (bar toggled OR labels non-empty) AND view=table.

---

## 7. Resource table (`views/resource_table.rs`) and data store (`state/resources.rs`)

### 7.1 Data flow (`ResourceStore::refresh(Query {kind, namespace, label_selector, multi_cluster})`)
* `AppState::refresh_resources()` runs on context change, kind change (`set_kind` / `go_to_kind`
  / `open_resource` with another kind), namespace change, multi-cluster toggle, label add/remove
  and Mod+R. No-op while there is no context.
  1. generation += 1, current watch dropped, `loading = true`, rows cleared.
  2. wait **500 ms**.
  3. `Core::list_resources(kind, ns, selector)` — or **`Core::list_resources_multi_cluster`**
     (same args) when multi-cluster; `ns = None` for `"*"`.
  4. if the generation is stale, stop; else map through `to_resource_item` (§7.5), drop `None`s,
     record restart history (§7.10). Errors are only logged (no toast; the table shows the empty
     state).
  5. single-cluster only: wait 100 ms, then **`Core::start_watch(kind, ns, selector)`** →
     `Subscription<WatchEventPayload {action, kind, object}>`, drained in a loop that coalesces
     everything queued (up to 4096 events) into one UI update.
  6. `loading = false`.
* Watch events: ignored if `event.kind` ≠ current kind; the object goes through
  `to_resource_item` (objects without a name, e.g. `error` events, are ignored); `deleted`
  removes the row matching (name, namespace); anything else (`applied`, incl. re-list after a
  watch restart) upserts in place or appends at the end. Dropping the subscription (next refresh,
  context switch) cancels the backend stream.
* `label_selector` = labels joined by `,`.
* During loading the **skeleton** is shown (≥600ms on every refresh).

### 7.2 Skeleton (`resource_table::skeleton_table`, `table.skeleton`)
Full size, radius 8px, overflow hidden, glass. Header row: row gap 16px, padding 12px, bottom
border `slate::S800`; 5 shimmer bars height 12px, widths 16%, 14%, 10%, 10%, 8%. Then 8 rows
(padding 12px, gap 16px, bottom border `rgba(30,41,59,.5)`), row i opacity `1 − 0.08·i`, bar widths:
```
40 20 15 12 10 | 55 25 18 10 8 | 35 30 12 15 10 | 60 15 20 8 12
45 22 16 14 9  | 50 18 14 11 10 | 38 28 17 13 8 | 52 20 15 10 11   (percent)
```

### 7.3 Table visuals
* Container (`table`): full width/height, radius 8px, glass, overflow hidden; horizontal scroll
  for wide tables; rows are virtualized (`uniform_list`) with a 6px vertical scrollbar gutter.
* Column widths reproduce automatic table layout: each column's natural width is
  max(header text width (uppercase, 12px medium, 0.05em; +18px for the sort icon if sortable) +
  24, widest cell + 24), where cell text is measured at the mono pitch (12px × 0.601em per char);
  badges add 28px of chrome; MESSAGE is capped at 300px, CLUSTER at 120px; pin 22px, actions
  24px. If the total is narrower than the available width (window − 256 sidebar − 32 padding − 2
  border − scrollbar gutter), the extra space is distributed proportionally.
* Header (stays on top while rows scroll): height **33px**, bg `rgba(16,24,40,.8)`, bottom border
  `slate::S800`. Cell (`table.header.<id>`): px 12 py 8, left-aligned, 12/16 **sans** medium,
  caps 0.05em, `slate::S400`, pointer, hover text accent. Row gap 4px: label + sort icon (sortable
  columns only): unsorted `chevrons-up-down` 14px `slate::S600`; asc `chevron-up` 14px accent;
  desc `chevron-down` 14px accent.
* Body rows (`table.row.<ns>/<name>`): height **41px**, separated by a 1px line
  `rgba(30,41,59,.5)` (none above the first row). Cells px 12, vertically centered, **mono 12/16**
  drawn at the 0.601em pitch, color `slate::S100` (highlighted row `#ffffff`).
* Row background: highlighted (`index == selected_row_index`) `rgba(51,65,85,.6)`; else hover
  `rgba(29,41,57,.3)`; pointer cursor.
* Row click → set highlight + open (§2.2). Double-click → open again. A click that started on
  the pin or actions button never opens the row.
* Empty (0 visible rows, not loading; `table.empty`): centered column gap 12px: `inbox` 48px
  `slate::S600`; "No resources found" 14/20 medium `slate::S400`; "Try switching namespace or
  adjusting your search" 12/16 `slate::S500`.

### 7.4 Sorting (`AppState::toggle_sort`, `ResourceStore::visible`)
Click a header toggles its sort; there is no return to unsorted. First-click direction: **asc if
the first row's value for that column is a string, desc otherwise** (numbers / undefined), then it
alternates. Shift+click = multi-column sort (adds the column or flips it). Sort state lives in
`AppState::sorting`, is session-only and **persists across kinds** (by column id). Default = none
(backend order + appended watch rows). `pin` and `actions` are not sortable (their header cells
still have the pointer/hover color). Comparator per column, chosen from the first row's value:
string containing a digit → natural alphanumeric (case-insensitive); other string →
case-insensitive text; numbers → numeric. Undefined values always sort last (both directions).
**Age sort** (`age`, `lastSeen`): compares timestamps; asc = oldest first (largest age first);
missing/invalid timestamps last. CronJob "Last Schedule" sorts its ISO timestamp as a string.
Results are cached per (rows version, search, sorting).

### 7.5 `to_resource_item` field derivation (`state/resource_item.rs`, all kinds)
* `name` = metadata.name (item dropped if empty); `namespace` = metadata.namespace or "".
* `ready`: if `status.containerStatuses` → `"<ready>/<total>"`; else if `readyReplicas` &
  `replicas` → `"r/n"`; else if only readyReplicas → `"r"`; else none (blank cell).
* **Pods** (`kind: Pod`, or `spec.containers` present): `status` and `restarts` come from
  `resource_item::pod_status`, a port of kubectl's `printPod` STATUS column: start with
  status.phase (status.reason overrides it, e.g. "Evicted"); the first unfinished init container
  gives `Init:<reason>` / `Init:ExitCode:N` / `Init:Signal:N` / `Init:i/n`; otherwise containers
  (last to first) override with their waiting reason (`ContainerCreating`, `CrashLoopBackOff`,
  `ImagePullBackOff`…) or terminated reason (`Completed`, `Error`, `OOMKilled`…, else
  `ExitCode:N` / `Signal:N`); "Completed" with a container still running becomes "Running"
  (Ready condition) or "NotReady"; a deletionTimestamp gives **"Terminating"** ("Unknown" for
  NodeLost). `restarts` = **sum** of the containers' restartCounts (plus init containers while
  initializing), like kubectl's RESTARTS.
* Other kinds: `status` = jobStatus ?? readyStatus ?? status.phase ?? status.conditions[0].type, where
  * jobStatus = "Complete" if a condition `Complete=True`, else "Failed" if `Failed=True`;
  * readyStatus = "Ready"/"NotReady" from a condition `type: Ready` (nodes).
  `restarts` = sum of containerStatuses restartCounts if present.
* `node` = status.hostIP, `ip` = status.podIP.
* `up_to_date` = status.updatedReplicas; `available` = status.availableReplicas.
* `type_` = spec.type (services). `secret_type` = root `type` of Secrets (`kind: Secret`), e.g.
  "Opaque", "kubernetes.io/tls" (column id `secretType`).
* `cluster_ip` = spec.clusterIP. `external_ip`: LB ingress ip/hostname → spec.externalIPs[0] →
  `<pending>` if type LoadBalancer → `<none>`.
* `ports` = spec.ports joined by ",": `nodePort:port/PROTO` if nodePort else `port/PROTO`
  (protocol default TCP).
* `roles` = from labels `node-role.kubernetes.io/<role>` and `kubernetes.io/role=<v>`, joined ",",
  default `<none>`. `version` = status.nodeInfo.kubeletVersion.
* Events: `reason`, `message`, `count`, `event_type` = root `type`, `involved_object` =
  `"<Kind>/<name>"`, `last_seen` = lastTimestamp ?? creationTimestamp.
* Jobs: `completions` = `"<succeeded??0>/<spec.completions??1>"` if either is set; `duration` from
  start/completion time: `<60s` → "Ns"; `<1h` → "MmSs" (e.g. "2m5s"); else "HhMm" (unparseable
  timestamps → "NaNs").
* CronJobs: `schedule`, `last_schedule` = status.lastScheduleTime, `active` = len(status.active[])
  (jobs: the status.active number).
* `data_keys` = number of keys in root `data` (binaryData not counted).
* Ingress: `hosts` = rule hosts joined ", " (missing host → "*"); `ingress_class` =
  spec.ingressClassName; `ingress_ports` = "80", or **"80, 443"** when spec.tls is non-empty
  (kubectl's PORTS; column id `ingressPorts`).
* `kind_marker` (`_kind`, search results), `context` (`_context`, multi-cluster rows).

### 7.6 Age formatting (`format_relative_age`)
Missing/invalid → "-". Else floor of the difference: ≥1 day → "Nd"; ≥1h → "Nh"; ≥1m → "Nm";
else "Ns". Single unit only; computed at render time (refreshes whenever the table re-renders,
no ticking timer).

### 7.7 Status badge (`widgets::status_badge`, `status_badge_mono` in table cells)
Row, gap 6px, px 8 py 2, fully rounded, 12/16 medium; in the table the label is mono at the 0.601em
pitch. Dot 6×6 round; text = status or "-".
Variant by lowercase status (`status_variant`):
| Variant | Statuses | dot / text | bg | pulse |
|---|---|---|---|---|
| running | running, ready, succeeded, completed, complete | `#34d399` | `rgba(16,185,129,.1)` | yes — ping halo (§0.2) |
| pending | pending, containercreating, podinitializing, init, waiting, `Init:…` (unless its reason is a failure) | `#fbbf24` | `rgba(245,158,11,.1)` | no |
| failed | failed, crashloopbackoff, error, imagepullbackoff, errimagepull, evicted, oomkilled, createcontainerconfigerror, createcontainererror, invalidimagename, containerstatusunknown, `ExitCode:N`, `Signal:N`, `Init:<failure>` | `#f87171` | `rgba(239,68,68,.1)` | no |
| terminating | terminating | `#fb923c` | `rgba(249,115,22,.1)` | no |
| default | anything else (NotReady, Active, Available, none…) | dot `#94a3b8`, text `#cbd5e1` | `rgba(100,116,139,.1)` | no |

### 7.8 Columns per kind (`resource_table::columns`; in order; "-" = shows "-" when undefined)

Multi-cluster mode prepends **CLUSTER** (`_context`): mono, truncated at 120px, row text color,
"-" if empty. All kinds append **pin** (header blank) and **actions** (header blank) columns.

| Kind | Columns |
|---|---|
| pods | NAME · NAMESPACE · STATUS (badge) · READY (raw, blank if none) · RESTARTS · AGE |
| deployments | NAMESPACE · NAME · READY (raw) · UP-TO-DATE (-) · AVAILABLE (-) · AGE |
| services | NAMESPACE · NAME · TYPE (-) · CLUSTER-IP (-) · EXTERNAL-IP (-) · PORT(S) (-) · AGE |
| nodes | NAME · STATUS (badge) · ROLES (-) · AGE · VERSION (-) |
| events | LAST SEEN (age of last_seen) · TYPE ("Warning" `#fbbf24`, else `#60a5fa`; "-" if none) · REASON (-) · OBJECT (raw) · MESSAGE (truncate at 300px, "-") · COUNT (-) |
| configmaps | NAMESPACE · NAME · DATA KEYS (0 if none) · AGE |
| secrets | NAMESPACE · NAME · TYPE (secret type, -) · DATA KEYS (0) · AGE |
| ingresses | NAMESPACE · NAME · CLASS (-) · HOSTS (-) · PORTS ("80" / "80, 443") · AGE |
| jobs | NAMESPACE · NAME · STATUS (badge) · COMPLETIONS (-) · DURATION (-) · AGE |
| cronjobs | NAMESPACE · NAME · SCHEDULE (-) · LAST SCHEDULE (age) · ACTIVE (0) · AGE |
| namespaces | NAME · STATUS (badge; e.g. "Active" → default gray) · AGE |

Header labels are drawn uppercase via `caps()`; the source labels are "Name", "Up-to-date",
"Cluster-IP", "Port(s)", "Last Seen", "Data Keys", "Last Schedule", etc. Column ids: `name`,
`namespace`, `status`, `ready`, `restarts`, `age`, `upToDate`, `available`, `type`, `clusterIp`,
`externalIp`, `ports`, `roles`, `version`, `lastSeen`, `eventType`, `reason`, `involvedObject`,
`message`, `count`, `dataKeys`, `secretType`, `ingressClass`, `hosts`, `ingressPorts`,
`completions`, `duration`, `schedule`, `lastSchedule`, `active`, `_context`, `pin`, `actions`.
Each data cell has automation id `table.cell.<ns>/<name>.<column id>` (role `cell`, label = the
cell text; the RESTARTS cell has role `sparkline` when it renders through the sparkline).

**Restarts cell** (**Deliberate Fix 14**): if the pod has a recorded restart history (§7.10) the
cell renders `widgets::restart_sparkline`; otherwise the plain number (mono 12px, row color).

### 7.9 Pin & row actions
* **Pin cell** (`table.pin.<ns>/<name>`): button p 4, radius 4px, star 14px. Pinned: filled,
  `#fbbf24`, hover `#fcd34d`. Unpinned: outline `slate::S600`, hover `slate::S400`. Click (row not
  opened) toggles the pin for `(kind, name, namespace or "default")`.
* **Actions cell** (`table.actions.<ns>/<name>`): button p 4, radius 4px, hover bg
  `rgba(29,41,57,.4)`; `more-horizontal` 16px `slate::S500`, hover `slate::S300`. Click toggles a
  menu for that row (doesn't open the row).
  Menu: painted above everything (`deferred`, so never clipped by the table), right-aligned under
  the button (top 28px), min width 160px, radius 8px, padding 4px 0, glass. Items
  (`table.menu.<action>`): full width, px 12 py 6, left, 12/16 (mono inherited), gap 8px, icon
  14px; normal `slate::S300`, hover bg `rgba(29,41,57,.4)`; Delete `#f87171`, hover bg
  `rgba(239,68,68,.1)`. Closes on outside mouse-down or item click.
  * pods: View Logs `scroll-text` · Exec Shell `terminal` · Port Forward `network` · Delete `trash-2`
  * deployments: Scale `arrow-up-down` · Restart `refresh-cw` · Delete `trash-2`
  * others: Describe `file-search` · Delete `trash-2`

### 7.10 RestartSparkline (`widgets::restart_sparkline`) and restart history
Input: per-point restart counts (oldest first), drawn 60×16. If <2 points or all zero → just the
latest count (mono 12px) colored `slate::S500` if 0 else `#f59e0b`. Else a row, gap 6px: a
polyline (padding 1px, x evenly spaced, y normalised min..max, inverted), stroke 1.5px, no fill;
color: latest 0 → `slate::S500`; latest > previous → `#ef4444`; else `#f59e0b`; then the count in
the same color, line height 1.
History (`ResourceStore`, pods only, session-lifetime, keyed `ns/pod`): on every list and every
watch upsert a timestamp is recorded the first time a pod is seen with restarts > 0 and whenever
its count increases; up to 20 timestamps within the last hour are kept; point i count =
max(0, current − (len−1−i)). So a pod that has restarted shows at least an amber count, and a
red rising line once it restarts while watched.

---

## 8. LabelFilterBar (`views/header.rs`)

Row, wrap, gap 8px. `filter` icon 14px `slate::S500`. Then one chip per label
(`labelbar.chip.<label>`): row gap 4px, px 8 py 2, fully rounded, 12/16, text accent, 1px border
`PREFLIGHT_BORDER`, no bg; label mono; `x` 12px button (`labelbar.remove.<label>`, accent, hover
`#f87171`) removes it. Then the input (`labelbar.input`): transparent, 12/16 `slate::S300`,
placeholder "app=name" `slate::S600`, width 128px when there are no labels else 96px, height 16px,
no border (focused: 4px radius + focus ring).
* Enter → add the trimmed value if non-empty and not a duplicate; clears the input.
* Backspace on an empty input → remove the last label.
* Labels (session only) form the selector `a=b,c=d` passed to list/watch; each add/remove refreshes.
* `/` is typeable (**Deliberate Fix 1**), so selectors like `app.kubernetes.io/name=x` work.

---

## 9. PinnedResources (sidebar)

Storage: `local-storage.json` key `kore-pinned-resources` (`state/pins.rs`) = JSON array
`[{kind,name,namespace}]`, newest first, max 50, loaded at startup, written on every change.
Inside the sidebar "PINNED" section (under its single heading, §4.3), `Sidebar::render_pinned`
renders the pinned rows, aligned with the other nav items (same 16px nav padding, no extra
block padding or border):
* Items (`sidebar.pin.<ns>/<name>`, gap 2px): row gap 8px, px 12 py 4, radius 6px, pointer; hover
  bg `rgba(29,41,57,.3)`. Kind icon 14px `slate::S500` (hover `slate::S400`) from
  `KindExt::icon` — pods `boxes`, deployments `server`, services `activity`, nodes `cpu`, jobs
  `timer`, cronjobs `calendar`, ingresses `globe`, configmaps `file-text`, secrets `lock`, events
  `zap`, namespaces / unknown `boxes`. Name mono 12/16 `slate::S300` (hover `slate::S100`) flex-1
  truncate. Namespace 10px `slate::S600`, truncated at 60px. Remove button
  (`sidebar.pin.unpin.<ns>/<name>`): opacity 0 → 1 on row hover, p 2, radius 4px, hover bg
  `rgba(51,65,85,.5)`, `x` 12px `slate::S500`.
* Click row → open details (§2.2). Click X → unpin (section disappears when empty).

---

## 10. ConfirmDialog (`ui/confirm_dialog.rs`; delete from table)

Driven by `AppState::confirm` (`ConfirmRequest`). Row action Delete (and the table's `d` key, §3)
calls `request_delete`: title `"Delete " + KindExt::singular_label()` (the Kubernetes kind name):
"Delete Pod", "Delete Deployment", "Delete Service", "Delete Node", "Delete Event",
"Delete ConfigMap", "Delete Secret", "Delete Ingress", "Delete Job", "Delete CronJob",
"Delete Namespace".
Message: `Are you sure you want to delete "<name>" in namespace "<ns>"?` (ns = row namespace or
"default"). Buttons: "Cancel", "Delete". Variant Danger.

Visuals (`confirm`): backdrop full-window `rgba(0,0,0,.6)` (no blur), blocks the mouse. Centering
layer padding 16px. Panel: bg `SURFACE` `#101828` (Warning variant `#1b180d`), 1px border
`rgba(239,68,68,.5)`, radius 8px, confirm shadow (§0.2), max width 448px; enter animation 250ms
ease-out: opacity 0→1, y 20→0. No exit animation (disappears instantly).
Inside padding 24px:
* Row gap 16px, mb 16px: icon circle (p 8, round, bg `rgba(239,68,68,.2)`) with `alert-triangle`
  24px `#f87171`; text column: title 18/28 semibold `slate::S100` mb 8px; message 14/20
  `slate::S300` (`confirm.message`); close `x` 20px (`confirm.close`) `slate::S400`, hover `slate::S200`.
* Buttons row: right-aligned, gap 12px, margin-top 24px. Cancel (`confirm.cancel`): px 16 py 8,
  radius 4px, 1px `slate::S800`, hover border `slate::S700` + bg `rgba(29,41,57,.4)`, 14/20
  `slate::S100`. Delete (`confirm.ok`): px 16 py 8, radius 4px, bg `#dc2626` hover `#b91c1c`, 1px
  `#ef4444`, 14/20 white.
* Close: Cancel, X, **Esc** (root, consumed — never navigates back) and a **mouse-down on the
  backdrop** (the panel occludes it, so presses inside the panel don't close). Once the confirm
  button was pressed (`AppState::confirm_busy`, until `close_confirm`), Esc and backdrop clicks are
  ignored so the running action isn't interrupted; Cancel/X still close.
Confirm → `Core::delete_resource(kind, ns, name)` → success toast `"<name> deleted"` or error toast
(`backend::format_error`), then the dialog closes. The deleted row disappears via the watch event.
Other variants: Warning — border `rgba(234,179,8,.5)`, icon bg `rgba(234,179,8,.2)`, icon `#facc15`,
button `#ca8a04` hover `#a16207` border `#eab308`; Info — border `rgba(59,130,246,.5)`, icon bg
`rgba(59,130,246,.2)`, icon `#60a5fa`, button `#2563eb` hover `#1d4ed8` border `#3b82f6`.

---

## 11. Toasts (`ui/toast.rs`)

API: `ui::toast::toast(cx, message, ToastKind::{Info, Success, Error})` (app-global
`ToastStore`). Stack absolute bottom-right (16px insets), column gap 8px, right-aligned, newest at
the bottom, above the app content. Each toast (`toast.<n>`): row gap 10px, px 16 py 10, radius 8px,
glass, 14/20 `slate::S200`, max width 320px (message shrinks to fit, wrapping at 236px), pointer;
icon 16px: success `check-circle-2` `#34d399`, error `x-circle` `#f87171`, info `info` accent;
message; `x` 14px `slate::S500`. Auto-dismiss **3000ms**; a click anywhere on the toast dismisses
it. Enter animation 200ms ease-out: opacity 0→1, y 16→0. No exit animation.
Toasts emitted in this area: `"<name> deleted"` (success), formatted delete error (error),
Settings/AI: `"Connection successful"` (success), `"Connection failed: <msg>"` (error).
`backend::format_error(e)` = the `kore_core` error's display text.

---

## 12. ConnectionSetup (`views/connection_setup.rs`, not connected)

Full window bg `BACKGROUND`, padding 32px, scrollable, centered column max width 512px; enter
animation 400ms ease-out: opacity 0→1, y 20→0. Automation id `connection` (label = error title).
* **Logo** (centered, mb 32px): row gap 12px (mb 8px): tile 40×40 radius 12px, no bg, `wifi` 20px
  accent; "Kore" 24/32 bold `slate::S100`, tracking −0.025em. Below: "Kubernetes IDE" 14/20
  `slate::S500`.
* **Error card** (glass, radius 12px, padding 24px, mb 24px):
  * Row gap 16px mb 20px, top-aligned: icon 32px `#fbbf24` (by error_kind, 2px down), title 18/28
    semibold `slate::S100` mb 4px, description 14px line height 22px `slate::S400`.
  * If `status.error`: a toggle (`connection.details`) "▶ Show technical details" (▼ when open; the
    triangle is 9px) 12/16 `slate::S600`, hover `slate::S400`; expanded: mt 8px, a wrapping mono
    block (`connection.error`) 12px `slate::S500`, bg `rgba(11,18,33,.6)`, radius 8px, padding
    12px. Block mb 20px.
  * "HOW TO FIX" 12/16 medium `slate::S500` caps; each step mt 10px, row gap 10px: circle 20px bg
    `slate::S800` with the number 10px bold `slate::S400`; text 14px line height 22px `slate::S300`.
  * error_kind → icon / title / description / steps:
    * `no_kubeconfig`: `file-question`, "No Kubeconfig Found", "Kore couldn't find a kubeconfig file[ at <path>]." Steps: "Ensure you have a Kubernetes cluster configured" / "Check that ~/.kube/config exists and is readable" / "Or set the KUBECONFIG environment variable to your config path" / "If using a cloud provider, run the appropriate auth command (e.g. aws eks update-kubeconfig, gcloud container clusters get-credentials, az aks get-credentials)".
    * `auth_failed`: `shield-x`, "Authentication Failed", "Kore connected to the cluster but your credentials were rejected." Steps: "Your token or certificate may have expired" / "Run kubectl get pods to verify your credentials work" / "If using a cloud provider, re-authenticate (e.g. aws sso login, gcloud auth login)" / "Check that your kubeconfig user entry has valid credentials".
    * `cluster_unreachable`: `wifi-off`, "Cluster Unreachable", "Kore found your kubeconfig but couldn't connect to the cluster." Steps: "Check that your cluster is running and accessible" / "If using a VPN, ensure it's connected" / "Verify the cluster endpoint in your kubeconfig is correct" / "Try running kubectl cluster-info to test connectivity".
    * other (`invalid_context`, `unknown`, none): `alert-triangle`, "Connection Error", description = status.error or "An unexpected error occurred while connecting." Steps: "Check your kubeconfig file for syntax errors" / "Try running kubectl get pods to test your configuration" / "Restart Kore after fixing the issue".
* **Context picker** (if `contexts_available` non-empty; glass, radius 12px, padding 20px, mb 24px):
  heading 14/20 medium `slate::S300` with `terminal` 16px `slate::S500` (gap 8px), "Try a different
  context" (mb 12px). List max height 192px, scrollable (6px gutter), gap 6px. Row
  (`connection.context.<ctx>`): full width, gap 10px, px 12 py 8, radius 8px, 14/20, 1px border;
  dot 6px; name mono truncated.
  * selected: text accent, border `PREFLIGHT_BORDER`, no bg, dot accent, trailing `chevron-right`
    14px accent.
  * current (== `current_context`): bg `rgba(30,41,59,.6)`, border `slate::S700`, text
    `slate::S400`, dot `#fbbf24`, trailing "current" 10px `slate::S600`.
  * other: bg `rgba(16,24,40,.4)`, border transparent (hover `slate::S700`), text `slate::S300`
    (hover `slate::S100`), dot `slate::S600`.
  * Click toggles the selection; ignored while retrying.
* **Retry button** (`connection.retry`): full width, row centered gap 8px, px 16 py 10, radius 8px,
  text accent 14/20 medium, 1px border `PREFLIGHT_BORDER`, no bg; `refresh-cw` 16px. Label "Retry
  Connection", or `Connect to <ctx>` when a context is selected; while retrying: spinning icon +
  "Connecting...", opacity .5, clicks ignored.
  Action: `AppState::retry_connection(ctx)` → **`Core::retry_connection(Option<String>)`** → new
  status. If connected: contexts = `contexts_available`, context = `current_context`, then
  namespaces and the table load (§1 step 5, §7.1) and the main app renders. Otherwise the error
  screen re-renders with the new status. A failed call is only logged.
* If `kubeconfig_path`: footer "Checking: <path>" 11px mono `slate::S600`, centered, mt 16px.

---

## 13. Banners (`views/banners.rs`)

### 13.1 Update banners (`update_banner`; top of window, full width)
Appear and disappear instantly (no height animation). Row, items centered, gap 12px, padding 8px
16px, 12/16 text, 1px bottom border. At most one is shown, in this priority:
* **Success** (`banner.update-success`; `Core::perform_update` returned a version): bg
  `rgba(16,185,129,.15)`, border `rgba(16,185,129,.3)`; `check-circle` 16px `#34d399`; text
  `#6ee7b7` "Updated to <version> — restart Kore to apply."
* **Error** (`banner.update-error`; update failed): bg `rgba(239,68,68,.1)`, border
  `rgba(239,68,68,.3)`; `alert-triangle` 16px `#f87171`; "Update failed: <error>" `#fca5a5`; if
  `release_url` is non-empty: "Download manually" (`external-link` 12px, 11px `slate::S400`, hover
  `slate::S200`) opens the URL externally; `x` 14px pushed right (`rgba(248,113,113,.6)`, hover
  `#fca5a5`) dismisses (clears the error and hides the banner for the session).
* **Available** (`banner.update`; `has_update`, not dismissed): no bg, border `PREFLIGHT_BORDER`,
  content centered: text accent "Kore <latest> is available"; button (`banner.update-install`;
  px 10 py 4, radius 6px, 11px medium, text accent, 1px `PREFLIGHT_BORDER` border):
  - macOS (`kore_core::SELF_UPDATE_SUPPORTED`): "Update" with `download` 12px →
    `AppState::perform_update()` → `Core::perform_update`; while running: spinning `loader-2` 12px +
    "Installing...", opacity .5, inert.
  - Linux/Windows: "Download" with `external-link` 12px → opens `release_url` in the browser
    (there is no in-app install).

  `x` 14px `slate::S100`
  (`banner.update-dismiss`, "Dismiss until next launch") hides it for the session.

### 13.2 RBAC impersonation banner (`impersonation_banner`; top of main column)
Shown while `AppState::impersonation` is set (set by the RBAC view). No animation. Row gap 12px,
padding 8px 16px, bg `rgba(245,158,11,.1)`, bottom border `rgba(245,158,11,.2)`. `shield-alert`
16px `#fbbf24`; "Impersonation Mode:" 12/16 `#fcd34d`; `viewing as <label>` 12/16 mono `#fde68a`
(label = `ns/name` for ServiceAccounts, else name); "(simulated — actual API calls use your real
credentials)" 10px `rgba(245,158,11,.7)`, ml 4px; spacer; "Exit" button
(`banner.impersonation.exit`; row gap 4px, px 8 py 2, radius 4px, 12/16 `#fbbf24`, hover bg
`rgba(245,158,11,.1)`, `x` 12px) → `set_impersonation(None)`.

### 13.3 RbacForbiddenBanner
Not rendered by the native app: no view calls `Core::rbac_analyze_forbidden`. Design, if it is
added (inside the RBAC view content): radius 8px, 1px `rgba(239,68,68,.2)`, bg
`rgba(239,68,68,.05)`, padding 16px, children gap 12px. Row gap 12px: `shield-x` 20px `#f87171`;
"Permission Denied" 14px medium `#fca5a5`; below (mt 4px, 12px mono `slate::S400`):
`<identity> cannot <verb> <resource>[ in namespace "<ns>"]`; right: "Dismiss" 12px `slate::S500`
hover `slate::S300`. If closest rules: "CLOSEST MATCHING RULES" caps 10px medium `slate::S500` mb
6px + the RBAC rule chain (`views/rbac/rule_chain.rs`). Suggestion box: row gap 8px, px 12 py 8,
radius 4px, bg `rgba(245,158,11,.05)`, 1px `rgba(245,158,11,.15)`; `lightbulb` 14px `#fbbf24`;
text 12px `rgba(253,230,138,.8)`.

### 13.4 AIConfigWarning (`banners::ai_config_warning()`, `banner.ai-config`)
px 16 py 10, bottom border `rgba(30,41,59,.5)`, bg `rgba(245,158,11,.05)`; row gap 8px, 12/16:
`alert-circle` 14px `#fbbf24`; "Configure an AI provider in Settings to use this feature"
`rgba(252,211,77,.9)`. Used by the AI panel and the Network Policies AI section; AI Chat renders
the same strip itself (`views/ai_chat.rs`).
Shown when the provider isn't configured (`state/ai_config::is_provider_configured`): unknown
provider, or a provider that needs an API key (openai, anthropic) with no key. The key is loaded
from the OS keychain (`Core::get_api_key`) into the shared `AiConfigModel` (**Deliberate Fix
11**); AI views observe that app-global entity to pick up config changes.

### 13.5 Update check logic (`AppState::check_updates`)
At boot: read `local-storage.json` key `kore-update-check` = `{timestamp (ms), info}`; if younger
than 24h and `info` is present → use it; else **`Core::check_for_updates()`** (→ `UpdateInfo
{has_update, current_version, latest_version, release_url, release_notes}`) and store the result
with the current timestamp. Failures are silent. `perform_update` → version string; on success the
cache key is removed. Dismiss is in-memory only. Settings "Check for Updates" calls
`check_updates_now` (always hits the backend) and re-shows a dismissed banner when a result
arrives.

---

## 14. Command Palette (`views/command_palette.rs`)

Open/close: Mod+K toggles; Esc closes (root); clicking the backdrop closes; activating any item
closes. The entity is created on open and dropped on close, so the query resets.
* Backdrop (`palette`): painted above everything (`deferred`), full window, black at **0.9**
  (`BACKDROP_ALPHA`; stands in for a blurred 0.6 dim), blocks the mouse; content column aligned
  top-center with **padding-top 20% of the window height**. Appears instantly (no fade).
* Panel: width 100% up to 576px, radius 12px, overflow hidden, glass with the opaque
  `GLASS_OVER_DIM` fill. No enter/exit motion.
* Input (`palette.input`, focused on open): box height 45px, bottom border `rgba(30,41,59,.5)`;
  input padding 16px left/right, 12px top/bottom, 14/20 `slate::S100`, placeholder **"Search
  resources, navigate, switch context..."** (`PREFLIGHT_PLACEHOLDER`); while focused the
  `ai_chat::focus_ring` overlay is drawn.
* List: max height 400px, scrollable (6px gutter when the content exceeds 400px), horizontal
  padding 4px plus 4px top/bottom spacers.
* **Group headings**: section labels like the sidebar's — `caps(heading, 10)`, 10px medium
  `slate::S500`, line height 12px, padding 8px 12px 4px (24px tall): "SEARCH RESULTS", "VIEWS",
  "RESOURCES", "QUICK FILTERS", "ACTIONS", "CONTEXTS", "RECENT".
* Item (`palette.item.<value>`, `.selected(true)` for the keyboard selection): padding 8px 12px,
  radius 6px, pointer, 14/20 `slate::S200`, row gap 12px, icon 16px `slate::S500`; hover bg
  `rgba(29,41,57,.6)`. **The keyboard selection gets the same bg** `rgba(29,41,57,.6)`. ↑/↓ move
  the selection (clamped, scrolled into view), Enter activates; the first item is selected by
  default; mouse movement over an item selects it. Automation: `palette.selected` (label = the
  selected item's value).
* Empty: "No results found" (or "Searching..." while a backend search is in flight), padding 24px
  16px, 14/20 `slate::S500`, centered.
* Filtering (`ui/command_score.rs`, a port of the command-score fuzzy ranker): each item's `value` is
  scored against the trimmed query; items with score > 0 are shown, sorted by score within their
  group, and groups are ordered by their best score. With an empty query everything shows in
  group order. Values: `search-<kind>-<name>`, `view-<Label>`, `nav-<Label>`, `filter-:<x>`,
  `action-ask-ai`, `action-sponsor`, `ctx-<context>`, `recent-<kind>-<name>`.

Groups (in order):
1. **Search Results** (when ≥1): **`Core::search_resources(query, namespace)`** after a 300ms
   debounce (restarted per keystroke), only if the query has ≥ 2 characters (namespace `None` for
   `*`); errors give no results. The backend searches name substrings across pods, deployments,
   services, configmaps, secrets, jobs, cronjobs, ingresses (current namespace) and namespaces
   (cluster), max 200, tagging `_kind`. Item: `search` icon; name (mono 12/16, truncate) + kind pill
   (10px, bg `slate::S800`, `slate::S400`, px 6 py 2, round) e.g. "pods"; namespace line 10px
   `slate::S500` if any; trailing `arrow-right` 12px `slate::S600`. Select → add to Recent, then
   `open_resource(Some(kind), item)`.
2. **Views**: AI Chat `message-circle` · Dashboard `layout-dashboard` · Resource Graph `git-branch`
   · Network Policies `shield` · Custom Resources `puzzle` · Helm Releases `package` · RBAC
   Simulator `shield-check` · Settings `settings` → `set_view`.
3. **Resources**: "Go to Pods/Deployments/Services/Nodes/Jobs/CronJobs/ConfigMaps/Secrets/
   Ingresses/Events/Namespaces" with icons boxes, server, activity, cpu, timer, calendar,
   file-text, lock, globe, zap, folder-open → `go_to_kind`.
4. **Quick Filters** (only when the query starts with ":"; filtered by prefix): `:pods` "Filter to
   pods", `:deployments` "Filter to deployments", `:services` "Filter to services", `:running`
   "Show running resources", `:failed` "Show failed/error resources", `:pending` "Show pending
   resources". `filter` icon, label mono 12/16, description 10px `slate::S500` right-aligned. The
   three kind filters → `go_to_kind`. The three status filters → `set_status_filter(Running |
   Failed | Pending)`: view=table (selection cleared), and the table shows only rows whose status
   badge has that variant (§7.7: `:running` = Running/Completed/Succeeded/Ready…, `:failed` =
   Failed/Error/CrashLoopBackOff…, `:pending` = Pending/ContainerCreating/Init…; rows without a
   status are hidden). The header shows a chip (§6); its × clears the filter, as do a sidebar kind
   click (`set_kind`) and a context switch.
5. **Actions**: "Ask AI" (`sparkles`, right "Troubleshoot with AI") → `open_ai_panel` (with the
   selected resource in details view); "Sponsor Kore" (`heart` `#f472b6`, right "Support on GitHub
   Sponsors") → opens the sponsors URL in the system browser.
6. **Contexts**: one per context: 6px `#34d399` dot + name mono 12/16 truncate → `switch_context`.
7. **Recent** (if any): up to 8 from `local-storage.json` key `kore-recent-resources`
   (`[{kind,name,namespace,timestamp}]`, max 20, de-duplicated, newest first); `clock` icon + name
   mono 12/16 + kind pill → `open_resource(Some(kind), …)`. The list is re-read from storage on
   every palette open, so a new entry shows the next time the palette opens. Only Search Result
   selections add recents (namespace "" stored as "default").

---

## 15. Shortcut Overlay (`views/shortcut_overlay.rs`, `?`)

Backdrop (`shortcuts`): painted above everything, full window, black at 0.9, blocks the mouse,
content top-center with padding-top 15% of the window height; click closes. Appears instantly.
Panel: width 100% up to 512px, radius 12px, overflow hidden, glass with `GLASS_OVER_DIM` fill.
* Header: padding 14px 20px, bottom border `rgba(30,41,59,.5)`. Left (gap 10px): tile 28×28 radius
  8px (no bg) with `keyboard` 16px accent; "Keyboard Shortcuts" 14/20 semibold `slate::S100`;
  "Quick reference for all shortcuts" 10px `slate::S500`. Right: close button (`shortcuts.close`)
  p 6, radius 6px, `x` 16px `slate::S400`, hover `slate::S200` + bg `rgba(30,41,59,.5)`.
* Body: bg `rgba(30,41,59,.3)` (an even tint), two rows of two equal cells with a 1px gap between
  the rows. Each cell padding 16px: category caps 10px medium **`slate::S100`**, mb 8px; rows gap
  2px: row space-between, padding 6px 8px, radius 4px, hover bg `rgba(255,255,255,.02)`;
  description 12/16 `slate::S400`; keys (ml 16px, gap 2px), each a key cap: min width 24px, height
  24px, px 6, bg `rgba(30,41,59,.8)`, radius 4px, 11px mono `slate::S300`, 1px
  `rgba(51,65,85,.5)`, shadow `0 1px 0 0 rgba(51,65,85,.5)`. Mod / Shift caps read "⌘" / "⇧" on
  macOS, "Ctrl" / "Shift" elsewhere.
  * **Navigation**: Move down [j] · Move up [k] · Enter detail view [l] · Go back [h] · Switch tabs [1][2][3][4][5]
  * **Actions**: Delete resource [d] · Command palette [⌘][K] · Refresh resources [⌘][R] · Search / filter [/] · Search logs [⌘][F]
  * **RBAC**: Open RBAC view [⌘][⇧][R] · Toggle impersonation [i] · Switch RBAC tabs [1][2][3][4]
  * **General**: Close / go back [Esc] · Select / confirm [Enter] · Toggle this overlay [?]
* Footer: padding 10px 20px, top border `rgba(30,41,59,.5)`, centered row gap 6px, 10px
  `slate::S600`: "Press [?] or [Esc] to close" (key caps with 9px text).
* Close: `?`, Esc, backdrop click, X.

---

## 16. TextSearchBar (`ui/text_search_bar.rs`; shared by describe, YAML and log views)

Place inside a relative container; it positions itself absolute top 8px, right 24px; row gap 4px;
bg `SURFACE`, 1px `slate::S700`, radius 8px, padding 6px 12px, TextSearchBar shadow (§0.2).
`search` 14px `slate::S500`; input 160×16, transparent, 12/16 `slate::S200`, placeholder
"Search..." `slate::S600` ("Search logs..." in logs mode), auto-focused after 50ms; if query &&
0 matches: "No results" 10px `rgba(248,113,113,.7)` mx 4 (not in logs mode); if matches:
`<current+1>/<count>` 10px `slate::S500` mx 4; buttons `chevron-up`, `chevron-down`, `x` (ml 4)
14px `slate::S400`, hover `slate::S200`. Emits `SearchBarEvent::{Query, Next, Prev, Close}`; the
owner computes matches and reports `set_matches(current, count)`. Enter → Next, Shift+Enter → Prev,
Esc while the input is focused → Close (and nothing else: no back-navigation).

---

## 17. Settings page (`views/settings.rs`)

Column, full size, bg `BACKGROUND` (`view.settings`). No enter/exit motion.
* **Header**: bottom border `slate::S800`, padding 16px, bg `rgba(16,24,40,.5)`; row gap 16px:
  Back button (`settings.back`; row gap 8px, px 12 py 6, radius 6px, 1px `slate::S800`, hover bg
  `rgba(29,41,57,.3)`, 14/20 `slate::S300`; `arrow-left` 16px + "Back" + Kbd "Esc") → table; title
  group gap 8px: `settings` icon 20px accent + "Settings" 18/28 semibold `slate::S100`.
  Settings Kbd: min width 22px, px 6 py 2, bg `rgba(30,41,59,.8)`, radius 4px, 10px mono
  `slate::S400`, 1px `rgba(51,65,85,.5)`.
* **Content**: vertical scroll (6px gutter), padding 24px, cards gap 24px. Card = glass, radius
  12px, padding 20px (no entry animation). Card title row: icon 16px accent + title 14/20 semibold
  `slate::S100`, gap 8px, mb 16px. Field label: 12/16 `slate::S400` mb 6px (optional icon 12px,
  gap 6px). Helper text: 10px `slate::S500` mt 4px. Select (`ui::native_controls::NativeSelect`,
  no arrows): full width within max 320px, px 12 py 8, bg `BACKGROUND`, 1px `slate::S800`, radius
  6px, 14/20 `slate::S100`. Clicking opens a macOS-style popup menu painted above everything (bg
  `rgba(42,42,44,.98)`, 1px `rgba(255,255,255,.15)`, radius 6px, shadow `0 8px 24px rgba(0,0,0,.4)`,
  22px rows, positioned so the selected option overlays the button); while open: type-ahead,
  ↑/↓, Enter/Space choose, Esc closes, outside click closes; registered as modal `select:<id>`.
  1. **General** (`clock`): "Default View" (`layout` icon) select (`settings.default-view`): AI Chat
     (`chat`, default) / Resource Table (`table`) / Dashboard (`dashboard`); helper "Which view to
     show when the app starts." — "Event Retention Period" select (`settings.retention`): 1 day /
     3 days / 7 days (default) / 14 days / 30 days (`1d…30d`); helper "How long to keep event
     history in the timeline view." The value is passed to `Core::set_event_retention(days)` at
     startup and whenever it changes; Core prunes immediately and then hourly
     (`harness/scenarios/10-shell-fix-event-retention.yaml`).
  2. **AI Provider** (§17.1).
  3. **Display** (`monitor`), fields gap 20px:
     * "Default Namespace per Context" (`globe`): existing entries (gap 6px, mb 12px, sorted by
       context): row gap 8px, px 12 py 6, bg `rgba(11,18,33,.5)`, 1px `rgba(30,41,59,.5)`, radius
       4px, 12/16: context mono `slate::S300` flex-1 truncate, "→" `slate::S600`, namespace mono
       accent, "×" `slate::S500` hover `#f87171` (removes; `settings.default-ns.remove.<ctx>`).
       Inputs row gap 8px: "Context name", "Namespace" (each flex-1, box height 38px, bg
       `BACKGROUND`, 1px `slate::S800`, radius 6px (4px + focus ring when focused), input padding
       12px/8px, 14/20, placeholder `slate::S600`); "Add" button (`settings.default-ns.add`; px 16
       py 8, radius 6px, 12px with 20px line height, accent, 1px `PREFLIGHT_BORDER`; opacity .4 and
       not-allowed while either trimmed input is blank). Enter in the Namespace input also adds.
       Helper "Automatically select a namespace when switching to a context." Applied whenever
       a context's namespaces load (startup, context switch, reconnect): the configured namespace
       is selected if it exists in that context (§1 step 5).
     * "Theme Accent Color" (`palette`, label mb 8px): row gap 12px of 5 swatch buttons
       (`settings.accent.<Name>`; column gap 6px, padding 8px, radius 8px, 1px border): circle 32px
       of the color; label 10px `slate::S500` (Cyan/Violet/Emerald/Orange/Red). Selected: border
       `slate::S600`, bg `rgba(30,41,59,.5)`, circle scaled 1.1 with a 2px `BACKGROUND` gap and a
       2px `slate::S400` ring. Unselected: transparent border, hover border `slate::S700` + bg
       `rgba(30,41,59,.3)`. Click applies immediately app-wide.
  4. **Keyboard Shortcuts** (`keyboard`): groups gap 16px; group caps 10px medium `slate::S500` mb
     8px; rows (gap 2px) px 12 py 8, radius 4px, hover bg `rgba(255,255,255,.02)`: description
     12/16 `slate::S300`, keys right (Settings Kbd; " / "-separated keys get a "/" separator 10px
     `slate::S600`). Mod labels per platform (`keys::MOD_K` etc.).
     * Navigation: Navigate up / down [j] / [k] · Enter detail view [l] · Go back [h] · Switch tabs [1 - 5]
     * Actions: Command palette [⌘K] · Refresh resources [⌘R] · Search / filter [/] · Search logs [⌘F] · Delete resource [D]
     * General: Close / go back [Esc] · Select / confirm [Enter] · Show shortcut overlay [?]
  5. **About** (`info`), gap 16px: row "Current Version" (12/16 `slate::S400`) … version mono 12/16
     `slate::S200` (`UpdateInfo.current_version`, or "unknown" until an update check result is
     available). Button "Check for Updates" (`settings.check-updates`; row gap 8px, px 12 py 8, bg
     `BACKGROUND`, 1px `slate::S800`, radius 6px, 12/16 `slate::S300`, hover `slate::S100`;
     `refresh-cw` 14px, spinning + "Checking..." while checking, opacity .5, clicks ignored). Calls
     `check_updates_now` (its callback gets `Result<&UpdateInfo, String>`); update available →
     banner appears at the top (un-dismissed); no update → **"up to date" modal**; a **failed
     check → the same modal in its error state** (below).
* **Up-to-date modal** (`settings.up-to-date`; registered modal `settings-up-to-date`): painted
  above everything; full-window backdrop black at 0.85 (click closes); card max width 384px, mx
  16px, glass, radius 12px, padding 24px, centered text; no animation: `check-circle` 40px
  `#34d399` (mb 12px); "You're up to date!" 14/20 semibold `slate::S100` mb 4px; "Kore <version> is
  the latest version." 12/16 `slate::S400` mb 16px; "OK" button (`settings.up-to-date.ok`; px 16
  py 8, radius 6px, 12/16 accent, 1px `PREFLIGHT_BORDER`). Esc closes only the modal.
  **Error state** (the check failed; automation id `settings.update-error`, label
  `Couldn't check for updates: <error>`): `alert-circle` 40px `#f87171`, title "Couldn't check for
  updates", the error text (e.g. "Failed to reach GitHub: …") in place of the version line; same
  OK / backdrop / Esc behavior.
* **Esc** (`AppEvent::Escape`) → back to table, but only when no text entry has focus and nothing
  modal is open (**Deliberate Fix 4**): Esc while typing in a Settings input, with a select open,
  or with the palette / shortcut overlay open does not leave Settings.
* Persistence: `local-storage.json` key `kore-settings` = `{eventRetention:"7d",
  defaultNamespaces:{}, accentColor:"#58d0ff", defaultView:"chat"}` (missing fields take these
  defaults); saved on every change via `AppState::update_settings`, which also applies the accent
  and emits `AppEvent::SettingsChanged`.

### 17.1 AI Provider card (`views/ai_settings.rs`, `state/ai_config.rs`)
Column gap 20px. Header `settings-2` 16px accent + "AI Provider" 14/20 semibold `slate::S100`.
* **Segmented control**: row, 1px `slate::S800` border, radius 8px, overflow hidden; 5 equal
  buttons (`settings.ai.provider.<id>`; px 12 py 8, 12/16 medium, centered), 1px `slate::S800`
  separators on the right of all but the last: **OpenAI · Anthropic · Ollama · Claude Code ·
  Cursor Agent** (ids `openai`, `anthropic`, `ollama`, `claude_cli`, `cursor_agent`). Selected:
  text accent, no bg. Unselected: bg `rgba(16,24,40,.6)`, `slate::S400`, hover `slate::S200` + bg
  `rgba(29,41,57,.4)`. Unavailable (from **`Core::check_providers_availability(base_url)`** on
  mount; the Ollama probe uses the configured base URL — **Deliberate Fix 13**): not clickable,
  `slate::S600`, opacity .4, not-allowed cursor, extra line "not found" 9px `slate::S600`.
  Switching provider sets the model to the provider default (openai `gpt-4o`, anthropic
  `claude-sonnet-4-20250514`, ollama `llama3.1`, claude_cli `sonnet`, cursor_agent `auto`), the
  base URL to its default (ollama `http://localhost:11434`, else none), keeps the key only when
  the provider is unchanged (a key-needing provider then loads its key from the keychain), and
  resets the test result.
* **API Key** (openai/anthropic): label "API Key"; masked text box (bullets) height 38px, bg
  `rgba(16,24,40,.6)`, 1px `slate::S800`, radius 8px, input padding 12px/8px, 14/20, placeholder
  "Enter your <Label> API key" `slate::S600`. On change → `Core::store_api_key(provider, key)` (or
  `Core::delete_api_key(provider)` when cleared) and the key is kept in the shared config.
* **Model**: for ollama / claude_cli / cursor_agent, models are fetched via
  `Core::list_ollama_models(base_url)` / `list_claude_models()` / `list_cursor_agent_models()`
  (refetched when the provider or the Ollama base URL changes) → a mono `NativeSelect` (bg
  `rgba(16,24,40,.6)`, 1px `slate::S800`, radius 8px, no arrows), auto-selecting the first model
  if the current one is not in the list; while loading: a box (same bg/border/radius, px 12 py 8,
  14/20 `slate::S500`) with spinning `loader-2` 14px + "Loading models..."; otherwise (other
  providers, or an empty list) a mono text box with placeholder = the default model.
* **Base URL** (ollama): mono text box, placeholder `http://localhost:11434`.
* **Test Connection** button (`settings.ai.test`): full width, px 16 py 8, radius 8px, 1px border,
  14/20 medium, centered. Default: text accent, border `PREFLIGHT_BORDER`, no bg. Testing: spinning
  `loader-2` 16px + "Testing...". Success: bg `rgba(16,185,129,.1)`, border `rgba(16,185,129,.5)`,
  `#34d399`, `check-circle-2` + "Connected". Error: bg `rgba(239,68,68,.1)`, border
  `rgba(239,68,68,.5)`, `#f87171`, `alert-circle` + "Failed - Retry". Disabled (opacity .4,
  not-allowed) while testing or when a key is required but missing. Calls
  **`Core::ai_test_connection_checked(config)`**, which reports HTTP errors (**Deliberate Fix 12**)
  → toast "Connection successful" / "Connection failed: <error>". Any field edit resets the result.
* Persistence: `local-storage.json` key `kore-ai-config` = `{provider, model, base_url?}` (never
  the key); the app-global `AiConfigModel` entity notifies every AI view on change.

---

## 18. Persistence summary

`local-storage.json` lives at `<data dir>/kore/local-storage.json` (`state/storage.rs`; macOS
`~/Library/Application Support/kore/`, Linux `~/.local/share/kore/`, Windows `%APPDATA%\kore\`;
fallback `~/.kore/kore/`), one pretty-printed JSON object of keys, written atomically
(write-then-rename) on every change.

| Store | Key | Shape | Written by |
|---|---|---|---|
| `local-storage.json` | `kore-settings` | `{eventRetention, defaultNamespaces, accentColor, defaultView}` | Settings |
| `local-storage.json` | `kore-pinned-resources` | `[{kind,name,namespace}]` ≤50 newest-first | pin toggles |
| `local-storage.json` | `kore-recent-resources` | `[{kind,name,namespace,timestamp}]` ≤20 | palette search-result selection |
| `local-storage.json` | `kore-update-check` | `{timestamp, info}` (24h cache) | update check |
| `local-storage.json` | `kore-ai-config` | `{provider, model, base_url?}` | AI settings |
| `~/.kore/favorites.json` (kore-core) | `kore-favorite-contexts`, `kore-favorite-namespaces` | `{key: string[]}` pretty JSON | `Core::save_favorites` / `load_favorites` |
| OS keychain (kore-core, `keyring`) | per provider | API key | `Core::store_api_key` / `get_api_key` / `delete_api_key` |

Session-only (not persisted): kind (starts `pods`), namespace (re-derived), search, sorting, label
filters, status quick filter, multi-cluster toggle, update dismissal, highlighted row index, restart history.
Change propagation inside the app: `AppEvent::SettingsChanged` on `AppState`, and the
`AiConfigModel` global entity for AI config.

---

## 19. Backend calls (`kore_core::Core`)

Called through `backend::core(cx)` / `backend::run` (CONVENTIONS "Core access"). Methods used
directly by the shell/navigation area are marked ★; the rest are listed for orientation and
specified in specs 02–04.

| Area | `Core` method (args) |
|---|---|
| ★ connection | `get_connection_status()`, `retry_connection(context: Option<String>)`, `list_contexts()`, `list_namespaces()`, `switch_context(name)` |
| ★ resources | `list_resources(kind, namespace: Option, label_selector: Option)`, `list_resources_multi_cluster(kind, namespace, label_selector)`, `start_watch(kind, namespace, label_selector)` → `Subscription<WatchEventPayload>`, `delete_resource(kind, namespace, name)`, `search_resources(query, namespace: Option)` |
| ★ prefs / updates | `load_favorites(key)`, `save_favorites(key, values)`, `check_for_updates()`, `perform_update()` |
| ★ AI settings | `check_providers_availability(ollama_base_url)`, `get_api_key(provider)`, `store_api_key(provider, key)`, `delete_api_key(provider)`, `list_ollama_models(base_url)`, `list_claude_models()`, `list_cursor_agent_models()`, `ai_test_connection_checked(config)` |
| pods / details | `describe_pod(namespace, pod_name)`, `describe_resource(kind, namespace, name)`, `list_events_for_resource(kind, namespace, name)`, `fetch_logs(LogRequest)`, `start_pod_logs_stream(namespace, pod_name, container, previous)` → `Subscription<PodLogPayload>`, `stop_pod_logs_stream()`, `get_pod_metrics(namespace, pod_name)`, `start_port_forward` / `stop_port_forward(namespace, pod_name, local_port, pod_port)` |
| exec | `exec_into_pod(namespace, pod_name, container, shell)` → `ExecSession`, `send_exec_input(session_id, data)`, `send_exec_bytes`, `resize_exec(session_id, cols, rows)`, `stop_exec(session_id)` |
| YAML / deployments | `get_resource_yaml`, `apply_resource_yaml(…, yaml)`, `diff_resource_yaml(…, new_yaml)`, `scale_deployment(namespace, name, replicas)`, `restart_deployment(namespace, name)`, `list_deployment_revisions(namespace, name)`, `get_revision_yaml(namespace, rs_name)`, `rollback_deployment(namespace, deployment_name, rs_name)` |
| cluster views | `get_cluster_health()`, `get_cluster_health_multi_cluster()`, `query_stored_events(since, until, namespace)`, `set_event_retention(days)`, `stream_multi_pod_logs(namespace, label_selector, tail_lines)`, `list_crds()`, `list_crd_instances(group, version, plural, namespace)`, `get_crd_instance` / `delete_crd_instance(…, namespace, name)`, `build_resource_graph(namespace)` |
| helm | `helm_available()`, `list_helm_releases(namespace)`, `get_helm_values` / `get_helm_manifest` / `get_helm_history(release, namespace)`, `rollback_helm_release(release, namespace, revision)` |
| AI | `ai_diagnose(config, request)` / `ai_chat(config, request)` → `Subscription<AIStreamEvent>`, `ai_test_connection(config)`, `claude_cli_available()`, `cursor_agent_available()` |
| debug / network | `add_debug_container(namespace, pod_name, image, target_container, command)`, `list_debug_containers(namespace, pod_name)`, `stop_debug_container(…, container_name)`, `build_network_policy_graph(namespace)`, `simulate_network_traffic(source_namespace, source_pod, dest_namespace, dest_pod, port, protocol)` |
| RBAC | `rbac_check_permission(identity, verb, resource, namespace)`, `rbac_build_matrix(identity, namespace)`, `rbac_reverse_lookup(role_kind, role_name, role_namespace)`, `rbac_analyze_forbidden(error_message)`, `rbac_list_identities()`, `rbac_list_roles(namespace)`, `rbac_who_can(verb, resource, namespace)`, `rbac_natural_language_query(query, namespace)` |

★ Watch stream: `WatchEventPayload {action, kind, object}`; the backend watcher emits `applied`
(also once per object after a watch restart / re-list), `deleted` and `error`. Dropping the
`Subscription` stops it; a context switch cancels all streams in Core.
External URLs open via `platform::open_url` (or `cx.open_url` in the palette).

---

## 20. Deliberate fixes and kept quirks

Deliberate fixes in this area (the native behavior is the spec; harness scenarios tagged `fix`):
* **Fix 1** — `/` (and every key) types normally while a text input has focus (§3, §8).
* **Fix 2** — `j`/`k`/`l` use the sorted, visible row order, same as ↑/↓/Enter (§3).
* **Fix 3** — SearchableDropdown ↑/↓/Enter/Esc work from inside its search input (§5).
* **Fix 4** — Esc never navigates back while a text entry has focus or something modal is open;
  with the palette/overlay open it only closes that overlay (§3, §17).
* **Fix 11** — the AI config carries the keychain API key, so key-based providers count as
  configured (§13.4).
* **Fix 12** — AI "Test Connection" reports HTTP errors (§17.1).
* **Fix 13** — the Ollama availability probe uses the configured base URL (§17.1).
* **Fix 14** — restart history is recorded, so restart sparklines appear (§7.8, §7.10).
* **Fix 17** — Mod+Shift+R opens RBAC (outside inputs) and does not also refresh (§3).

Shell bug fixes (former kept quirks; harness scenarios `10-shell-bug-*`, tags `shell, bugfix`):
* **ConfirmDialog** — titles use the Kubernetes kind ("Delete Ingress", "Delete ConfigMap",
  "Delete CronJob"); Esc and a backdrop click cancel it unless its action is running (§3, §10).
  `10-shell-bug-confirm-dialog`.
* **Palette** — group headings are uppercase muted section labels; the keyboard selection is
  highlighted; `:running` / `:failed` / `:pending` filter the table (with a header chip); Recent
  is re-read on every open; "Network Policies" view entry (§6, §14). `10-shell-bug-palette`.
* **Table values** — pod STATUS is kubectl's phase/reason (Running, Completed, CrashLoopBackOff,
  ContainerCreating, Init:…, Terminating…); RESTARTS sums all containers; Secret TYPE shows the
  secret type; Ingress PORTS shows "80" / "80, 443" (§7.5, §7.7, §7.8). `10-shell-bug-table-columns`.
* **Sidebar pinned** — one "Pinned" heading; pinned rows aligned with the other nav items (§4.3,
  §9). `10-shell-bug-pinned-heading`.
* **Default namespace per context** — applied when a context's namespaces load (§1 step 5, §17).
  `10-shell-bug-default-namespace`.
* **`d` shortcut** — deletes the highlighted table row via the confirm dialog (§3).
  `10-shell-bug-delete-shortcut`.
* **Initial context** — single favorite, else kubeconfig's current-context, else the first
  context (§1 step 4). `10-shell-bug-initial-context`.
* **"All Namespaces" trigger** — the namespace trigger shows "All Namespaces" for `*` (§5).
  `10-shell-bug-ns-all-label`.
* **Update check errors** — a failed "Check for Updates" shows "Couldn't check for updates" with
  the error instead of "You're up to date!" (§17). `10-shell-bug-update-error`.
* **Focus ring radius** — focused fields keep their corner radius (header note).
  `10-shell-bug-focus-radius` (screenshots).

Kept quirks (part of the current design; change only deliberately):
1. No accent-opacity tints: light-gray `PREFLIGHT_BORDER` borders and missing tints (header note).
2. No exit animations (ConfirmDialog, dropdowns, toasts) and no backdrop blur (§0.2).

---

## 21. Harness test scenarios (black-box: keyboard, clicking visible text, screenshots)

Preconditions unless stated: valid kubeconfig with ≥1 reachable context; fresh state (no
`favorites.json` entries, empty `local-storage.json` — the harness sandboxes `HOME`); window
1400×900. Scenario files: `harness/scenarios/10-shell-*.yaml`; they cite these items as §21.N.

**Boot / connection**
1. Launch with valid cluster, fresh state → AI Chat view is shown; sidebar shows "Kore", "Kubernetes Desktop", "CONTEXT", "NAMESPACES" (trigger text `default`), "Multi-Cluster", "VIEWS", "AI Chat" highlighted (accent text + left bar).
2. Launch with `KUBECONFIG=/nonexistent` → screen shows "Kore", "Kubernetes IDE", "No Kubeconfig Found", "HOW TO FIX", "Retry Connection", "Checking: /nonexistent" (if path reported). No sidebar.
3. Launch with an unreachable cluster endpoint → "Cluster Unreachable"; "Try a different context" lists contexts with "current" tag on the current one. Click a context → button reads "Connect to <ctx>". Click it → "Connecting..." then main UI (if reachable).
4. On error screen click "Show technical details" → raw error text visible in a monospace block.

**Sidebar & navigation**
5. Click "Pods" → header shows "Pods" + count pill; table headers NAME, NAMESPACE, STATUS, READY, RESTARTS, AGE. Pods item shows the same count at right.
6. For each kind click and verify header label & columns: Deployments (NAMESPACE NAME READY UP-TO-DATE AVAILABLE AGE), Jobs, CronJobs (SCHEDULE, LAST SCHEDULE, ACTIVE), Services (TYPE, CLUSTER-IP, EXTERNAL-IP, PORT(S)), Ingresses (CLASS, HOSTS, PORTS), ConfigMaps (header label "Configmaps", DATA KEYS), Secrets, Nodes (NAME STATUS ROLES AGE VERSION), Namespaces (NAME STATUS AGE), Events (LAST SEEN TYPE REASON OBJECT MESSAGE COUNT).
7. Click "Dashboard", "Graph", "Network Policies", "RBAC", "Custom Resources", "Helm", "AI Chat" → each view loads; header with search box is hidden; sidebar indicator moves.
8. Click gear (footer) → "Settings" page with "Back", "Esc", "General", "AI Provider", "Display", "Keyboard Shortcuts", "About". Press Esc → back to resource table.
9. Click "Multi-Cluster" → toggle turns accent; Pods table gains first column "CLUSTER" showing context names. Click again → column disappears.

**Context / namespace dropdowns**
10. Click "Namespaces" trigger → panel with search input and "All Namespaces" row plus namespaces alphabetically. Type `kube` → only matching rows, matched text bold accent; ghost completion visible (e.g. `kube-node-lease` suffix). Press Tab → input becomes full name.
11. Click "All Namespaces" → trigger shows "All Namespaces"; Pods table shows NAMESPACE values from multiple namespaces.
12. Click star next to `kube-system` → star fills yellow and row moves to top; reopen app → `kube-system` auto-selected (single favorite) and trigger shows a yellow star.
13. Open Context dropdown, click a different context → view switches to resource table, search cleared, header pill shows new context name.
14. Click outside an open dropdown → it closes. (Fix 3: in the search box, ↓/Enter select the highlighted match and Esc closes the panel.)

**Header / search / filters**
15. In Pods table press `/` → search input focused; type `core` → only rows whose name/namespace/status contains "core"; count pill updates; clear → all rows.
16. Type a non-matching string → "No resources found" + "Try switching namespace or adjusting your search".
17. Click Filter icon → label bar appears with placeholder "app=name". Type `k8s-app=kube-dns` + Enter (namespace kube-system) → chip appears, table refetches (skeleton) and shows only dns pods. Backspace in empty input → chip removed. (Fix 1: a selector containing `/` stays entirely in the label input.)
18. Switch kind via sidebar → label chips cleared and bar hidden.
19. Press ⌘R → skeleton rows (shimmer) briefly, then table repopulates.

**Table interaction**
20. Click column header "NAME" → ChevronUp accent icon, rows sorted A→Z; click again → ChevronDown, Z→A. Click "AGE" → oldest first.
21. Press `j` three times → third row highlighted; `k` → second row; `l` → details view of that row; breadcrumb "Pods › <name>". Press `h` → back to table. (Fix 2: with a sort active, `j` highlights the first visible row and `l` opens that same row.)
22. Press ↓/↑ and Enter → opens highlighted row.
23. Click a row → opens details directly.
24. Click "⋯" on a pod row → menu "View Logs", "Exec Shell", "Port Forward", "Delete". On deployment row → "Scale", "Restart", "Delete". On service row → "Describe", "Delete". Click elsewhere → menu closes.
25. Click "Delete" in menu → dialog "Delete Pod", text `Are you sure you want to delete "<name>" in namespace "<ns>"?`, buttons "Cancel"/"Delete". Click "Cancel" → closes. Repeat and click "Delete" → toast "<name> deleted" bottom-right (disappears after ~3s); row disappears (or is replaced) via watch.
26. Watch: `kubectl run harness-test --image=nginx` in the current namespace → new row "harness-test" appears without refresh; `kubectl delete pod harness-test` → row disappears.
27. Click star on a row → star turns amber/filled; sidebar shows "PINNED" section with the item (name + namespace). Click the pinned item → details view of that resource. Hover pinned item, click its X → section disappears when empty. Restart app → pins persist.

**Command palette**
28. Press ⌘K → overlay with placeholder "Search resources, navigate, switch context..."; groups "Views", "Resources", "Actions", "Contexts" visible. Press Esc → closes. ⌘K twice → toggles.
29. In palette type `Dashboard`, press Enter → Dashboard view. Type `Go to Secrets` + Enter → Secrets table.
30. Type a pod name fragment (≥2 chars) → "Search Results" group with the pod, kind pill "pods" and namespace; click it → pod details. Open the palette again (or restart the app) → "Recent" group lists it.
31. Type `:` → "Quick Filters" with ":pods" … ":pending"; select ":deployments" → Deployments table. Select ":failed" on Pods → only failed pods (e.g. crashloop) and a "Status: Failed" chip; its × clears it.
32. Type `zzzzqqq` → "No results found".
33. Click a context under "Contexts" → switches context (header pill updates).
34. Select "Ask AI" → AI panel opens.

**Shortcut overlay**
35. Press `?` (focus not in input) → "Keyboard Shortcuts", "Quick reference for all shortcuts", groups "NAVIGATION", "ACTIONS", "RBAC", "GENERAL", footer "Press ? or Esc to close". Press `?` → closes. Reopen, press Esc → closes; reopen, click backdrop → closes. (Fix 4: on Settings, Esc with the palette or overlay open closes only that overlay.)
36. Press ⌘⇧R → RBAC view (Fix 17: no refresh).

**Settings**
37. Settings › click "Violet" swatch → accent instantly changes (sidebar active item, header icons violet). Restart → still violet.
38. Settings › Default View select "Resource Table" → restart app → opens on Pods table.
39. Default Namespace per Context: type "ctx1" and "ns1", click "Add" → row "ctx1 → ns1"; click "×" → removed. "Add" disabled when either empty.
40. About › "Check for Updates" → "Checking..." then either "You're up to date!" modal ("Kore <ver> is the latest version.", "OK") or top banner "Kore <ver> is available" with "Update"; if the check fails, "Couldn't check for updates" with the error.
41. AI Provider: click "Anthropic" → "API Key" field with placeholder "Enter your Anthropic API key"; "Test Connection" disabled until key typed; enter a bad key → click → "Failed - Retry" + toast "Connection failed: …". Click "Ollama" → "Base URL" field with `http://localhost:11434` and model select or "Loading models...".

**Banners / misc**
42. Update available (mock `check_for_updates` has_update=true) → banner "Kore X is available"; click X → banner gone until next launch.
43. Impersonate an identity in RBAC → amber banner "Impersonation Mode:" "viewing as <id>" with "Exit"; click Exit → banner removed.
44. AI not configured (provider OpenAI, no key) → AI panel/chat shows "Configure an AI provider in Settings to use this feature".
45. Toast stacking: trigger two deletes quickly → two toasts stacked bottom-right; clicking one dismisses it.
