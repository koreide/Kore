# Kore UI conventions (`crates/kore-app`)

Read this before changing UI code. Behavior and visual specs per area are in `spec/`
(01 shell & navigation, 02 details & workloads, 03 cluster views, 04 backend).

## Module map

| Module (`crates/kore-app/src/…`) | Notes |
|---|---|
| `main.rs`, `root.rs`, `backend.rs`, `keys.rs` | Window, routing, global keymap, Core bridge. Feature work rarely needs to edit these. |
| `state/*` (`AppState`, `ResourceStore`, `ResourceItem`, pins, settings, storage) | Add fields/methods if needed, keep existing semantics. |
| `theme.rs`, `ui/*` (input, dropdown, toast, confirm, scrollbar, widgets, text, icon, mono_text, markdown) | Shared primitives. Extend freely, don't change the existing look. |
| `views/sidebar.rs`, `header.rs`, `resource_table.rs`, `connection_setup.rs`, `banners.rs` | App shell (spec 01). The multi-cluster table column lives in `resource_table.rs`. |
| `views/pod_details.rs`, `deployment_details.rs`, `resource_details.rs` + their tabs (logs, exec, metrics, port-forward, debug, YAML, rollback, events) | Details views (spec 02). |
| `views/dashboard.rs`, `graph.rs`, `crd_browser.rs`, `helm_releases.rs`, `helm_detail.rs`, `rbac/`, `network_policy.rs` | Cluster views (spec 03). `banners::impersonation_banner` reads `AppState::impersonation`. |
| `views/ai_chat.rs`, `ai_panel.rs`, `command_palette.rs`, `shortcut_overlay.rs`, `settings.rs` | AI, palette, overlays, settings (specs 01, 03). `banners::ai_config_warning()` shows the AI setup hint. |
| `automation/` | Element registry (`auto_id`); the Layer 2 control server (`test-harness` feature, `KORE_CONTROL_SOCKET`). |

Add new view files under `views/` and register them in `views/mod.rs`; a routed
view must keep the constructor below.

## View contract

Every routed view and overlay has exactly this constructor, and root calls it:

```rust
pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self
```

* Root creates the view **when the route is entered** and drops it when the route
  changes, so view state resets on re-entry. Details views are
  keyed by `(kind, namespace, name)`, helm-detail by the release.
* Read your inputs from `AppState`: `app.read(cx).selected` (the `ResourceItem` for
  details views), `.kind`, `.namespace` (`"*"` = all), `.context`,
  `.helm_release`, `.multi_cluster`, `.settings`.
* `cx.observe(&app, …)` if you render AppState-derived data;
  `cx.subscribe(&app, …)` for `AppEvent`s.
* Overlays (`CommandPalette`, `ShortcutOverlay`, `AiPanel`) are created when
  `palette_open` / `shortcuts_open` / `ai_panel` become set, and render their own
  backdrop (`absolute().inset_0()`), above the layout.

## Core (backend) access

`backend::core(cx) -> kore_core::Core` (cheap clone). Every `Core` method is an
async fn whose future runs on Core's own tokio runtime, so it can be awaited on
GPUI's executor:

```rust
let task = backend::run(cx, |core| async move { core.describe_pod(ns, name).await },
    |this: &mut Self, res, cx| { this.describe = Some(res); cx.notify(); });
self.load_task = Some(task); // dropping the Task cancels it
```

* Streams return `kore_core::Subscription<T>`; drain it in a `cx.spawn` loop and
  **coalesce** (`recv().await` then `try_recv_batch(n)`) into one update per batch
  (see `state/resources.rs::start_watch_loop`). Keep the `Task` in a field;
  dropping the view drops the task, which drops the subscription, which cancels
  the backend stream. Context switches also cancel all streams in Core.
* Timers: `backend::sleep(cx, Duration)` (GPUI timer). Don't use tokio APIs in
  the UI crate.
* Errors: `backend::format_error(&e)` turns a `K8sError` into the user-facing message
  (spec 01 error formatting).

## Navigation API (`AppState`, spec 01 §2.2)

| Call | Effect |
|---|---|
| `set_view(view)` | Sidebar/palette view click: view, `selected = None` (helm clears release). |
| `set_kind(kind)` | Sidebar kind click: kind, view=table, clears selection + labels. |
| `go_to_kind(kind)` | Palette "Go to X" (labels kept). |
| `open_resource(Some(kind) / None, item)` | Row/pinned/palette/dashboard/graph navigate: details view. Build a `ResourceItem { name, namespace, ..Default::default() }` when you only have a name. |
| `back()` | `h`/Back buttons: helm-detail → helm, else table. |
| `open_helm_release(release)` | helm → helm-detail. |
| `switch_context(name)`, `set_namespace(ns)` | As the sidebar dropdowns. |
| `toggle_palette`, `set_palette_open`, `set_shortcuts_open`, `open_ai_panel(AiPanelRequest)`, `close_ai_panel` | Overlays. |
| `set_impersonation(Some(Impersonation { label, identity }))` | RBAC banner. |
| `update_settings(|s| …)` | Persists `kore-settings`, applies accent app-wide, emits `SettingsChanged`. |
| `check_updates_now(done)`, `perform_update()` | Settings "Check for Updates". |
| `refresh_resources()` | Table refresh (⌘R does this). |

## Keyboard

* **Global shortcuts** live in `root.rs::on_key` (⌘K, ⌘R, ⌘⇧R, `/`, `?`, j/k/l/h,
  ↑/↓/Enter, Esc). Fixes 1, 2, 4 and 17 (⌘⇧R opens RBAC; ⌘R only refreshes) are
  implemented there.
* **Esc → back**: don't listen for Esc yourself to navigate. Subscribe to
  `AppEvent::Escape`, which root emits only when no modal/dropdown/palette/overlay
  is open and no text entry has focus (Fix 4). The placeholders show the pattern.
* **View-local keys** (tabs `1`–`6`, `b`, `d`, ⌘F, RBAC `i`, …): use
  `keys::on_window_key(cx, |this, key, window, cx| -> bool)`, a window-level key
  listener. It runs before focused elements; return `true` to consume. Check
  `keys::text_entry_focused(window)` yourself (so single-key shortcuts don't fire
  while typing) and `app.modal_open()` where the spec says so. Keep the returned `Subscription` in a field.
* **Text entry surfaces**: use `ui::input::TextInput` (single line). It
  registers itself with `keys::register_text_entry`, so global single-key
  shortcuts never fire while it has focus. A terminal or YAML editor must call
  `keys::register_text_entry(&focus_handle)` too. `TextInput` emits
  `InputEvent::{Change, Enter{shift}, Escape, Up, Down, Tab, BackspaceEmpty, Focus, Blur}`.
* **Modals you own** (debug-container dialog, popovers, …): call
  `app.set_modal("your-id", true/false)` so Esc does not navigate back (Fix 4).
  The shared confirm dialog and dropdowns already do this.

## Toasts and confirm dialog

```rust
ui::toast::toast(cx, format!("{name} deleted"), ToastKind::Success); // Info | Success | Error
app.update(cx, |s, cx| s.confirm(ConfirmRequest {
    title: "Rollback Helm Release".into(), message: "…".into(),
    confirm_label: "Rollback".into(), cancel_label: "Cancel".into(),
    variant: ConfirmVariant::Warning, // Danger | Warning | Info
    on_confirm: Rc::new(move |window, cx| { /* call Core, then */ app.update(cx, |s, cx| s.close_confirm(cx)); }),
}, cx));
```

`on_confirm` must close the dialog itself (`close_confirm`) when its work is done
(the dialog stays open while the async action runs). Esc and a backdrop click cancel the dialog
until the confirm button is pressed (`AppState::confirm_busy`); Cancel/X always close it.

## Theme and primitives

* Colors: `theme::{BACKGROUND, SURFACE, MUTED, PREFLIGHT_BORDER, PREFLIGHT_PLACEHOLDER}`,
  palettes `slate::S100…S900`, `emerald`, `amber`, `red`, `misc`;
  `theme::hexa(0x1e293b, 0.5)` for slate-800 at 50%; `theme::accent(cx)` for the
  user accent (never hard-code `#58d0ff`). By design, accent is only ever used at
  full opacity: borders that might look like a faint accent use `PREFLIGHT_BORDER`
  (`#e5e7eb`, 1px), and there are no accent-tinted backgrounds (spec 01 header note).
* `KoreStyled`: `.glass()`, `.font_mono()`, `.font_sans()`, `.text_xs_tw()` (12/16),
  `.text_sm_tw()` (14/20), `.text_base_tw()`, `.text_lg_tw()`, `.text_xl_tw()`,
  `.text_2xl_tw()` (size/line-height pairs of the type scale), `.text_10()`/`.text_11()`/`.text_9()`
  (size only; line height inherits). Root sets 16px / line-height 1.5 (relative).
* Icons: `ui::icon::icon("kebab-name", size)` (exact Lucide 0.424 glyphs in
  `assets/lucide`; add missing ones there). **An SVG does not inherit color**: always
  call `.text_color(c)` on the icon itself, and for hover colors put `.group(g)` on
  the parent and `.group_hover(g, |s| s.text_color(..))` on the icon. Filled star:
  `ui::searchable_dropdown::star(true, size)`.
* GPUI has no letter-spacing/text-transform: `ui::text::caps("Views", 10.)`
  (uppercase + 0.05em) or `tracked(text, size, em)`. Measure with
  `ui::text::measure`.
* Widgets: `ui::widgets::{kbd, status_badge, spinner, spinning_icon, skeleton_bar,
  restart_sparkline}`; `ui::scrollbar::{Gutter, vertical}` for the 6px
  scrollbars (they take layout space: reserve `scrollbar::WIDTH` when
  `gutter.overflows()`); `ui::searchable_dropdown::SearchableDropdown`;
  `ui::text_search_bar::TextSearchBar` (spec 01 §16, ⌘F in describe/YAML/logs:
  emits `Query/Next/Prev/Close`, the owner reports `set_matches(current, count)`;
  Esc in its input closes it without navigating back).
* Mono text: GPUI renders SF Mono at the font's own advance (0.618em). Kore's design
  lays mono text out at a tighter 0.601em pitch (`theme::MONO_ADVANCE_EM`).
  Wrapped mono (logs/YAML/JSON/code): `ui::mono_text::mono_paragraph(text)`
  (`.line_height(px(19.))`, `.colors(spans)`, `.highlights(spans)`, `.wrap(false)`;
  `row_count(text, width, size)` for virtualized row heights).
  For single-line mono text that must match, use `ui::mono_text::mono_text(text)`
  (`.truncate(max)` for ellipsis). It paints glyphs at the 0.601em pitch. Use
  `ui::mono_text::{pitch, width}` or the constant for layout math. For wrapped
  multi-line mono, either paint glyphs at `pitch()` (`layout_line` + `paint_glyph`,
  as in `ui/mono_text.rs`) or use `views::describe_content::pitched_mono_size`
  (scales the font to the pitch: columns are exact, glyphs ~2.7% small).
* Text that must wrap inside a flex row needs `.min_w_0()` on its flex child
  (GPUI flex items default to `min-width: auto`).
* GPUI has no backdrop blur. Overlays use the plain tint.
* Popovers/menus that must paint above everything: `deferred(div().absolute()…)`
  with `.on_mouse_down_out(…)` to close (see the table row menu).

## Automation ids

Tag interactive and asserted elements with `.auto_id(id, role, label)`
(`automation::Automation`; layout-neutral). Use `.selected(bool)` / `.disabled(bool)`.
Naming scheme: dot-separated, lowercase, `<area>.<thing>[.<key>]`:

| Area | Examples |
|---|---|
| sidebar | `sidebar.view.<app-view>` (`sidebar.view.chat`, `sidebar.view.network-policies`), `sidebar.kind.<kind>` (`sidebar.kind.pods`), `sidebar.multi-cluster`, `sidebar.settings`, `sidebar.sponsor`, `sidebar.pin.<ns>/<name>`, `sidebar.pin.unpin.<ns>/<name>` |
| dropdowns | `sidebar.context.trigger`, `sidebar.context.panel`, `sidebar.context.item.<value>`, `sidebar.context.star.<value>` (same for `sidebar.namespace.*`) |
| header | `header.breadcrumb`, `header.count`, `header.search`, `header.filter`, `header.context`, `header.ai`; label bar `labelbar.input`, `labelbar.chip.<label>`, `labelbar.remove.<label>` |
| table | `table`, `table.header.<column-id>`, `table.row.<ns>/<name>`, `table.pin.<ns>/<name>`, `table.actions.<ns>/<name>`, `table.menu.<action>`, `table.empty`, `table.skeleton` |
| dialogs | `confirm`, `confirm.ok`, `confirm.cancel`, `confirm.close`; `toast.<n>` |
| connection | `connection`, `connection.retry`, `connection.context.<ctx>`, `connection.details`, `connection.error` |
| banners | `banner.update`, `banner.update-install`, `banner.update-dismiss`, `banner.impersonation`, `banner.impersonation.exit`, `banner.ai-config` |
| views | `view.<app-view>` root region; tabs `tab.<name>` (`tab.logs`, `tab.yaml`); palette `palette`, `palette.input`, `palette.item.<value>` (cmdk value, e.g. `palette.item.view-Dashboard`); `shortcuts`; `ai-panel`, `ai-panel.close` |

The snapshot `state` (JSON) comes from `AppState::snapshot_json`: `view`, `kind`,
`context`, `namespace`, `multiCluster`, `search`, `labelFilters`, `sorting`,
`selectedRowIndex`, `selected`, `loading`, `rowCount`, `visibleCount`, `pins`,
`statusFilter`, `paletteOpen`, `shortcutsOpen`, `aiPanel`, `confirmOpen`, `confirmBusy`, `modals`,
`impersonation`, `update`, `accent`, `toasts`. Add fields there for view state that
has no visible text.

The same ids and state serve every test layer:
- **Layer 1** (`crate::testing::Kore`): `k.click(id)`, `k.element(id)`, `k.wait_label(id, ..)`, `k.state()`.
- **Layer 2/3 scenarios:** `click: {id: ..}`, `element: {id: .., label: ..}`, `state: [[pointer, value]]`.
- **The MCP server:** `click {id}`, `wait {element}`, `snapshot`.

An id ending in `*` matches by prefix (`toast.*`).

## Verifying

See `CLAUDE.md` "Testing layers" for which layer a change needs.

```bash
cargo build --release -p kore-app            # must be warning-free
just lint                                    # clippy as CI runs it
just test-unit                               # Layer 1: headless UI tests (crates/kore-app/src/tests/)
just test-app -f <tag>                       # Layer 2: drives target/release/kore (--features test-harness) in the background
just compare harness/runs/<new-run>          # visual regression vs harness/BASELINE_RUN
```

Inspect `harness/runs/<new-run>/compare/*.diff.png` with an image viewer. To drive the app
interactively, use the `kore` MCP server, or `$H launch` followed by `$H do --socket <sock> …`.

Known platform limitations, by design: letter-spacing is emulated per glyph (no
kerning across tracked letters), no backdrop blur, no native `title` tooltips.
