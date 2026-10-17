//! Root view: boot gating (spec 01 §1), global layout (§2), view routing,
//! overlays, global keyboard shortcuts (§3 + Fixes 1, 2, 4) and the
//! automation host.

use crate::automation::{frames, registry, Automation};
#[cfg(any(test, feature = "test-harness"))]
use crate::automation::AutomationHost;
use crate::keys::{self, Key};
use crate::state::resource_item::ResourceItem;
use crate::state::settings::Settings;
#[cfg(any(test, feature = "test-harness"))]
use crate::state::types::KindExt;
use crate::state::types::{AppView, ResourceKind};
use crate::state::{AppEvent, AppState};
use crate::theme::{self, slate, KoreStyled};
use crate::ui::toast::{self, ToastStore};
use crate::ui::widgets::spinner;
use crate::views::{
    ai_chat::AiChatView, ai_panel::AiPanel, banners, command_palette::CommandPalette, connection_setup::ConnectionSetup,
    crd_browser::CrdBrowserView, dashboard::DashboardView, deployment_details::DeploymentDetailsView, graph::GraphView,
    header::Header, helm_detail::HelmDetailView, helm_releases::HelmReleasesView, network_policy::NetworkPolicyView,
    pod_details::PodDetailsView, rbac::RbacView, resource_details::ResourceDetailsView, resource_table::ResourceTable,
    settings::SettingsView, shortcut_overlay::ShortcutOverlay, sidebar::Sidebar,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

pub const ROOT_KEY_CONTEXT: &str = "KoreApp";

pub fn init_keymap(cx: &mut App) {
    crate::ui::input::init_keymap(cx);
}

/// Identity of the routed view: a new key mounts a fresh view.
#[derive(Clone, PartialEq, Debug)]
enum RouteKey {
    Table,
    Details(ResourceKind, String, String),
    View(AppView),
    HelmDetail(String, String),
}

pub struct KoreApp {
    pub(crate) app: Entity<AppState>,
    focus: FocusHandle,
    sidebar: Entity<Sidebar>,
    header: Entity<Header>,
    table: Entity<ResourceTable>,
    toasts: Entity<ToastStore>,
    setup: Option<Entity<ConnectionSetup>>,
    route: Option<(RouteKey, AnyView)>,
    palette: Option<Entity<CommandPalette>>,
    shortcuts: Option<Entity<ShortcutOverlay>>,
    ai_panel: Option<Entity<AiPanel>>,
    _subs: Vec<Subscription>,
}

impl KoreApp {
    pub fn new(settings: Settings, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let toasts = toast::init(cx);
        let app = cx.new(|cx| AppState::new(settings, cx));
        let sidebar = cx.new(|cx| Sidebar::new(app.clone(), window, cx));
        let header = cx.new(|cx| Header::new(app.clone(), window, cx));
        let table = cx.new(|cx| ResourceTable::new(app.clone(), window, cx));
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let subs = vec![
            cx.observe(&app, |_, _, cx| cx.notify()),
            cx.observe(&toasts, |_, _, cx| cx.notify()),
            keys::on_window_key(cx, Self::on_key),
        ];
        app.update(cx, |a, cx| a.boot(cx));
        KoreApp {
            app,
            focus,
            sidebar,
            header,
            table,
            toasts,
            setup: None,
            route: None,
            palette: None,
            shortcuts: None,
            ai_panel: None,
            _subs: subs,
        }
    }


    // ── Global shortcuts (spec 01 §3) ───────────────────────────────────

    fn on_key(&mut self, k: &Key, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let app = self.app.read(cx);
        if !app.connection.as_ref().map(|c| c.connected).unwrap_or(false) {
            return false;
        }
        let in_input = keys::text_entry_focused(window);
        let view = app.view;
        let palette = app.palette_open;
        let overlay = app.shortcuts_open;
        let modal = app.modal_open();

        // ⌘K / Ctrl+K: everywhere, even in inputs.
        if k.mod_key("k") && !k.shift {
            self.app.update(cx, |a, cx| a.toggle_palette(cx));
            return true;
        }
        // Fix 17: ⌘⇧R opens the RBAC view (outside inputs); plain ⌘R refreshes.
        if k.mod_key("r") && k.shift {
            if in_input {
                return false;
            }
            self.app.update(cx, |a, cx| a.set_view(AppView::Rbac, cx));
            return true;
        }
        if k.mod_key("r") {
            self.app.update(cx, |a, cx| {
                a.refresh_resources(cx);
                cx.emit(AppEvent::Refresh);
            });
            return true;
        }
        if k.key == "escape" && k.plain() {
            if palette {
                self.app.update(cx, |a, cx| a.set_palette_open(false, cx));
                return true;
            }
            if overlay {
                self.app.update(cx, |a, cx| a.set_shortcuts_open(false, cx));
                return true;
            }
            // Esc cancels the confirm dialog (unless its action is already running).
            if app.confirm.is_some() {
                self.app.update(cx, |a, cx| a.dismiss_confirm(cx));
                return true;
            }
            // Fix 4: no back-navigation while a modal/dropdown or text entry has focus.
            if in_input || modal {
                return false;
            }
            self.app.update(cx, |_, cx| cx.emit(AppEvent::Escape));
            return false;
        }
        if in_input {
            // Fix 1: `/` (and every other plain key) types into the focused input.
            return false;
        }
        if k.is_slash() {
            if matches!(view, AppView::Table | AppView::Details) && !modal {
                self.header.update(cx, |h, cx| h.focus_search(window, cx));
            }
            return true;
        }
        if k.is_question() && k.plain() {
            if overlay {
                self.app.update(cx, |a, cx| a.set_shortcuts_open(false, cx));
                return true;
            }
            if !palette {
                self.app.update(cx, |a, cx| a.set_shortcuts_open(true, cx));
                return true;
            }
            return false;
        }
        if palette || overlay || modal {
            return false;
        }
        if k.is("h") && view == AppView::Details {
            self.app.update(cx, |a, cx| a.back(cx));
            return true;
        }
        if view != AppView::Table {
            return false;
        }
        let rows = self.app.read(cx).visible_rows(cx);
        let loading = self.app.read(cx).loading(cx);
        let n = rows.len() as i64;
        let cur = self.app.read(cx).selected_row_index;
        // Fix 2: j/k/l act on the sorted, visible order (same as ↑/↓/Enter).
        let open = |this: &mut Self, ix: i64, cx: &mut Context<Self>| {
            if ix >= 0 && ix < n {
                let item: ResourceItem = rows[ix as usize].clone();
                this.app.update(cx, |a, cx| a.open_resource(None, item, cx));
            }
        };
        if k.is("j") {
            self.app.update(cx, |a, cx| a.set_selected_row_index((cur + 1).min(n - 1).max(if n > 0 { 0 } else { -1 }), cx));
            return true;
        }
        if k.is("k") {
            self.app.update(cx, |a, cx| a.set_selected_row_index((cur - 1).max(0), cx));
            return true;
        }
        if k.is("l") {
            open(self, cur, cx);
            return true;
        }
        // `d`: delete the highlighted row (same confirm as the row menu's Delete).
        if k.is("d") {
            if cur >= 0 && cur < n {
                let item: ResourceItem = rows[cur as usize].clone();
                let kind = self.app.read(cx).kind;
                self.app.update(cx, |a, cx| a.request_delete(kind, item, cx));
            }
            return true;
        }
        if loading || n == 0 {
            return false;
        }
        if k.key == "down" && k.plain() {
            let next = if cur < n - 1 { cur + 1 } else { cur };
            self.app.update(cx, |a, cx| a.set_selected_row_index(next, cx));
            if next >= 0 {
                self.table.read(cx).scroll_to_row(next as usize);
            }
            return true;
        }
        if k.key == "up" && k.plain() {
            let next = if cur > 0 { cur - 1 } else { 0 };
            self.app.update(cx, |a, cx| a.set_selected_row_index(next, cx));
            self.table.read(cx).scroll_to_row(next as usize);
            return true;
        }
        if k.key == "enter" && k.plain() && !k.shift && cur >= 0 && cur < n {
            open(self, cur, cx);
            return true;
        }
        false
    }

    // ── Routing (spec 01 §2.1) ──────────────────────────────────────────

    fn route_key(app: &AppState) -> RouteKey {
        match app.view {
            AppView::Table => RouteKey::Table,
            AppView::Details => match &app.selected {
                Some(s) => RouteKey::Details(app.kind, s.namespace.clone(), s.name.clone()),
                None => RouteKey::Table,
            },
            AppView::HelmDetail => match &app.helm_release {
                Some(r) => RouteKey::HelmDetail(r.namespace.clone(), r.name.clone()),
                None => RouteKey::View(AppView::HelmDetail),
            },
            v => RouteKey::View(v),
        }
    }

    fn routed_view(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyView> {
        let key = Self::route_key(self.app.read(cx));
        if key == RouteKey::Table {
            self.route = None;
            return None;
        }
        if let Some((k, v)) = &self.route {
            if *k == key {
                return Some(v.clone());
            }
        }
        let app = self.app.clone();
        let view: Option<AnyView> = match &key {
            RouteKey::Table => None,
            RouteKey::Details(kind, _, _) => Some(match kind {
                ResourceKind::Pods => cx.new(|cx| PodDetailsView::new(app, window, cx)).into(),
                ResourceKind::Deployments => cx.new(|cx| DeploymentDetailsView::new(app, window, cx)).into(),
                _ => cx.new(|cx| ResourceDetailsView::new(app, window, cx)).into(),
            }),
            RouteKey::HelmDetail(..) => Some(cx.new(|cx| HelmDetailView::new(app, window, cx)).into()),
            RouteKey::View(v) => match v {
                AppView::Chat => Some(cx.new(|cx| AiChatView::new(app, window, cx)).into()),
                AppView::Dashboard => Some(cx.new(|cx| DashboardView::new(app, window, cx)).into()),
                AppView::Graph => Some(cx.new(|cx| GraphView::new(app, window, cx)).into()),
                AppView::NetworkPolicies => Some(cx.new(|cx| NetworkPolicyView::new(app, window, cx)).into()),
                AppView::Rbac => Some(cx.new(|cx| RbacView::new(app, window, cx)).into()),
                AppView::Crds => Some(cx.new(|cx| CrdBrowserView::new(app, window, cx)).into()),
                AppView::Helm => Some(cx.new(|cx| HelmReleasesView::new(app, window, cx)).into()),
                AppView::Settings => Some(cx.new(|cx| SettingsView::new(app, window, cx)).into()),
                // helm-detail without a release renders nothing (falls through).
                AppView::HelmDetail | AppView::Table | AppView::Details => None,
            },
        };
        self.route = view.clone().map(|v| (key, v));
        view
    }

    fn sync_overlays(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (palette, shortcuts, ai) = {
            let a = self.app.read(cx);
            (a.palette_open, a.shortcuts_open, a.ai_panel.is_some())
        };
        let app = self.app.clone();
        match (palette, self.palette.is_some()) {
            (true, false) => self.palette = Some(cx.new(|cx| CommandPalette::new(app.clone(), window, cx))),
            (false, true) => self.palette = None,
            _ => {}
        }
        match (shortcuts, self.shortcuts.is_some()) {
            (true, false) => self.shortcuts = Some(cx.new(|cx| ShortcutOverlay::new(app.clone(), window, cx))),
            (false, true) => self.shortcuts = None,
            _ => {}
        }
        match (ai, self.ai_panel.is_some()) {
            (true, false) => self.ai_panel = Some(cx.new(|cx| AiPanel::new(app.clone(), window, cx))),
            (false, true) => self.ai_panel = None,
            _ => {}
        }
    }

    fn render_main(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let view = self.app.read(cx).view;
        let show_header = matches!(view, AppView::Table | AppView::Details);
        let section: AnyElement = match self.routed_view(window, cx) {
            Some(v) => v.into_any_element(),
            None if view == AppView::Table => self.table.clone().into_any_element(),
            None => div().into_any_element(),
        };
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .children(banners::impersonation_banner(&self.app, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .p(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .map(|d| if show_header { d.child(self.header.clone()) } else { d.child(div()) })
                    .child(div().flex_1().min_h_0().relative().overflow_hidden().child(section)),
            )
            .into_any_element()
    }
}

#[cfg(any(test, feature = "test-harness"))]
impl AutomationHost for KoreApp {
    fn automation_state(&self, cx: &App) -> serde_json::Value {
        let mut v = self.app.read(cx).snapshot_json(cx);
        v["toasts"] = serde_json::to_value(&self.toasts.read(cx).toasts).unwrap_or_default();
        v
    }

    fn automation_action(
        &mut self,
        name: &str,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<serde_json::Value> {
        match name {
            // A minimized window is invisible to GPUI (no display link), which is
            // how Layer 2 checks it still works when covered or in the background.
            "minimize" => window.minimize_window(),
            // Content size in points: Layer 2 uses the same content size as Layer 3's placed window.
            "resize" => {
                let w = args["width"].as_f64().ok_or_else(|| anyhow::anyhow!("width"))? as f32;
                let h = args["height"].as_f64().ok_or_else(|| anyhow::anyhow!("height"))? as f32;
                window.resize(size(px(w), px(h)));
            }
            "navigate" => {
                let view: AppView = serde_json::from_value(args["view"].clone())?;
                self.app.update(cx, |m, cx| m.set_view(view, cx));
            }
            "set_kind" => {
                let kind = ResourceKind::parse(args["kind"].as_str().unwrap_or_default())
                    .ok_or_else(|| anyhow::anyhow!("unknown kind"))?;
                self.app.update(cx, |m, cx| m.set_kind(kind, cx));
            }
            "set_namespace" => {
                let ns = args["namespace"].as_str().unwrap_or("default").to_string();
                self.app.update(cx, |m, cx| m.set_namespace(ns, cx));
            }
            "toast" => {
                let msg = args["message"].as_str().unwrap_or("").to_string();
                toast::toast(cx, msg, toast::ToastKind::Info);
            }
            other => anyhow::bail!("unknown action {other:?}"),
        }
        Ok(serde_json::Value::Null)
    }
}

impl Render for KoreApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        frames::frame_started();
        registry::begin_frame();
        let accent = theme::accent(cx);
        let connection = self.app.read(cx).connection.clone();

        let content: AnyElement = match &connection {
            None => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(spinner("boot-spinner", 24., theme::PREFLIGHT_BORDER, accent))
                .auto_id("boot.spinner", "progressbar", "Loading")
                .into_any_element(),
            Some(c) if !c.connected => {
                let app = self.app.clone();
                let setup = self.setup.get_or_insert_with(|| cx.new(|cx| ConnectionSetup::new(app, window, cx))).clone();
                setup.into_any_element()
            }
            Some(_) => {
                self.setup = None;
                self.sync_overlays(window, cx);
                let main = self.render_main(window, cx);
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .children(banners::update_banner(&self.app, cx))
                    .child(div().flex_1().min_h_0().flex().flex_row().overflow_hidden().child(self.sidebar.clone()).child(main))
                    .into_any_element()
            }
        };
        let confirm = self.app.read(cx).confirm.clone();

        div()
            .id("kore-root")
            .key_context(ROOT_KEY_CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(theme::BACKGROUND)
            .text_color(slate::S100)
            .font_sans()
            .text_size(px(16.))
            .line_height(relative(1.5))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    // Clicking outside a text input blurs it (browser behaviour).
                    if keys::text_entry_focused(window) {
                        window.focus(&this.focus, cx);
                    }
                }),
            )
            .child(content)
            .children(self.ai_panel.clone())
            .children(self.palette.clone())
            .children(self.shortcuts.clone())
            .when_some(confirm, |d, req| d.child(crate::ui::confirm_dialog::render(&self.app, &req)))
            .child(self.toasts.clone())
            // Painted last: closes the frame for timing and the element registry.
            .child(
                canvas(|_, _, _| {}, |_, _, _, _| {
                    registry::end_frame();
                    frames::frame_finished();
                })
                .absolute()
                .size_0(),
            )
    }
}

