//! CRD browser (Task 6; spec 03 §3): CRD discovery list grouped by API group
//! with search + collapse, instance table with delete, instance detail as
//! highlighted JSON. Esc walks back one level (detail → list → none → table).

use crate::automation::Automation;
use crate::backend;
use crate::state::app_state::{ConfirmRequest, ConfirmVariant};
use crate::state::resource_item::format_relative_age;
use crate::state::types::AppView;
use crate::state::{AppEvent, AppState};
use crate::theme::{self, red, slate, KoreStyled, SANS};
use crate::ui::icon::icon;
use crate::ui::input::{InputEvent, InputStyle, TextInput};
use crate::ui::scrollbar;
use crate::ui::mono_text::mono_paragraph;
use crate::ui::text::{caps, measure_tracked};
use crate::ui::toast::{toast, ToastKind};
use crate::ui::widgets::skeleton_bar;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use crate::views::rbac::{forbidden_banner, ForbiddenBanner};
use kore_core::state::crd::{instance_namespace, CrdInfo};
use serde_json::Value;
use std::collections::HashSet;
use std::rc::Rc;
use std::time::Duration;

// ── JSON highlighting (`highlightJson`, reproduced via the same regex chain) ─

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsonColor {
    Accent,
    Emerald,
    Amber,
    Indigo,
    Slate,
}

/// Highlights pretty-printed JSON without changing it: object keys (a string
/// followed by `:`) in accent, and the value right after a `:` by type:
/// strings emerald, numbers amber, `true`/`false` indigo, `null` slate.
/// Array items and punctuation stay uncolored. (The original regex chain also
/// rewrote `:` inside strings, e.g. `"x:12"` → `"x: 12"`; fixed.)
pub fn highlight_json(json: &str) -> (String, Vec<(std::ops::Range<usize>, JsonColor)>) {
    let b = json.as_bytes();
    let mut ranges: Vec<(std::ops::Range<usize>, JsonColor)> = Vec::new();
    let mut after_colon = false;
    let mut i = 0;
    let next_non_ws = |mut j: usize| {
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        b.get(j).copied()
    };
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        let color = if c == b'"' {
            i += 1;
            while i < b.len() && b[i] != b'"' {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            i = (i + 1).min(b.len());
            if next_non_ws(i) == Some(b':') {
                Some(JsonColor::Accent)
            } else if after_colon {
                Some(JsonColor::Emerald)
            } else {
                None
            }
        } else if c == b'-' || c.is_ascii_digit() {
            i += 1;
            while i < b.len() && (b[i].is_ascii_digit() || matches!(b[i], b'.' | b'e' | b'E' | b'+' | b'-')) {
                i += 1;
            }
            after_colon.then_some(JsonColor::Amber)
        } else if c.is_ascii_alphabetic() {
            while i < b.len() && b[i].is_ascii_alphanumeric() {
                i += 1;
            }
            match &json[start..i] {
                "true" | "false" if after_colon => Some(JsonColor::Indigo),
                "null" if after_colon => Some(JsonColor::Slate),
                _ => None,
            }
        } else {
            // Punctuation (or any other char): skip one UTF-8 scalar.
            i += json[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
            None
        };
        if let Some(col) = color {
            ranges.push((start..i, col));
        }
        after_colon = c == b':' && i == start + 1;
    }
    (json.to_string(), ranges)
}

// ── View ────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct Detail {
    name: String,
    loading: bool,
    json: String,
}

pub struct CrdBrowserView {
    app: Entity<AppState>,
    namespace: Option<String>,
    crds: Vec<CrdInfo>,
    crds_loading: bool,
    crds_error: Option<String>,
    search: Entity<TextInput>,
    query: String,
    collapsed: HashSet<String>,
    selected: Option<CrdInfo>,
    instances: Vec<Value>,
    inst_loading: bool,
    inst_error: Option<String>,
    detail: Option<Detail>,
    deleting: bool,
    list_scroll: ScrollHandle,
    list_gutter: scrollbar::Gutter,
    table_scroll: ScrollHandle,
    table_gutter: scrollbar::Gutter,
    detail_scroll: ScrollHandle,
    detail_gutter: scrollbar::Gutter,
    crd_task: Option<Task<()>>,
    inst_task: Option<Task<()>>,
    detail_task: Option<Task<()>>,
    /// "Permission Denied" analysis of the latest 403 (spec 03 §7.8).
    forbidden: Entity<ForbiddenBanner>,
    _subs: Vec<Subscription>,
}

fn ns_of(a: &AppState) -> Option<String> {
    if a.namespace == "*" {
        None
    } else {
        Some(a.namespace.clone())
    }
}

fn meta_str(v: &Value, key: &str) -> Option<String> {
    v.get("metadata").and_then(|m| m.get(key)).and_then(|x| x.as_str()).filter(|s| !s.is_empty()).map(String::from)
}

impl CrdBrowserView {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| {
            TextInput::new(
                "Search CRDs...",
                InputStyle {
                    text_color: slate::S200,
                    placeholder_color: slate::S600,
                    font_size: 12.,
                    line_height: 16.,
                    ..Default::default()
                },
                window,
                cx,
            )
        });
        let namespace = ns_of(app.read(cx));
        let (forbidden, forbidden_sub) = forbidden_banner("crd.forbidden", cx);
        let subs = vec![
            forbidden_sub,
            cx.observe(&app, |this: &mut Self, app, cx| {
                let ns = ns_of(app.read(cx));
                if ns != this.namespace {
                    this.namespace = ns;
                    this.load_instances(cx);
                }
                cx.notify();
            }),
            cx.subscribe(&app, |this, _, ev: &AppEvent, cx| {
                if matches!(ev, AppEvent::Escape) {
                    this.escape(cx);
                }
            }),
            cx.subscribe(&search, |this, _, ev: &InputEvent, cx| {
                if let InputEvent::Change(s) = ev {
                    this.query = s.clone();
                }
                cx.notify();
            }),
        ];
        let mut v = CrdBrowserView {
            app,
            namespace,
            crds: Vec::new(),
            crds_loading: true,
            crds_error: None,
            search,
            query: String::new(),
            collapsed: HashSet::new(),
            selected: None,
            instances: Vec::new(),
            inst_loading: false,
            inst_error: None,
            detail: None,
            deleting: false,
            list_scroll: ScrollHandle::new(),
            list_gutter: Default::default(),
            table_scroll: ScrollHandle::new(),
            table_gutter: Default::default(),
            detail_scroll: ScrollHandle::new(),
            detail_gutter: Default::default(),
            crd_task: None,
            inst_task: None,
            detail_task: None,
            forbidden,
            _subs: subs,
        };
        v.crd_task = Some(backend::run(cx, |core| async move { core.list_crds().await }, |this: &mut Self, res, cx| {
            this.crds_loading = false;
            match res {
                Ok(c) => this.crds = c,
                Err(e) => this.crds_error = Some(backend::format_error(&e)),
            }
            let err = this.crds_error.clone();
            this.report_error(err, cx);
            cx.notify();
        }));
        v
    }

    fn report_error(&mut self, err: Option<String>, cx: &mut Context<Self>) {
        self.forbidden.update(cx, |b, cx| b.set_error(err.as_deref(), cx));
    }

    fn escape(&mut self, cx: &mut Context<Self>) {
        if self.detail.is_some() {
            self.detail = None;
            self.detail_task = None;
        } else if self.selected.is_some() {
            self.select_crd(None, cx);
        } else {
            self.app.update(cx, |a, cx| a.set_view(AppView::Table, cx));
        }
        cx.notify();
    }

    fn select_crd(&mut self, crd: Option<CrdInfo>, cx: &mut Context<Self>) {
        self.selected = crd;
        self.load_instances(cx);
    }

    fn load_instances(&mut self, cx: &mut Context<Self>) {
        self.detail = None;
        self.detail_task = None;
        let Some(crd) = self.selected.clone() else {
            self.instances.clear();
            self.inst_task = None;
            return;
        };
        self.inst_loading = true;
        self.inst_error = None;
        let ns = if crd.scope == "Namespaced" { self.namespace.clone() } else { None };
        self.inst_task = Some(backend::run(
            cx,
            move |core| async move { core.list_crd_instances(crd.group, crd.version, crd.plural, ns).await },
            |this: &mut Self, res, cx| {
                this.inst_loading = false;
                match res {
                    Ok(v) => this.instances = v,
                    Err(e) => this.inst_error = Some(backend::format_error(&e)),
                }
                let err = this.inst_error.clone().or_else(|| this.crds_error.clone());
                this.report_error(err, cx);
                cx.notify();
            },
        ));
        cx.notify();
    }

    fn open_instance(&mut self, inst: &Value, cx: &mut Context<Self>) {
        let Some(crd) = self.selected.clone() else { return };
        let name_raw = meta_str(inst, "name").unwrap_or_default();
        // Cluster-scoped CRDs are addressed without a namespace (quirk 10.10, fixed).
        let ns = instance_namespace(&crd.scope, meta_str(inst, "namespace").as_deref());
        self.detail = Some(Detail { name: meta_str(inst, "name").unwrap_or_else(|| "unknown".into()), loading: true, json: String::new() });
        self.detail_task = Some(backend::run(
            cx,
            move |core| async move { core.get_crd_instance(crd.group, crd.version, crd.plural, ns, name_raw).await },
            |this: &mut Self, res, cx| {
                if let Some(d) = &mut this.detail {
                    d.loading = false;
                    d.json = match res {
                        Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_default(),
                        Err(e) => {
                            let msg = backend::format_error(&e);
                            this.forbidden.update(cx, |b, cx| b.set_error(Some(&msg), cx));
                            format!("Error fetching instance: {msg}")
                        }
                    };
                }
                cx.notify();
            },
        ));
        cx.notify();
    }

    /// `ns` is the table's namespace cell (`"-"` for cluster-scoped rows); it
    /// identifies the row locally, while the API call uses the CRD scope.
    fn request_delete(&mut self, name: String, ns: String, cx: &mut Context<Self>) {
        let Some(crd) = self.selected.clone() else { return };
        let this = cx.entity().downgrade();
        let app = self.app.clone();
        let kind = crd.kind.clone();
        let api_ns = instance_namespace(&crd.scope, Some(&ns));
        let message = if api_ns.is_empty() {
            format!("Are you sure you want to delete \"{name}\"? This action cannot be undone.")
        } else {
            format!("Are you sure you want to delete \"{name}\" in namespace \"{ns}\"? This action cannot be undone.")
        };
        self.app.update(cx, |a, cx| {
            a.confirm(
                ConfirmRequest {
                    title: format!("Delete {kind}"),
                    message,
                    confirm_label: "Delete".into(),
                    cancel_label: "Cancel".into(),
                    variant: ConfirmVariant::Danger,
                    on_confirm: Rc::new(move |_, cx| {
                        app.update(cx, |a, cx| {
                            if let Some(c) = &mut a.confirm {
                                c.confirm_label = "Deleting...".into();
                            }
                            cx.notify();
                        });
                        this.update(cx, |v, cx| {
                            v.deleting = true;
                            cx.notify();
                        })
                        .ok();
                        let (crd, name, ns, api_ns) = (crd.clone(), name.clone(), ns.clone(), api_ns.clone());
                        let core = backend::core(cx);
                        let (this, app) = (this.clone(), app.clone());
                        cx.spawn(async move |cx| {
                            let res = core
                                .delete_crd_instance(crd.group.clone(), crd.version.clone(), crd.plural.clone(), api_ns, name.clone())
                                .await;
                            cx.update(|cx| {
                                match res {
                                    Ok(()) => {
                                        toast(cx, "Resource deleted", ToastKind::Success);
                                        app.update(cx, |a, cx| a.close_confirm(cx));
                                        this.update(cx, |v, cx| {
                                            v.instances.retain(|i| {
                                                meta_str(i, "name").unwrap_or_else(|| "-".into()) != name
                                                    || meta_str(i, "namespace").unwrap_or_else(|| "-".into()) != ns
                                            });
                                            if v.detail.as_ref().map(|d| d.name == name).unwrap_or(false) {
                                                v.detail = None;
                                            }
                                            cx.notify();
                                        })
                                        .ok();
                                    }
                                    Err(e) => {
                                        // The dialog stays open on failure (delete target kept).
                                        let msg = backend::format_error(&e);
                                        this.update(cx, |v, cx| v.report_error(Some(msg.clone()), cx)).ok();
                                        toast(cx, msg, ToastKind::Error);
                                        app.update(cx, |a, cx| {
                                            if let Some(c) = &mut a.confirm {
                                                c.confirm_label = "Delete".into();
                                            }
                                            cx.notify();
                                        });
                                    }
                                }
                                this.update(cx, |v, cx| {
                                    v.deleting = false;
                                    cx.notify();
                                })
                                .ok();
                            });
                        })
                        .detach();
                    }),
                },
                cx,
            )
        });
    }

    /// Groups sorted by name; items by kind (filtered by the search).
    fn grouped(&self) -> Vec<(String, Vec<CrdInfo>)> {
        let q = self.query.to_lowercase();
        let mut groups: Vec<(String, Vec<CrdInfo>)> = Vec::new();
        for c in &self.crds {
            if !q.is_empty()
                && !(c.kind.to_lowercase().contains(&q) || c.group.to_lowercase().contains(&q) || c.name.to_lowercase().contains(&q))
            {
                continue;
            }
            match groups.iter_mut().find(|(g, _)| *g == c.group) {
                Some((_, v)) => v.push(c.clone()),
                None => groups.push((c.group.clone(), vec![c.clone()])),
            }
        }
        groups.sort_by_key(|a| a.0.to_lowercase());
        for (_, items) in &mut groups {
            items.sort_by_key(|a| a.kind.to_lowercase());
        }
        groups
    }
}

fn kbd_esc() -> Div {
    // <kbd> inside the text-sm Back button: inherits line-height 20px.
    div()
        .flex_none()
        .ml(px(6.))
        .px(px(6.))
        .py(px(2.))
        .bg(theme::hexa(0x1e293b, 0.8))
        .rounded(px(4.))
        .border_1()
        .border_color(theme::hexa(0x334155, 0.5))
        .text_10()
        .line_height(px(20.))
        .font_mono()
        .text_color(slate::S400)
        .child("Esc")
}

fn with_scrollbar(id: &'static str, handle: &ScrollHandle, gutter: &scrollbar::Gutter, inner: Stateful<Div>) -> Div {
    let g = gutter.overflows();
    div()
        .flex_1()
        .min_h_0()
        .relative()
        .flex()
        .flex_row()
        .child(inner.id(id).flex_1().min_w_0().h_full().overflow_y_scroll().track_scroll(handle))
        .when(g, |d| d.child(div().w(px(scrollbar::WIDTH)).flex_none()))
        .child(scrollbar::vertical(handle.clone(), gutter.clone()))
}

impl CrdBrowserView {
    fn render_left(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let accent = theme::accent(cx);
        let focused = self.search.read(cx).is_focused(window);
        let search_box = div()
            .p(px(12.))
            .border_b_1()
            .border_color(theme::hexa(0x1e293b, 0.5))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .px(px(10.))
                    .py(px(6.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(slate::S800)
                    .bg(theme::hexa(0x0b1221, 0.5))
                    .child(icon("search", 14.).text_color(slate::S500))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .when(focused, |d| {
                                d.rounded(px(4.)).shadow(vec![BoxShadow {
                                    color: theme::Theme::get(cx).focus_ring().into(),
                                    offset: point(px(0.), px(0.)),
                                    blur_radius: px(0.),
                                    spread_radius: px(2.),
                                    inset: false,
                                }])
                            })
                            .child(self.search.clone()),
                    )
                    .auto_id("crd.search", "textbox", "Search CRDs..."),
            );

        let body: AnyElement = if self.crds_loading {
            div()
                .p(px(12.))
                .flex()
                .flex_col()
                .gap(px(12.))
                .children((0..3).map(|g| {
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(skeleton_bar(SharedString::from(format!("crd-sk-{g}")), px(128.).into(), 12.))
                        .child(
                            div()
                                .ml(px(8.))
                                .flex()
                                .flex_col()
                                .gap(px(6.))
                                .children([160., 144., 176.].into_iter().enumerate().map(move |(i, w)| {
                                    skeleton_bar(SharedString::from(format!("crd-sk-{g}-{i}")), px(w).into(), 12.)
                                })),
                        )
                }))
                .into_any_element()
        } else if let Some(err) = &self.crds_error {
            div().p(px(16.)).text_sm_tw().text_color(red::R400).child(err.clone()).into_any_element()
        } else {
            let groups = self.grouped();
            if groups.is_empty() {
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(8.))
                    .p(px(16.))
                    .text_color(slate::S400)
                    .child(icon("inbox", 40.).text_color(slate::S600))
                    .child(div().text_xs_tw().child(if self.query.is_empty() { "No CRDs found" } else { "No CRDs match your search" }))
                    .into_any_element()
            } else {
                let sel = self.selected.clone();
                let list = div().py(px(4.)).children(groups.into_iter().map(|(group, items)| {
                    let collapsed = self.collapsed.contains(&group);
                    let g2 = group.clone();
                    let header = div()
                        .id(SharedString::from(format!("crd-group-{group}")))
                        .w_full()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(6.))
                        .px(px(12.))
                        .py(px(8.))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::hexa(0x1d2939, 0.2)))
                        .child(icon(if collapsed { "chevron-right" } else { "chevron-down" }, 12.).text_color(slate::S500))
                        .child(
                            div()
                                .min_w_0()
                                .overflow_hidden()
                                .text_10()
                                .line_height(px(15.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(slate::S500)
                                .child(caps(&group, 10.)),
                        )
                        .child(div().ml_auto().flex_none().text_10().line_height(px(15.)).text_color(slate::S600).child(items.len().to_string()))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.collapsed.remove(&g2) {
                                this.collapsed.insert(g2.clone());
                            }
                            cx.notify();
                        }))
                        .auto_id(format!("crd.group.{group}"), "button", group.clone());
                    let rows = (!collapsed).then(|| {
                        div()
                            .children(items.into_iter().map(|crd| {
                                let selected =
                                    sel.as_ref().map(|s| s.name == crd.name && s.group == crd.group).unwrap_or(false);
                                let gid: SharedString = format!("crd-item-{}", crd.name).into();
                                let c2 = crd.clone();
                                let color = if selected { accent } else { slate::S300 };
                                div()
                                    .id(gid.clone())
                                    .group(gid.clone())
                                    .w_full()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(8.))
                                    .py(px(6.))
                                    .pl(px(28.))
                                    .pr(px(12.))
                                    .text_xs_tw()
                                    .cursor_pointer()
                                    .text_color(color)
                                    .when(selected, |d| d.border_l_2().border_color(accent))
                                    .when(!selected, |d| d.hover(|s| s.bg(theme::hexa(0x1d2939, 0.2)).text_color(slate::S100)))
                                    .child(
                                        icon("puzzle", 12.)
                                            .text_color(color)
                                            .when(!selected, |i| i.group_hover(gid.clone(), |s| s.text_color(slate::S100))),
                                    )
                                    .child(div().min_w_0().truncate().font_mono().child(crd.kind.clone()))
                                    .when(crd.scope == "Cluster", |d| {
                                        d.child(
                                            div()
                                                .flex_none()
                                                .text_9()
                                                .line_height(px(13.5))
                                                .bg(theme::hexa(0x334155, 0.5))
                                                .text_color(slate::S400)
                                                .px(px(4.))
                                                .py(px(2.))
                                                .rounded(px(4.))
                                                .child("cluster"),
                                        )
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| this.select_crd(Some(c2.clone()), cx)))
                                    .auto_id(format!("crd.item.{}", crd.name), "button", crd.kind.clone())
                                    .selected(selected)
                            }))
                            .with_animation(
                                SharedString::from(format!("crd-group-anim-{group}")),
                                Animation::new(Duration::from_millis(150)),
                                |el, t| el.opacity(t),
                            )
                    });
                    div().child(header).children(rows)
                }));
                list.into_any_element()
            }
        };

        div()
            .w(px(288.))
            .flex_none()
            .h_full()
            .border_r_1()
            .border_color(slate::S800)
            .bg(theme::hexa(0x101828, 0.3))
            .flex()
            .flex_col()
            .child(search_box)
            .child(with_scrollbar("crd-list", &self.list_scroll, &self.list_gutter, div().id("x").child(body)))
            .into_any_element()
    }

    fn render_table(&mut self, crd: &CrdInfo, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        // Auto table layout: max-content widths, extra width distributed in
        // proportion to them; the actions column is fixed at 64px.
        let rows: Vec<(String, String, String, Value)> = self
            .instances
            .iter()
            .map(|i| {
                (
                    meta_str(i, "name").unwrap_or_else(|| "-".into()),
                    meta_str(i, "namespace").unwrap_or_else(|| "-".into()),
                    format_relative_age(i.get("metadata").and_then(|m| m.get("creationTimestamp")).and_then(|t| t.as_str())),
                    i.clone(),
                )
            })
            .collect();
        let head = |t: &str, window: &mut Window| measure_tracked(window, &t.to_uppercase(), SANS, 12., FontWeight::MEDIUM, 0.05);
        let mono_w = |s: &str| s.chars().count() as f32 * 12. * theme::MONO_ADVANCE_EM;
        let mut w = [head("Name", window), head("Namespace", window), head("Age", window)];
        for (n, ns, age, _) in &rows {
            w[0] = w[0].max(mono_w(n));
            w[1] = w[1].max(mono_w(ns));
            w[2] = w[2].max(mono_w(age));
        }
        let w = w.map(|x| x + 24.);
        let viewport = f32::from(window.viewport_size().width);
        // main = window - sidebar - 32 (host insets); right panel = main - 288 - 1; table = panel - 32 - 2 (border).
        let gutter = self.table_gutter.overflows();
        let avail = viewport - crate::views::sidebar::WIDTH - 32. - 289. - 32. - 2. - if gutter { scrollbar::WIDTH } else { 0. };
        let sum: f32 = w.iter().sum();
        let extra = (avail - 64. - sum).max(0.);
        let widths = w.map(|x| x + extra * x / sum);
        let deleting = self.deleting;

        let th = |t: &'static str, width: f32| {
            div()
                .w(px(width))
                .flex_none()
                .px(px(12.))
                .py(px(8.))
                .text_xs_tw()
                .font_weight(FontWeight::MEDIUM)
                .text_color(slate::S400)
                .child(caps(t, 12.))
        };
        let header = div()
            .flex()
            .flex_row()
            .bg(theme::hexa(0x101828, 0.8))
            .border_b_1()
            .border_color(slate::S800)
            .child(th("Name", widths[0]))
            .child(th("Namespace", widths[1]))
            .child(th("Age", widths[2]))
            .child(div().w(px(64.)).flex_none());
        let n = rows.len();
        let body = rows.into_iter().enumerate().map(|(ix, (name, ns, age, inst))| {
            let td = |t: String, width: f32| {
                div().w(px(width)).flex_none().px(px(12.)).font_mono().text_xs_tw().text_color(slate::S100).child(t)
            };
            let (dn, dns) = (name.clone(), ns.clone());
            div()
                .id(SharedString::from(format!("crd-row-{ix}")))
                .flex()
                .flex_row()
                .items_center()
                .h(px(39.))
                .when(ix + 1 < n, |d| d.border_b_1().border_color(theme::hexa(0x1e293b, 0.5)).h(px(40.)))
                .cursor_pointer()
                .hover(|s| s.bg(theme::hexa(0x1d2939, 0.3)))
                .child(td(name.clone(), widths[0]))
                .child(td(ns.clone(), widths[1]))
                .child(td(age, widths[2]))
                .child(
                    div().w(px(64.)).flex_none().px(px(12.)).flex().justify_end().child(
                        div()
                            .id(SharedString::from(format!("crd-del-{ix}")))
                            .group("crd-del")
                            .p(px(4.))
                            .rounded(px(4.))
                            .when(deleting, |d| d.opacity(0.5))
                            .when(!deleting, |d| d.cursor_pointer().hover(|s| s.bg(theme::hexa(0xef4444, 0.1))))
                            .child(
                                icon("trash-2", 14.)
                                    .text_color(slate::S500)
                                    .when(!deleting, |i| i.group_hover("crd-del", |s| s.text_color(red::R400))),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                if !this.deleting {
                                    this.request_delete(dn.clone(), dns.clone(), cx);
                                }
                            }))
                            .auto_id(format!("crd.delete.{ns}/{name}"), "button", "Delete"),
                    ),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.open_instance(&inst, cx)))
                .auto_id(format!("crd.row.{ns}/{name}"), "row", name.clone())
        });

        let content: AnyElement = if self.inst_loading {
            div()
                .p(px(16.))
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(skeleton_bar("crd-tsk-h", px(192.).into(), 16.))
                .child(div().flex().flex_col().gap(px(8.)).children((0..5).map(|i| {
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(16.))
                        .child(skeleton_bar(SharedString::from(format!("crd-tsk-{i}-a")), px(160.).into(), 12.))
                        .child(skeleton_bar(SharedString::from(format!("crd-tsk-{i}-b")), px(96.).into(), 12.))
                        .child(skeleton_bar(SharedString::from(format!("crd-tsk-{i}-c")), px(64.).into(), 12.))
                })))
                .into_any_element()
        } else if let Some(err) = &self.inst_error {
            div().p(px(16.)).text_sm_tw().text_color(red::R400).child(err.clone()).into_any_element()
        } else if self.instances.is_empty() {
            let msg = if self.namespace.is_some() {
                format!("No {} resources in this namespace", crd.kind)
            } else {
                format!("No {} resources found", crd.kind)
            };
            div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(12.))
                .text_color(slate::S400)
                .child(icon("inbox", 40.).text_color(slate::S600))
                .child(div().text_xs_tw().child("No instances found"))
                .child(div().text_10().text_color(slate::S500).child(msg))
                .into_any_element()
        } else {
            div()
                .m(px(16.))
                .glass()
                .rounded(px(8.))
                .overflow_hidden()
                .text_sm_tw()
                .child(header)
                .children(body)
                .auto_id("crd.table", "table", "instances")
                .into_any_element()
        };

        let count = self.instances.len();
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .p(px(12.))
                    .border_b_1()
                    .border_color(slate::S800)
                    .bg(theme::hexa(0x101828, 0.3))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(12.))
                    .child(icon("puzzle", 16.).text_color(theme::accent(cx)))
                    .child(
                        div()
                            .min_w_0()
                            .child(div().text_sm_tw().font_weight(FontWeight::SEMIBOLD).text_color(slate::S100).truncate().child(crd.kind.clone()))
                            .child(
                                div()
                                    .text_10()
                                    .line_height(px(15.))
                                    .text_color(slate::S500)
                                    .truncate()
                                    .child(format!("{}/{} - {}", crd.group, crd.version, crd.scope))
                                    .auto_id("crd.scope", "text", format!("{}/{} - {}", crd.group, crd.version, crd.scope)),
                            ),
                    )
                    .child(
                        div()
                            .ml_auto()
                            .flex_none()
                            .text_10()
                            .line_height(px(15.))
                            .text_color(theme::accent(cx))
                            .px(px(6.))
                            .py(px(2.))
                            .rounded_full()
                            .font_weight(FontWeight::MEDIUM)
                            .child(count.to_string())
                            .auto_id("crd.count", "text", count.to_string()),
                    ),
            )
            .child(with_scrollbar("crd-table-scroll", &self.table_scroll, &self.table_gutter, div().id("x").flex().flex_col().child(content)))
            .with_animation(SharedString::from(format!("crd-table-{}", crd.name)), Animation::new(Duration::from_millis(300)), |el, t| {
                el.opacity(t)
            })
            .into_any_element()
    }

    fn render_detail(&mut self, d: &Detail, cx: &mut Context<Self>) -> AnyElement {
        let accent = theme::accent(cx);
        let content: AnyElement = if d.loading {
            let bar = |id: &str, w: f32, h: f32| skeleton_bar(SharedString::from(id.to_string()), px(w).into(), h);
            div()
                .p(px(16.))
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(bar("crd-dsk-1", 128., 16.))
                .child(div().ml(px(16.)).flex().flex_col().gap(px(8.)).child(bar("crd-dsk-2", 192., 12.)).child(bar("crd-dsk-3", 224., 12.)).child(bar("crd-dsk-4", 160., 12.)))
                .child(div().mt(px(16.)).child(bar("crd-dsk-5", 112., 16.)))
                .child(div().ml(px(16.)).flex().flex_col().gap(px(8.)).child(bar("crd-dsk-6", 256., 12.)).child(bar("crd-dsk-7", 208., 12.)))
                .into_any_element()
        } else {
            let src = if d.json.is_empty() { "No details available".to_string() } else { d.json.clone() };
            let (text, ranges) = highlight_json(&src);
            let hl: Vec<(std::ops::Range<usize>, Rgba)> = ranges
                .into_iter()
                .map(|(r, c)| {
                    let color = match c {
                        JsonColor::Accent => accent,
                        JsonColor::Emerald => theme::emerald::E400,
                        JsonColor::Amber => theme::amber::A400,
                        JsonColor::Indigo => theme::hex(0x818cf8),
                        JsonColor::Slate => slate::S500,
                    };
                    (r, color)
                })
                .collect();
            div()
                .p(px(16.))
                .font_mono()
                .text_xs_tw()
                .line_height(px(19.)) // 12px leading-relaxed mono lines are 19px apart
                .text_color(slate::S100)
                .child(mono_paragraph(text).colors(hl).line_height(px(19.)))
                .auto_id("crd.detail.json", "text", src)
                .into_any_element()
        };
        let g = self.detail_gutter.overflows();
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .p(px(12.))
                    .border_b_1()
                    .border_color(slate::S800)
                    .bg(theme::hexa(0x101828, 0.3))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .id("crd-back-to-list")
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(6.))
                            .px(px(10.))
                            .py(px(4.))
                            .rounded(px(6.))
                            .border_1()
                            .border_color(slate::S800)
                            .text_xs_tw()
                            .text_color(slate::S300)
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::hexa(0x1d2939, 0.3)))
                            .child(icon("arrow-left", 14.).text_color(slate::S300))
                            .child("Back to list")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.detail = None;
                                this.detail_task = None;
                                cx.notify();
                            }))
                            .auto_id("crd.back-to-list", "button", "Back to list"),
                    )
                    .child(div().text_xs_tw().text_color(slate::S500).child("|"))
                    .child(div().text_xs_tw().font_mono().text_color(accent).child(d.name.clone())),
            )
            .child(
                div()
                    .m(px(16.))
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .flex()
                    .flex_row()
                    .bg(theme::hexa(0x101828, 0.2))
                    .border_1()
                    .border_color(slate::S800)
                    .rounded(px(8.))
                    .overflow_hidden()
                    .child(div().id("crd-detail-scroll").flex_1().min_w_0().h_full().overflow_y_scroll().track_scroll(&self.detail_scroll).child(content))
                    .when(g, |d| d.child(div().w(px(scrollbar::WIDTH)).flex_none()))
                    .child(scrollbar::vertical(self.detail_scroll.clone(), self.detail_gutter.clone())),
            )
            .with_animation("crd-detail", Animation::new(Duration::from_millis(300)), |el, t| el.opacity(t))
            .into_any_element()
    }
}

impl Render for CrdBrowserView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        let n = self.crds.len();
        let header = div()
            .p(px(16.))
            .border_b_1()
            .border_color(slate::S800)
            .bg(theme::hexa(0x101828, 0.5))
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .child(
                div()
                    .id("crd-back")
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .px(px(12.))
                    .py(px(6.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(slate::S800)
                    .text_sm_tw()
                    .text_color(slate::S300)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::hexa(0x1d2939, 0.3)))
                    .child(icon("arrow-left", 16.).text_color(slate::S300))
                    .child("Back")
                    .child(kbd_esc())
                    .on_click(cx.listener(|this, _, _, cx| this.app.update(cx, |a, cx| a.set_view(AppView::Table, cx))))
                    .auto_id("crd.back", "button", "Back"),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .child(div().size(px(28.)).rounded(px(8.)).flex().items_center().justify_center().child(icon("puzzle", 16.).text_color(accent)))
                    .child(
                        div()
                            .child(div().text_sm_tw().font_weight(FontWeight::SEMIBOLD).text_color(slate::S100).child("Custom Resources"))
                            .child(
                                div()
                                    .text_10()
                                    .line_height(px(15.))
                                    .text_color(slate::S500)
                                    .child(format!("{n} CRD{} discovered", if n != 1 { "s" } else { "" })),
                            ),
                    ),
            )
            .child(div().w(px(80.)).flex_none());

        let right: AnyElement = match (self.selected.clone(), self.detail.clone()) {
            (None, _) => div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(12.))
                .text_color(slate::S400)
                .child(icon("puzzle", 48.).text_color(slate::S600))
                .child(div().text_sm_tw().font_weight(FontWeight::MEDIUM).child("Select a Custom Resource"))
                .child(div().text_xs_tw().text_color(slate::S500).child("Choose a CRD from the left panel to view its instances"))
                .into_any_element(),
            (Some(_), Some(d)) => self.render_detail(&d, cx),
            (Some(crd), None) => self.render_table(&crd, window, cx),
        };

        let left = self.render_left(window, cx);
        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(theme::BACKGROUND)
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .child(left)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .when(self.forbidden.read(cx).visible(), |d| d.child(div().flex_none().px(px(16.)).pt(px(16.)).child(self.forbidden.clone())))
                            .child(right),
                    ),
            )
            .with_animation("crd-enter", Animation::new(Duration::from_millis(300)), |el, t| {
                el.opacity(t).left(px(20. * (1. - t)))
            })
            .auto_id("view.crds", "region", "Custom Resources")
    }
}

#[cfg(test)]
mod tests {
    use super::{highlight_json, JsonColor};

    #[test]
    fn json_highlight_chain() {
        let src = "{\n  \"a\": \"x:12\",\n  \"n\": 3,\n  \"b\": true,\n  \"z\": null,\n  \"l\": [1, \"s\"],\n  \"q\": \"a\\\"b\"\n}";
        let (text, ranges) = highlight_json(src);
        // The text is never rewritten (a ":" inside a string stays as is).
        assert_eq!(text, src);
        let colored: Vec<(&str, JsonColor)> = ranges.iter().map(|(r, c)| (&text[r.clone()], *c)).collect();
        assert_eq!(
            colored,
            vec![
                ("\"a\"", JsonColor::Accent),
                ("\"x:12\"", JsonColor::Emerald),
                ("\"n\"", JsonColor::Accent),
                ("3", JsonColor::Amber),
                ("\"b\"", JsonColor::Accent),
                ("true", JsonColor::Indigo),
                ("\"z\"", JsonColor::Accent),
                ("null", JsonColor::Slate),
                // Array items are not colored.
                ("\"l\"", JsonColor::Accent),
                ("\"q\"", JsonColor::Accent),
                ("\"a\\\"b\"", JsonColor::Emerald),
            ]
        );
    }

    #[test]
    fn plain_text_untouched() {
        let (t, r) = highlight_json("No details available");
        assert_eq!(t, "No details available");
        assert!(r.is_empty());
    }
}
