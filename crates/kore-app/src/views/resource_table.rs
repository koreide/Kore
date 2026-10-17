//! Resource table (spec 01 §7): kind-specific columns, auto column widths
//! (HTML auto table layout), sorting, status badges, pins, row action menu,
//! restart sparklines, skeleton + empty states. Virtualized via `uniform_list`.

use crate::automation::Automation;
use crate::state::resource_item::{format_relative_age, ResourceItem};
use crate::state::resources::SortKey;
use crate::state::types::{KindExt, ResourceKind};
use crate::state::AppState;
use crate::theme::{self, amber, misc, red, slate, KoreStyled, SANS};
use crate::ui::icon::icon;
use crate::ui::scrollbar;
use crate::ui::searchable_dropdown::star;
use crate::ui::text::{caps, measure_tracked};
use crate::ui::mono_text::mono_text;
use crate::ui::widgets::{restart_sparkline, skeleton_bar, status_badge_mono};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::rc::Rc;

pub const ROW_H: f32 = 41.;
pub const HEADER_H: f32 = 33.;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cell {
    /// Raw text; `None` → blank.
    Raw,
    /// Text or "-" when undefined.
    Dash,
    /// Numeric, 0 when undefined.
    Zero,
    Age,
    Badge,
    Restarts,
    EventType,
    Message,
    Cluster,
    Pin,
    Actions,
}

#[derive(Clone, Debug)]
pub struct Column {
    pub id: &'static str,
    pub header: &'static str,
    pub cell: Cell,
}

impl Column {
    fn sortable(&self) -> bool {
        !matches!(self.cell, Cell::Pin | Cell::Actions)
    }
}

const fn col(id: &'static str, header: &'static str, cell: Cell) -> Column {
    Column { id, header, cell }
}

/// Columns per kind (spec 01 §7.8), with the multi-cluster CLUSTER column and
/// the trailing pin/actions columns.
pub fn columns(kind: ResourceKind, multi_cluster: bool) -> Vec<Column> {
    use Cell::*;
    let mut cols = Vec::new();
    if multi_cluster {
        cols.push(col("_context", "Cluster", Cluster));
    }
    let age = col("age", "Age", Age);
    cols.extend(match kind {
        ResourceKind::Deployments => vec![
            col("namespace", "Namespace", Raw),
            col("name", "Name", Raw),
            col("ready", "Ready", Raw),
            col("upToDate", "Up-to-date", Dash),
            col("available", "Available", Dash),
            age,
        ],
        ResourceKind::Services => vec![
            col("namespace", "Namespace", Raw),
            col("name", "Name", Raw),
            col("type", "Type", Dash),
            col("clusterIp", "Cluster-IP", Dash),
            col("externalIp", "External-IP", Dash),
            col("ports", "Port(s)", Dash),
            age,
        ],
        ResourceKind::Nodes => vec![
            col("name", "Name", Raw),
            col("status", "Status", Badge),
            col("roles", "Roles", Dash),
            age,
            col("version", "Version", Dash),
        ],
        ResourceKind::Events => vec![
            col("lastSeen", "Last Seen", Age),
            col("eventType", "Type", EventType),
            col("reason", "Reason", Dash),
            col("involvedObject", "Object", Raw),
            col("message", "Message", Message),
            col("count", "Count", Dash),
        ],
        ResourceKind::Configmaps => vec![
            col("namespace", "Namespace", Raw),
            col("name", "Name", Raw),
            col("dataKeys", "Data Keys", Zero),
            age,
        ],
        ResourceKind::Secrets => vec![
            col("namespace", "Namespace", Raw),
            col("name", "Name", Raw),
            col("secretType", "Type", Dash),
            col("dataKeys", "Data Keys", Zero),
            age,
        ],
        ResourceKind::Ingresses => vec![
            col("namespace", "Namespace", Raw),
            col("name", "Name", Raw),
            col("ingressClass", "Class", Dash),
            col("hosts", "Hosts", Dash),
            col("ingressPorts", "Ports", Dash),
            age,
        ],
        ResourceKind::Jobs => vec![
            col("namespace", "Namespace", Raw),
            col("name", "Name", Raw),
            col("status", "Status", Badge),
            col("completions", "Completions", Dash),
            col("duration", "Duration", Dash),
            age,
        ],
        ResourceKind::Cronjobs => vec![
            col("namespace", "Namespace", Raw),
            col("name", "Name", Raw),
            col("schedule", "Schedule", Dash),
            col("lastSchedule", "Last Schedule", Age),
            col("active", "Active", Zero),
            age,
        ],
        ResourceKind::Namespaces => vec![col("name", "Name", Raw), col("status", "Status", Badge), age],
        ResourceKind::Pods => vec![
            col("name", "Name", Raw),
            col("namespace", "Namespace", Raw),
            col("status", "Status", Badge),
            col("ready", "Ready", Raw),
            col("restarts", "Restarts", Restarts),
            age,
        ],
    });
    cols.push(col("pin", "", Pin));
    cols.push(col("actions", "", Actions));
    cols
}

/// Display text of a text-like cell (None = blank cell).
pub fn cell_text(item: &ResourceItem, c: &Column) -> Option<String> {
    let dash = |v: Option<String>| Some(v.unwrap_or_else(|| "-".into()));
    match (c.id, c.cell) {
        (_, Cell::Age) => Some(format_relative_age(if c.id == "age" {
            item.age.as_deref()
        } else if c.id == "lastSeen" {
            item.last_seen.as_deref()
        } else {
            item.last_schedule.as_deref()
        })),
        ("name", _) => Some(item.name.clone()),
        ("namespace", _) => Some(item.namespace.clone()),
        ("ready", _) => item.ready.clone(),
        ("involvedObject", _) => item.involved_object.clone(),
        ("upToDate", _) => dash(item.up_to_date.map(|v| v.to_string())),
        ("available", _) => dash(item.available.map(|v| v.to_string())),
        ("type", _) => dash(item.type_.clone()),
        ("clusterIp", _) => dash(item.cluster_ip.clone()),
        ("externalIp", _) => dash(item.external_ip.clone()),
        ("ports", _) => dash(item.ports.clone()),
        ("roles", _) => dash(item.roles.clone()),
        ("version", _) => dash(item.version.clone()),
        ("reason", _) => dash(item.reason.clone()),
        ("message", _) => dash(item.message.clone()),
        ("count", _) => dash(item.count.map(|v| v.to_string())),
        ("dataKeys", _) => Some(item.data_keys.unwrap_or(0).to_string()),
        ("secretType", _) => dash(item.secret_type.clone()),
        ("ingressPorts", _) => dash(item.ingress_ports.clone()),
        ("ingressClass", _) => dash(item.ingress_class.clone()),
        ("hosts", _) => dash(item.hosts.clone()),
        ("completions", _) => dash(item.completions.clone()),
        ("duration", _) => dash(item.duration.clone()),
        ("schedule", _) => dash(item.schedule.clone()),
        ("active", _) => Some(item.active.unwrap_or(0).to_string()),
        ("eventType", _) => dash(item.event_type.clone()),
        ("_context", _) => dash(item.context.clone()),
        ("restarts", _) => Some(item.restarts.unwrap_or(0).to_string()),
        ("status", _) => Some(item.status.clone().unwrap_or_else(|| "-".into())),
        _ => None,
    }
}

/// Row action menu entries: (label, icon, action id).
pub fn row_actions(kind: ResourceKind) -> Vec<(&'static str, &'static str, &'static str)> {
    match kind {
        ResourceKind::Pods => vec![
            ("View Logs", "scroll-text", "logs"),
            ("Exec Shell", "terminal", "exec"),
            ("Port Forward", "network", "port-forward"),
            ("Delete", "trash-2", "delete"),
        ],
        ResourceKind::Deployments => {
            vec![("Scale", "arrow-up-down", "scale"), ("Restart", "refresh-cw", "restart"), ("Delete", "trash-2", "delete")]
        }
        _ => vec![("Describe", "file-search", "describe"), ("Delete", "trash-2", "delete")],
    }
}

struct WidthCache {
    key: (u64, String, ResourceKind, bool, i32, Option<crate::state::resource_item::StatusVariant>),
    widths: Rc<Vec<f32>>,
}

pub struct ResourceTable {
    app: Entity<AppState>,
    scroll: UniformListScrollHandle,
    gutter: scrollbar::Gutter,
    h_scroll: ScrollHandle,
    menu: Option<(String, String)>,
    widths: Option<WidthCache>,
    mono_advance: Option<f32>,
    /// Press position recorded by the pin/actions buttons so the row ignores
    /// the click that started on them.
    suppress_row_click: std::rc::Rc<std::cell::Cell<Option<Point<Pixels>>>>,
    _subs: Vec<Subscription>,
}

impl ResourceTable {
    pub fn new(app: Entity<AppState>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let subs = vec![cx.observe(&app, |_, _, cx| cx.notify())];
        ResourceTable {
            app,
            scroll: UniformListScrollHandle::new(),
            gutter: Default::default(),
            h_scroll: ScrollHandle::new(),
            menu: None,
            widths: None,
            mono_advance: None,
            suppress_row_click: Default::default(),
            _subs: subs,
        }
    }

    /// Scroll so that visible row `ix` is in view (`block: nearest`).
    pub fn scroll_to_row(&self, ix: usize) {
        self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
    }

    /// Mono advance used for column sizing (see `MONO_ADVANCE_EM`).
    fn mono_advance(&mut self, window: &mut Window) -> f32 {
        let _ = window;
        *self.mono_advance.get_or_insert(12. * theme::MONO_ADVANCE_EM)
    }

    fn header_width(&mut self, window: &mut Window, c: &Column) -> f32 {
        let mut w = measure_tracked(window, &c.header.to_uppercase(), SANS, 12., FontWeight::MEDIUM, 0.05);
        if c.sortable() {
            w += 4. + 14.;
        }
        w + 24.
    }

    /// Natural (max-content) width per column, then the HTML auto-layout
    /// distribution of extra space proportional to those widths.
    fn compute_widths(&mut self, cols: &[Column], rows: &[ResourceItem], avail: f32, window: &mut Window, cx: &App) -> Rc<Vec<f32>> {
        let app = self.app.read(cx);
        let version = app.resources.read(cx).version;
        let key = (version, app.search.clone(), app.kind, app.multi_cluster, avail as i32, app.status_filter);
        if let Some(c) = &self.widths {
            if c.key == key {
                return c.widths.clone();
            }
        }
        let adv = self.mono_advance(window);
        let mut widths: Vec<f32> = Vec::with_capacity(cols.len());
        for c in cols {
            let mut w = self.header_width(window, c);
            let content = match c.cell {
                Cell::Pin => 22.,
                Cell::Actions => 24.,
                Cell::Badge => rows
                    .iter()
                    .map(|r| 8. + 6. + 6. + adv * r.status.as_deref().unwrap_or("-").chars().count() as f32 + 8.)
                    .fold(0., f32::max),
                Cell::Message => rows
                    .iter()
                    .map(|r| (adv * cell_text(r, c).map(|t| t.chars().count()).unwrap_or(0) as f32).min(300.))
                    .fold(0., f32::max),
                Cell::Cluster => rows
                    .iter()
                    .map(|r| (adv * cell_text(r, c).map(|t| t.chars().count()).unwrap_or(0) as f32).min(120.))
                    .fold(0., f32::max),
                _ => rows
                    .iter()
                    .map(|r| adv * cell_text(r, c).map(|t| t.chars().count()).unwrap_or(0) as f32)
                    .fold(0., f32::max),
            };
            w = w.max(content + 24.);
            widths.push(w);
        }
        let total: f32 = widths.iter().sum();
        if total > 0. && total < avail {
            let f = avail / total;
            for w in widths.iter_mut() {
                *w *= f;
            }
        }
        let widths = Rc::new(widths);
        self.widths = Some(WidthCache { key, widths: widths.clone() });
        widths
    }

    fn open_row(&mut self, ix: usize, item: ResourceItem, cx: &mut Context<Self>) {
        self.app.update(cx, |a, cx| {
            a.set_selected_row_index(ix as i64, cx);
            a.open_resource(None, item, cx);
        });
    }

    fn row_action(&mut self, action: &str, item: ResourceItem, cx: &mut Context<Self>) {
        self.menu = None;
        let kind = self.app.read(cx).kind;
        if action == "delete" {
            self.app.update(cx, |a, cx| a.request_delete(kind, item, cx));
        } else {
            // logs/exec/port-forward/describe/scale/restart: open details (no tab preselected).
            self.app.update(cx, |a, cx| a.open_resource(None, item, cx));
        }
        cx.notify();
    }

    fn render_header(&self, cols: &[Column], widths: &[f32], sorting: &[SortKey], accent: Rgba, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .h(px(HEADER_H))
            .flex_none()
            .bg(theme::hexa(0x101828, 0.8))
            .border_b_1()
            .border_color(slate::S800)
            .children(cols.iter().zip(widths.iter()).enumerate().map(|(i, (c, w))| {
                let sort = sorting.iter().find(|k| k.id == c.id).map(|k| k.desc);
                let id = c.id;
                let group: SharedString = format!("th-{id}").into();
                div()
                    .id(("th", i))
                    .group(group.clone())
                    .w(px(*w))
                    .flex_none()
                    .px(px(12.))
                    .py(px(8.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(4.))
                    .text_xs_tw()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(slate::S400)
                    .hover(move |s| s.text_color(accent))
                    .cursor_pointer()
                    .child(caps(c.header, 12.))
                    .when(c.sortable(), |d| {
                        d.child(match sort {
                            Some(false) => icon("chevron-up", 14.).text_color(accent),
                            Some(true) => icon("chevron-down", 14.).text_color(accent),
                            None => icon("chevrons-up-down", 14.).text_color(slate::S600),
                        })
                    })
                    .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                        let shift = e.modifiers().shift;
                        if let Some(c) = columns(this.app.read(cx).kind, this.app.read(cx).multi_cluster).iter().find(|c| c.id == id) {
                            if c.sortable() {
                                this.app.update(cx, |a, cx| a.toggle_sort(id, shift, cx));
                            }
                        }
                    }))
                    .auto_id(format!("table.header.{id}"), "columnheader", c.header.to_uppercase())
                    .selected(sort.is_some())
            }))
    }

    #[allow(clippy::too_many_arguments)]
    fn render_row(
        &self,
        ix: usize,
        item: &ResourceItem,
        cols: &[Column],
        widths: &[f32],
        selected_ix: i64,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let app = self.app.read(cx);
        let kind = app.kind;
        let selected = ix as i64 == selected_ix;
        let text_color = if selected { theme::hex(0xffffff) } else { slate::S100 };
        let pinned = app.is_pinned(kind.as_str(), &item.name, if item.namespace.is_empty() { "default" } else { &item.namespace });
        let history = if kind == ResourceKind::Pods {
            app.resources.read(cx).restart_history(&item.namespace, &item.name)
        } else {
            Vec::new()
        };
        let key = format!("{}/{}", item.namespace, item.name);
        let menu_open = self.menu.as_ref().map(|(n, ns)| *n == item.name && *ns == item.namespace).unwrap_or(false);
        let row_item = item.clone();
        let dbl_item = item.clone();

        let cells = cols.iter().zip(widths.iter()).enumerate().map(|(ci, (c, w))| {
            let base = div()
                .w(px(*w))
                .flex_none()
                .h_full()
                .px(px(12.))
                .flex()
                .flex_row()
                .items_center()
                .overflow_hidden();
            let content: AnyElement = match c.cell {
                Cell::Badge => status_badge_mono(("badge", ix), item.status.as_deref()).into_any_element(),
                Cell::Restarts => {
                    if history.is_empty() {
                        mono_text(item.restarts.unwrap_or(0).to_string()).into_any_element()
                    } else {
                        restart_sparkline(&history)
                    }
                }
                Cell::EventType => {
                    let t = item.event_type.clone();
                    div()
                        .text_color(if t.as_deref() == Some("Warning") { amber::A400 } else { misc::BLUE400 })
                        .child(mono_text(t.unwrap_or_else(|| "-".into())))
                        .into_any_element()
                }
                Cell::Message => mono_text(cell_text(item, c).unwrap_or_default()).truncate(px(300.)).into_any_element(),
                Cell::Cluster => mono_text(cell_text(item, c).unwrap_or_default()).truncate(px(120.)).into_any_element(),
                Cell::Pin => {
                    let (k, n, ns) = (kind.as_str().to_string(), item.name.clone(), item.namespace.clone());
                    let sup = self.suppress_row_click.clone();
                    div()
                        .id(("pin", ix))
                        .on_mouse_down(MouseButton::Left, move |e: &MouseDownEvent, _, _| sup.set(Some(e.position)))
                        .group("pin-btn")
                        .p(px(4.))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .child(star(pinned, 14.).map(|s| {
                            if pinned {
                                s.text_color(amber::A400).group_hover("pin-btn", |s| s.text_color(amber::A300))
                            } else {
                                s.text_color(slate::S600).group_hover("pin-btn", |s| s.text_color(slate::S400))
                            }
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.app.update(cx, |a, cx| a.toggle_pin(&k, &n, &ns, cx));
                        }))
                        .auto_id(format!("table.pin.{key}"), "button", if pinned { "Unpin" } else { "Pin" })
                        .selected(pinned)
                        .into_any_element()
                }
                Cell::Actions => self.render_actions(ix, item, kind, menu_open, cx).into_any_element(),
                _ => mono_text(cell_text(item, c).unwrap_or_default()).into_any_element(),
            };
            let _ = ci;
            // Automation: each data cell carries its rendered text; the RESTARTS cell
            // reports role "sparkline" when it renders through RestartSparkline.
            let content: AnyElement = match c.cell {
                Cell::Pin | Cell::Actions => content,
                Cell::Restarts => content
                    .auto_id(format!("table.cell.{key}.{}", c.id), if history.is_empty() { "cell" } else { "sparkline" }, cell_text(item, c).unwrap_or_default())
                    .into_any_element(),
                _ => content.auto_id(format!("table.cell.{key}.{}", c.id), "cell", cell_text(item, c).unwrap_or_default()).into_any_element(),
            };
            base.child(content)
        });

        div()
            .id(("row", ix))
            .w_full()
            .h(px(ROW_H))
            .flex()
            .flex_col()
            .cursor_pointer()
            .font_mono()
            .text_xs_tw()
            .text_color(text_color)
            .child(div().h(px(1.)).flex_none().when(ix > 0, |d| d.bg(theme::hexa(0x1e293b, 0.5))))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_row()
                    .map(|d| if selected { d.bg(theme::hexa(0x334155, 0.6)) } else { d.hover(|s| s.bg(theme::hexa(0x1d2939, 0.3))) })
                    .children(cells),
            )
            .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                if let Some(p) = this.suppress_row_click.take() {
                    let down = match e {
                        ClickEvent::Mouse(m) => m.down.position,
                        other => other.position(),
                    };
                    if p == down {
                        return;
                    }
                }
                if e.click_count() >= 2 {
                    this.app.update(cx, |a, cx| a.open_resource(None, dbl_item.clone(), cx));
                } else {
                    this.open_row(ix, row_item.clone(), cx);
                }
            }))
            .auto_id(format!("table.row.{key}"), "row", item.name.clone())
            .selected(selected)
            .into_any_element()
    }

    fn render_actions(&self, ix: usize, item: &ResourceItem, kind: ResourceKind, open: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let key = format!("{}/{}", item.namespace, item.name);
        let (n, ns) = (item.name.clone(), item.namespace.clone());
        let menu = open.then(|| {
            let entries = row_actions(kind);
            deferred(
                div()
                    .id(("menu", ix))
                    .absolute()
                    .right_0()
                    .top(px(24. + 4.))
                    .min_w(px(160.))
                    .rounded(px(8.))
                    .py(px(4.))
                    .glass()
                    .occlude()
                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                        this.menu = None;
                        cx.notify();
                    }))
                    .children(entries.into_iter().map(|(label, ic, action)| {
                        let item = item.clone();
                        let danger = action == "delete";
                        let color = if danger { red::R400 } else { slate::S300 };
                        div()
                            .id(SharedString::from(format!("menu-{action}")))
                            .w_full()
                            .px(px(12.))
                            .py(px(6.))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .text_xs_tw()
                            .text_color(color)
                            .hover(move |s| s.bg(if danger { theme::hexa(0xef4444, 0.1) } else { theme::hexa(0x1d2939, 0.4) }))
                            .child(icon(ic, 14.).text_color(color))
                            .child(label)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.row_action(action, item.clone(), cx);
                            }))
                            .auto_id(format!("table.menu.{action}"), "menuitem", label)
                    })),
            )
            .with_priority(2)
        });
        div()
            .relative()
            .child(
                div()
                    .id(("actions", ix))
                    .group("row-actions")
                    .p(px(4.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::hexa(0x1d2939, 0.4)))
                    .child(icon("more-horizontal", 16.).text_color(slate::S500).group_hover("row-actions", |s| s.text_color(slate::S300)))
                    .on_mouse_down(MouseButton::Left, {
                        let sup = self.suppress_row_click.clone();
                        move |e: &MouseDownEvent, _, _| sup.set(Some(e.position))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        let k = (n.clone(), ns.clone());
                        this.menu = if this.menu.as_ref() == Some(&k) { None } else { Some(k) };
                        cx.notify();
                    }))
                    .auto_id(format!("table.actions.{key}"), "button", "Actions"),
            )
            .children(menu)
    }
}

impl Render for ResourceTable {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        let app = self.app.read(cx);
        if app.loading(cx) {
            return skeleton_table().into_any_element();
        }
        let rows = app.visible_rows(cx);
        let kind = app.kind;
        let multi = app.multi_cluster;
        let sorting = app.sorting.clone();
        let mut sel = app.selected_row_index;
        if sel >= rows.len() as i64 && sel >= 0 {
            // Clamp an out-of-range highlight.
            let clamped = (rows.len() as i64 - 1).max(0);
            sel = clamped;
            self.app.update(cx, |a, _| a.selected_row_index = clamped);
        }
        if rows.is_empty() {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(12.))
                .text_color(slate::S400)
                .child(icon("inbox", 48.).text_color(slate::S600))
                .child(div().text_sm_tw().font_weight(FontWeight::MEDIUM).child("No resources found"))
                .child(div().text_xs_tw().text_color(slate::S500).child("Try switching namespace or adjusting your search"))
                .auto_id("table.empty", "status", "No resources found")
                .into_any_element();
        }
        let cols = columns(kind, multi);
        // Available width: content area minus the 1px glass border on each side.
        let viewport = window.viewport_size();
        let gutter = self.gutter.overflows();
        let avail = f32::from(viewport.width) - crate::views::sidebar::WIDTH - 32. - 2. - if gutter { scrollbar::WIDTH } else { 0. };
        let widths = self.compute_widths(&cols, &rows, avail, window, cx);
        let total: f32 = widths.iter().sum();
        let header = self.render_header(&cols, &widths, &sorting, accent, cx);
        let n = rows.len();
        let rows_for_list = rows.clone();
        let cols_for_list = cols.clone();
        let widths_for_list = widths.clone();
        let list = uniform_list(
            "resource-rows",
            n,
            cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                range
                    .map(|ix| {
                        this.render_row(ix, &rows_for_list[ix], &cols_for_list, &widths_for_list, sel, cx)
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .w(px(total))
        .flex_1()
        .min_h_0();

        let base = self.scroll.0.borrow().base_handle.clone();
        div()
            .size_full()
            .rounded(px(8.))
            .glass()
            .overflow_hidden()
            .child(
                div()
                    .id("table-hscroll")
                    .size_full()
                    .relative()
                    .flex()
                    .flex_row()
                    .overflow_x_scroll()
                    .track_scroll(&self.h_scroll)
                    .child(
                        // CSS: row 0 has no top border; every row here carries a 1px top
                        // border, so the list starts 1px under the sticky header's border.
                        div()
                            .w(px(total))
                            .h_full()
                            .flex_none()
                            .relative()
                            .flex()
                            .flex_col()
                            .child(div().h(px(HEADER_H - 1.)).flex_none())
                            .child(list)
                            .child(div().absolute().top_0().left_0().w(px(total)).child(header)),
                    )
                    .when(gutter, |d| d.child(div().w(px(scrollbar::WIDTH)).flex_none()))
                    .child(scrollbar::vertical(base, self.gutter.clone())),
            )
            .auto_id("table", "table", kind.header_label())
            .into_any_element()
    }
}

/// SkeletonTable (spec 01 §7.2).
pub fn skeleton_table() -> impl IntoElement {
    const ROWS: [[f32; 5]; 8] = [
        [40., 20., 15., 12., 10.],
        [55., 25., 18., 10., 8.],
        [35., 30., 12., 15., 10.],
        [60., 15., 20., 8., 12.],
        [45., 22., 16., 14., 9.],
        [50., 18., 14., 11., 10.],
        [38., 28., 17., 13., 8.],
        [52., 20., 15., 10., 11.],
    ];
    const HEAD: [f32; 5] = [16., 14., 10., 10., 8.];
    div()
        .size_full()
        .rounded(px(8.))
        .overflow_hidden()
        .glass()
        .child(
            div()
                .flex()
                .flex_row()
                .gap(px(16.))
                .p(px(12.))
                .border_b_1()
                .border_color(slate::S800)
                .children(HEAD.iter().enumerate().map(|(i, w)| skeleton_bar(("sk-h", i), relative(w / 100.), 12.))),
        )
        .children(ROWS.iter().enumerate().map(|(r, ws)| {
            div()
                .flex()
                .flex_row()
                .gap(px(16.))
                .p(px(12.))
                .border_b_1()
                .border_color(theme::hexa(0x1e293b, 0.5))
                .opacity(1. - 0.08 * r as f32)
                .children(ws.iter().enumerate().map(move |(i, w)| skeleton_bar(("sk", r * 10 + i), relative(w / 100.), 12.)))
        }))
        .auto_id("table.skeleton", "progressbar", "Loading")
}
