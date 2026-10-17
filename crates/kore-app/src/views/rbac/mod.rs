//! RBAC Simulator (spec 03 §7): Permission Matrix, Role Browser, Query and
//! Impersonation tabs. Impersonation sets `AppState::impersonation`, which the
//! global banner (`banners::impersonation_banner`) renders.

mod matrix;
mod picker;
mod query;
mod roles;
mod rule_chain;
pub mod forbidden;

use crate::automation::Automation;
use crate::backend;
use crate::keys;
use crate::state::app_state::Impersonation;
use crate::state::AppState;
use crate::theme::{self, amber, red, slate, KoreStyled};
use crate::ui::icon::icon;
use crate::ui::input::{InputEvent, InputStyle, TextInput};
use crate::ui::text::caps;
use crate::ui::widgets::spinning_icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use kore_core::state::rbac::{
    PermissionCell, PermissionCheckResult, PermissionMatrix, RbacIdentity, RbacIdentityList, ReverseLookupResult, RoleSummary,
};
use picker::{identity_label, IdentityPicker, PickerEvent};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

pub use picker::focus_ring;
pub use rule_chain::spans;
pub use forbidden::{forbidden_banner, ForbiddenBanner};


#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Matrix,
    Roles,
    Query,
    Impersonation,
}

const TABS: [(Tab, &str, &str, &str); 4] = [
    (Tab::Matrix, "Permission Matrix", "grid-3x3", "1"),
    (Tab::Roles, "Role Browser", "book-open", "2"),
    (Tab::Query, "Query", "message-circle-question", "3"),
    (Tab::Impersonation, "Impersonation", "user-cog", "4"),
];

pub const VERBS: [&str; 7] = ["get", "list", "watch", "create", "update", "patch", "delete"];
pub const RESOURCES: [&str; 22] = [
    "pods",
    "deployments",
    "replicasets",
    "statefulsets",
    "daemonsets",
    "services",
    "ingresses",
    "configmaps",
    "secrets",
    "nodes",
    "namespaces",
    "persistentvolumeclaims",
    "persistentvolumes",
    "serviceaccounts",
    "jobs",
    "cronjobs",
    "roles",
    "rolebindings",
    "clusterroles",
    "clusterrolebindings",
    "events",
    "networkpolicies",
];

#[derive(Clone)]
pub struct ActiveCell {
    pub resource: String,
    pub verb: String,
    pub cell: PermissionCell,
    pub top: f32,
    pub left: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QueryMode {
    Check,
    WhoCan,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelectId {
    Verb,
    Resource,
}

/// CSV of the matrix, as "Copy CSV" writes it.
pub fn matrix_csv(m: &PermissionMatrix) -> String {
    let mut lines = vec![std::iter::once("Resource").chain(VERBS).collect::<Vec<_>>().join(",")];
    for row in &m.rows {
        let mut cells = vec![row.resource.clone()];
        for v in VERBS {
            cells.push(row.verbs.get(v).map(|c| status_str(&c.status).to_string()).unwrap_or_else(|| "denied".into()));
        }
        lines.push(cells.join(","));
    }
    lines.join("\n")
}

pub fn status_str(s: &kore_core::state::rbac::PermissionStatus) -> &'static str {
    use kore_core::state::rbac::PermissionStatus::*;
    match s {
        Allowed => "allowed",
        Denied => "denied",
        Conditional => "conditional",
    }
}

/// Painted bounds of each matrix cell, keyed by (row, verb) index.
type CellBounds = Rc<RefCell<HashMap<(usize, usize), Bounds<Pixels>>>>;

pub struct RbacView {
    app: Entity<AppState>,
    namespace: Option<String>,
    tab: Tab,
    identities: Option<Rc<RbacIdentityList>>,
    selected: Option<RbacIdentity>,
    matrix: Option<Rc<PermissionMatrix>>,
    roles: Vec<RoleSummary>,
    reverse: Option<ReverseLookupResult>,
    loading: bool,
    error: Option<String>,
    impersonation: Option<RbacIdentity>,
    // Pickers: matrix + impersonation share `selected`; query has its own.
    matrix_picker: Entity<IdentityPicker>,
    query_picker: Entity<IdentityPicker>,
    imp_picker: Entity<IdentityPicker>,
    // Matrix
    popover: Option<ActiveCell>,
    cell_bounds: CellBounds,
    matrix_scroll: ScrollHandle,
    matrix_gutter: crate::ui::scrollbar::Gutter,
    // Roles
    roles_filter: Entity<TextInput>,
    roles_query: String,
    selected_role: Option<String>,
    // Query
    mode: QueryMode,
    verb: String,
    resource: String,
    query_identity: Option<RbacIdentity>,
    query_result: Option<PermissionCheckResult>,
    who_can: Vec<PermissionCheckResult>,
    query_loading: bool,
    query_ran: bool,
    /// The last query failure, shown in the Query tab (quirk 10.15, fixed:
    /// errors used to be swallowed).
    query_error: Option<String>,
    /// "Permission Denied" analysis of the latest 403 (spec 03 §7.8).
    forbidden: Entity<ForbiddenBanner>,
    select_open: Option<SelectId>,
    content_scroll: ScrollHandle,
    whocan_scroll: ScrollHandle,
    whocan_gutter: crate::ui::scrollbar::Gutter,
    tasks: HashMap<&'static str, Task<()>>,
    _subs: Vec<Subscription>,
}

impl RbacView {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let weak = app.downgrade();
        let mk = |id: &str, window: &mut Window, cx: &mut Context<Self>| {
            let w = weak.clone();
            cx.new(|cx| IdentityPicker::new(id, w, window, cx))
        };
        let matrix_picker = mk("matrix", window, cx);
        let query_picker = mk("query", window, cx);
        let imp_picker = mk("impersonation", window, cx);
        let roles_filter = cx.new(|cx| {
            TextInput::new(
                "Filter roles...",
                InputStyle {
                    text_color: slate::S200,
                    placeholder_color: slate::S600,
                    font_size: 14.,
                    line_height: 20.,
                    padding: (28., 12., 6., 6.),
                    ..Default::default()
                },
                window,
                cx,
            )
        });
        let namespace = Self::ns_of(app.read(cx));
        let impersonation = app.read(cx).impersonation.as_ref().and_then(|i| serde_json::from_value(i.identity.clone()).ok());
        let subs = vec![
            cx.observe(&app, |this: &mut Self, app, cx| {
                let ns = Self::ns_of(app.read(cx));
                let imp: Option<RbacIdentity> =
                    app.read(cx).impersonation.as_ref().and_then(|i| serde_json::from_value(i.identity.clone()).ok());
                if imp != this.impersonation {
                    this.impersonation = imp.clone();
                    if let Some(i) = imp {
                        this.set_selected(Some(i), cx);
                    }
                }
                if ns != this.namespace {
                    this.namespace = ns;
                    this.build_matrix(cx);
                    if this.tab == Tab::Roles {
                        this.fetch_roles(cx);
                    }
                }
                cx.notify();
            }),
            cx.subscribe(&matrix_picker, |this: &mut Self, _, ev: &PickerEvent, cx| {
                let PickerEvent::Select(i) = ev;
                this.select_identity(i.clone(), cx);
            }),
            cx.subscribe(&imp_picker, |this: &mut Self, _, ev: &PickerEvent, cx| {
                let PickerEvent::Select(i) = ev;
                this.set_selected(Some(i.clone()), cx);
            }),
            cx.subscribe(&query_picker, |this: &mut Self, p, ev: &PickerEvent, cx| {
                let PickerEvent::Select(i) = ev;
                this.query_identity = Some(i.clone());
                p.update(cx, |p, cx| {
                    p.selected = Some(i.clone());
                    cx.notify();
                });
                cx.notify();
            }),
            cx.subscribe(&roles_filter, |this: &mut Self, _, ev: &InputEvent, cx| {
                if let InputEvent::Change(q) = ev {
                    this.roles_query = q.clone();
                    cx.notify();
                }
            }),
            keys::on_window_key(cx, Self::on_key),
        ];
        let (forbidden, forbidden_sub) = forbidden_banner("rbac.forbidden", cx);
        let mut subs = subs;
        subs.push(forbidden_sub);
        let mut this = RbacView {
            app,
            namespace,
            tab: Tab::Matrix,
            identities: None,
            selected: None,
            matrix: None,
            roles: Vec::new(),
            reverse: None,
            loading: false,
            error: None,
            impersonation: impersonation.clone(),
            matrix_picker,
            query_picker,
            imp_picker,
            popover: None,
            cell_bounds: Default::default(),
            matrix_scroll: ScrollHandle::new(),
            matrix_gutter: Default::default(),
            roles_filter,
            roles_query: String::new(),
            selected_role: None,
            mode: QueryMode::Check,
            verb: "get".into(),
            resource: "pods".into(),
            query_identity: None,
            query_result: None,
            who_can: Vec::new(),
            query_loading: false,
            query_ran: false,
            query_error: None,
            forbidden,
            select_open: None,
            content_scroll: ScrollHandle::new(),
            whocan_scroll: ScrollHandle::new(),
            whocan_gutter: Default::default(),
            tasks: HashMap::new(),
            _subs: subs,
        };
        this.fetch_identities(cx);
        if let Some(i) = impersonation {
            this.set_selected(Some(i), cx);
        }
        this
    }

    /// Sets the header error (and the forbidden analysis for a 403).
    fn set_error(&mut self, msg: String, cx: &mut Context<Self>) {
        self.forbidden.update(cx, |b, cx| b.set_error(Some(&msg), cx));
        self.error = Some(msg);
    }

    fn ns_of(a: &AppState) -> Option<String> {
        (a.namespace != "*").then(|| a.namespace.clone())
    }

    // ── Data (use-rbac.ts) ──────────────────────────────────────────────

    fn fetch_identities(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        let t = backend::run(cx, |c| async move { c.rbac_list_identities().await }, |this: &mut Self, res, cx| {
            match res {
                Ok(l) => {
                    let l = Rc::new(l);
                    this.identities = Some(l.clone());
                    for p in [&this.matrix_picker, &this.query_picker, &this.imp_picker] {
                        p.update(cx, |p, cx| {
                            p.identities = Some(l.clone());
                            cx.notify();
                        });
                    }
                }
                Err(e) => this.set_error(backend::format_error(&e), cx),
            }
            cx.notify();
        });
        self.tasks.insert("identities", t);
    }

    fn build_matrix(&mut self, cx: &mut Context<Self>) {
        let Some(identity) = self.selected.clone() else { return };
        self.loading = true;
        self.error = None;
        let ns = self.namespace.clone();
        let t = backend::run(cx, move |c| async move { c.rbac_build_matrix(identity, ns).await }, |this: &mut Self, res, cx| {
            match res {
                Ok(m) => {
                    this.matrix = Some(Rc::new(m));
                    this.popover = None;
                }
                Err(e) => this.set_error(backend::format_error(&e), cx),
            }
            this.loading = false;
            cx.notify();
        });
        self.tasks.insert("matrix", t);
        cx.notify();
    }

    fn fetch_roles(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        let ns = self.namespace.clone();
        let t = backend::run(cx, move |c| async move { c.rbac_list_roles(ns).await }, |this: &mut Self, res, cx| {
            match res {
                Ok(r) => this.roles = r,
                Err(e) => this.set_error(backend::format_error(&e), cx),
            }
            cx.notify();
        });
        self.tasks.insert("roles", t);
    }

    fn reverse_lookup(&mut self, role: &RoleSummary, cx: &mut Context<Self>) {
        self.selected_role = Some(format!("{}/{}/{}", role.kind, role.namespace.clone().unwrap_or_default(), role.name));
        self.loading = true;
        self.error = None;
        let (k, n, ns) = (role.kind.clone(), role.name.clone(), role.namespace.clone());
        let t = backend::run(cx, move |c| async move { c.rbac_reverse_lookup(k, n, ns).await }, |this: &mut Self, res, cx| {
            match res {
                Ok(r) => this.reverse = Some(r),
                Err(e) => this.set_error(backend::format_error(&e), cx),
            }
            this.loading = false;
            cx.notify();
        });
        self.tasks.insert("lookup", t);
        cx.notify();
    }

    /// `rbac.setSelectedIdentity` (+ the matrix rebuild effect).
    fn set_selected(&mut self, identity: Option<RbacIdentity>, cx: &mut Context<Self>) {
        let changed = identity != self.selected;
        self.selected = identity.clone();
        for p in [&self.matrix_picker, &self.imp_picker] {
            let id = identity.clone();
            p.update(cx, |p, cx| {
                p.selected = id;
                cx.notify();
            });
        }
        if changed {
            self.build_matrix(cx);
        }
        cx.notify();
    }

    /// `handleIdentitySelect`: select + jump to the matrix.
    fn select_identity(&mut self, identity: RbacIdentity, cx: &mut Context<Self>) {
        self.set_selected(Some(identity), cx);
        self.set_tab(Tab::Matrix, cx);
    }

    fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        if tab != self.tab {
            self.tab = tab;
            self.popover = None;
            self.select_open = None;
            self.content_scroll = ScrollHandle::new();
            if tab == Tab::Roles {
                self.fetch_roles(cx);
            }
            cx.notify();
        } else if tab == Tab::Roles {
            // Effect keyed on activeTab does not re-run for the same tab.
        }
    }

    fn set_impersonation(&mut self, identity: Option<RbacIdentity>, cx: &mut Context<Self>) {
        let imp = identity.map(|i| Impersonation { label: identity_label(&i), identity: serde_json::to_value(&i).unwrap_or_default() });
        self.app.update(cx, |s, cx| s.set_impersonation(imp, cx));
    }

    fn on_key(&mut self, k: &keys::Key, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if k.key == "escape" && k.plain() {
            // An overlay above the view (palette, shortcuts, confirm, AI
            // panel) owns Esc.
            let a = self.app.read(cx);
            if a.palette_open || a.shortcuts_open || a.confirm.is_some() || a.ai_panel.is_some() {
                return false;
            }
            // An open identity picker closes first, also while its search
            // field has focus (Fix 4: Esc never navigates back from here).
            if let Some(p) = [&self.matrix_picker, &self.query_picker, &self.imp_picker].into_iter().find(|p| p.read(cx).is_open()).cloned() {
                p.update(cx, |p, cx| p.close(window, cx));
                return true;
            }
            if self.popover.is_some() {
                self.popover = None;
                cx.notify();
                return true;
            }
            if self.select_open.is_some() {
                self.select_open = None;
                cx.notify();
                return true;
            }
            return false;
        }
        if keys::text_entry_focused(window) || self.app.read(cx).palette_open || self.app.read(cx).shortcuts_open {
            return false;
        }
        for (i, (t, ..)) in TABS.iter().enumerate() {
            if k.is(&(i + 1).to_string()) {
                self.set_tab(*t, cx);
                return true;
            }
        }
        if k.key == "i" && !k.cmd && !k.ctrl && self.impersonation.is_some() {
            self.set_impersonation(None, cx);
            return true;
        }
        false
    }

    // ── Rendering ───────────────────────────────────────────────────────

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        div()
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(12.))
            .px(px(16.))
            .py(px(12.))
            .border_b_1()
            .border_color(slate::S800)
            .child(
                div()
                    .id("rbac-back")
                    .group("rbac-back")
                    .p(px(4.))
                    .rounded(px(4.))
                    .hover(|s| s.bg(theme::hexa(0x334155, 0.4)))
                    .cursor_pointer()
                    .child(icon("arrow-left", 16.).text_color(slate::S400).group_hover("rbac-back", |s| s.text_color(slate::S200)))
                    .on_click(cx.listener(|this, _, _, cx| this.app.update(cx, |s, cx| s.back(cx))))
                    .auto_id("rbac.back", "button", "Back"),
            )
            .child(icon("shield-check", 20.).text_color(accent))
            .child(div().text_sm_tw().font_weight(FontWeight::MEDIUM).text_color(slate::S200).child("RBAC Simulator"))
            .child(div().flex_1())
            .when_some(self.error.clone(), |d, e| {
                d.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(6.))
                        .text_xs_tw()
                        .text_color(red::R400)
                        .child(icon("alert-circle", 14.).text_color(red::R400))
                        .child(e.clone())
                        .auto_id("rbac.error", "alert", e),
                )
            })
    }

    fn render_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.))
            .px(px(16.))
            .py(px(8.))
            .border_b_1()
            .border_color(slate::S800)
            .children(TABS.iter().map(|(t, label, ic, key)| {
                let t = *t;
                let active = t == self.tab;
                let color = if active { slate::S200 } else { slate::S500 };
                let group: SharedString = format!("rbac-tab-{key}").into();
                div()
                    .id(SharedString::from(format!("rbac-tab-{key}")))
                    .group(group.clone())
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .px(px(12.))
                    .py(px(6.))
                    .rounded(px(6.))
                    .text_xs_tw()
                    .text_color(color)
                    .cursor_pointer()
                    .map(|d| {
                        if active {
                            d.bg(theme::hexa(0x334155, 0.6))
                        } else {
                            d.hover(|s| s.text_color(slate::S300).bg(theme::hexa(0x1e293b, 0.5)))
                        }
                    })
                    .child(icon(ic, 14.).text_color(color).when(!active, |s| s.group_hover(group.clone(), |s| s.text_color(slate::S300))))
                    .child(*label)
                    .child(rbac_kbd(key))
                    .on_click(cx.listener(move |this, _, _, cx| this.set_tab(t, cx)))
                    .auto_id(format!("tab.{}", label.to_lowercase().replace(' ', "-")), "tab", *label)
                    .selected(active)
            }))
    }

    fn render_matrix_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let h = f32::from(window.viewport_size().height) - 200.;
        let body: AnyElement = if let Some(m) = self.matrix.clone() {
            div().flex_1().min_h_0().child(self.render_matrix(&m, window, cx)).into_any_element()
        } else if !self.loading {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_sm_tw()
                .text_color(slate::S500)
                .child("Select an identity to view their permissions")
                .into_any_element()
        } else {
            div().into_any_element()
        };
        div()
            .h(px(h))
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(12.))
                    .child(self.matrix_picker.clone())
                    .when(self.loading, |d| d.child(spinning_icon("rbac-loading", "loader-2", 16., slate::S500))),
            )
            .child(body)
            .into_any_element()
    }

    fn render_impersonation_tab(&self, cx: &mut Context<Self>) -> AnyElement {
        let accent = theme::accent(cx);
        let bullet = |t: &str| {
            div()
                .flex()
                .flex_row()
                .items_start()
                .gap(px(8.))
                .child(div().mt(px(2.)).text_color(theme::hex(0x34d399)).child("•"))
                .child(div().flex_1().child(t.to_string()))
        };
        div()
            .max_w(px(512.))
            .flex()
            .flex_col()
            .gap(px(20.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(div().text_sm_tw().font_weight(FontWeight::MEDIUM).text_color(slate::S300).child("What does impersonation do?"))
                    .child(div().text_xs_tw().line_height(px(19.)).text_color(slate::S400).child(
                        "Impersonation lets you pick a ServiceAccount, User, or Group and see what they are allowed to do. When active, the Permission Matrix tab automatically shows that identity's permissions instead of requiring you to re-select it each time.",
                    ))
                    .child(
                        div()
                            .rounded(px(8.))
                            .border_1()
                            .border_color(theme::hexa(0x334155, 0.5))
                            .bg(theme::hexa(0x1e293b, 0.3))
                            .p(px(12.))
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(div().text_10().font_weight(FontWeight::MEDIUM).text_color(slate::S500).child(caps("While impersonating you can", 10.)))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.))
                                    .text_xs_tw()
                                    .text_color(slate::S400)
                                    .child(bullet("Browse the Permission Matrix pre-filled for the selected identity"))
                                    .child(bullet("Run permission queries scoped to that identity"))
                                    .child(bullet(
                                        "Quickly check \"would this identity be able to...?\" without leaving your current view",
                                    )),
                            )
                            .child(
                                div()
                                    .pt(px(4.))
                                    .border_t_1()
                                    .border_color(theme::hexa(0x334155, 0.3))
                                    .text_10()
                                    .text_color(theme::hexa(0xf59e0b, 0.7))
                                    .child("Read-only simulation — all actual API calls still use your real credentials."),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(div().text_xs_tw().font_weight(FontWeight::MEDIUM).text_color(slate::S500).child("Select an identity to impersonate"))
                    .child(div().flex().flex_row().items_center().gap(px(12.)).child(self.imp_picker.clone())),
            )
            .when(self.selected.is_some(), |d| {
                let impersonating = self.impersonation.is_some();
                let btn = div()
                    .id("rbac-imp-btn")
                    .px(px(16.))
                    .py(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .text_sm_tw()
                    .cursor_pointer();
                let btn = if impersonating {
                    btn.bg(theme::hexa(0xf59e0b, 0.1))
                        .border_color(theme::hexa(0xf59e0b, 0.3))
                        .text_color(amber::A300)
                        .hover(|s| s.bg(theme::hexa(0xf59e0b, 0.2)))
                        .child("Exit Impersonation")
                        .on_click(cx.listener(|this, _, _, cx| this.set_impersonation(None, cx)))
                        .auto_id("rbac.impersonation.exit", "button", "Exit Impersonation")
                } else {
                    btn.border_color(theme::PREFLIGHT_BORDER)
                        .text_color(accent)
                        .child("Enter Impersonation Mode")
                        .on_click(cx.listener(|this, _, _, cx| {
                            let sel = this.selected.clone();
                            this.set_impersonation(sel, cx);
                        }))
                        .auto_id("rbac.impersonation.enter", "button", "Enter Impersonation Mode")
                };
                d.child(div().flex().flex_row().items_center().gap(px(12.)).child(btn))
            })
            .into_any_element()
    }
}

/// RBAC-variant Kbd (spec 03 §0.5).
pub fn rbac_kbd(key: &str) -> impl IntoElement {
    div()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .min_w(px(24.))
        .h(px(20.))
        .px(px(4.))
        .bg(theme::hexa(0x1e293b, 0.8))
        .rounded(px(4.))
        .border_1()
        .border_color(theme::hexa(0x334155, 0.5))
        .shadow(vec![BoxShadow {
            color: theme::hexa(0, 0.3).into(),
            offset: point(px(0.), px(1.)),
            blur_radius: px(0.),
            spread_radius: px(0.),
            inset: false,
        }])
        .text_10()
        .font_mono()
        .text_color(slate::S400)
        .child(key.to_string())
}

impl Render for RbacView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.tab {
            Tab::Matrix => self.render_matrix_tab(window, cx),
            Tab::Roles => self.render_roles_tab(window, cx),
            Tab::Query => self.render_query_tab(window, cx),
            Tab::Impersonation => self.render_impersonation_tab(cx),
        };
        let popover = self.popover.clone().map(|p| self.render_popover(&p, cx));
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.render_header(cx))
            .child(self.render_tabs(cx))
            .child(
                div()
                    .id("rbac-content")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.content_scroll)
                    .p(px(16.))
                    .when(self.forbidden.read(cx).visible(), |d| d.child(div().mb(px(16.)).max_w(px(672.)).child(self.forbidden.clone())))
                    .child(content),
            )
            .children(popover)
            .with_animation("rbac-in", Animation::new(Duration::from_millis(300)), |el, t| el.opacity(t))
            .auto_id("view.rbac", "region", "RBAC Simulator")
    }
}

#[cfg(test)]
mod tests {
    use super::{matrix_csv, VERBS};
    use kore_core::state::rbac::{PermissionCell, PermissionMatrix, PermissionMatrixRow, PermissionStatus, RbacIdentity};
    use std::collections::HashMap;

    #[test]
    fn csv_writes_missing_cells_as_denied() {
        let mut verbs = HashMap::new();
        verbs.insert("get".to_string(), PermissionCell { status: PermissionStatus::Allowed, rule_chain: vec![] });
        verbs.insert("list".to_string(), PermissionCell { status: PermissionStatus::Conditional, rule_chain: vec![] });
        let m = PermissionMatrix {
            identity: RbacIdentity::User { name: "a".into() },
            namespace: None,
            rows: vec![PermissionMatrixRow { resource: "pods".into(), api_group: String::new(), verbs }],
        };
        let csv = matrix_csv(&m);
        assert_eq!(csv, "Resource,get,list,watch,create,update,patch,delete\npods,allowed,conditional,denied,denied,denied,denied,denied");
        assert_eq!(VERBS.len(), 7);
    }
}
