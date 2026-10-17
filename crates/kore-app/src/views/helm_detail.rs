//! Helm release detail (spec 03 §5): Values / Manifest / History / Notes tabs,
//! Copy, rollback with a warning confirm. `AppState::helm_release` holds the
//! release; Esc (AppEvent::Escape) returns to the list.

use crate::automation::Automation;
use crate::backend;
use crate::state::app_state::{ConfirmRequest, ConfirmVariant};
use crate::state::{AppEvent, AppState};
use crate::theme::{self, amber, emerald, slate, KoreStyled, SANS};
use crate::ui::icon::icon;
use crate::ui::scrollbar;
use crate::ui::text::{caps, measure, measure_tracked};
use crate::ui::toast::{toast, ToastKind};
use crate::ui::widgets::{kbd, skeleton_bar};
use crate::views::helm_releases::{format_helm_date, helm_status_badge};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use kore_core::state::helm::HelmRevision;
use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tab {
    Values,
    Manifest,
    History,
    Notes,
}

const TABS: [(Tab, &str, &str, &str); 4] = [
    (Tab::Values, "values", "Values", "file-text"),
    (Tab::Manifest, "manifest", "Manifest", "scroll-text"),
    (Tab::History, "history", "History", "history"),
    (Tab::Notes, "notes", "Notes", "sticky-note"),
];

/// YAML syntax-highlighting color classes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hl {
    Comment,
    Key,
    Bool,
    Number,
    Null,
    Str,
    Dash,
}

fn value_class(value: &str) -> Option<Hl> {
    let t = value.trim();
    if t.is_empty() {
        return None;
    }
    let lower = t.to_lowercase();
    if lower == "true" || lower == "false" {
        return Some(Hl::Bool);
    }
    let num = t.strip_prefix('-').unwrap_or(t);
    let mut parts = num.splitn(2, '.');
    let int = parts.next().unwrap_or("");
    let frac = parts.next();
    if !int.is_empty() && int.chars().all(|c| c.is_ascii_digit()) && frac.map(|f| !f.is_empty() && f.chars().all(|c| c.is_ascii_digit())).unwrap_or(true) {
        return Some(Hl::Number);
    }
    if lower == "null" || t == "~" {
        return Some(Hl::Null);
    }
    Some(Hl::Str)
}

/// Highlight spans (byte ranges within `line`) for one YAML line.
pub fn highlight_line(line: &str) -> Vec<(Range<usize>, Hl)> {
    let indent = line.len() - line.trim_start().len();
    let rest = &line[indent..];
    if rest.starts_with('#') {
        return vec![(0..line.len(), Hl::Comment)];
    }
    // ^(\s*)([\w.-]+)(\s*:\s*)(.*)
    let key_len = rest.find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.' || c == '-')).unwrap_or(rest.len());
    if key_len > 0 {
        let after = &rest[key_len..];
        let ws = after.len() - after.trim_start().len();
        if after[ws..].starts_with(':') {
            let after_colon = &after[ws + 1..];
            let ws2 = after_colon.len() - after_colon.trim_start().len();
            let vstart = indent + key_len + ws + 1 + ws2;
            let mut out = vec![(indent..indent + key_len, Hl::Key)];
            if let Some(c) = value_class(&line[vstart..]) {
                out.push((vstart..line.len(), c));
            }
            return out;
        }
    }
    // ^(\s*-\s+)(.*)
    if let Some(after) = rest.strip_prefix('-') {
        let ws = after.len() - after.trim_start().len();
        if ws > 0 {
            let vstart = indent + 1 + ws;
            let mut out = vec![(0..vstart, Hl::Dash)];
            if let Some(c) = value_class(&line[vstart..]) {
                out.push((vstart..line.len(), c));
            }
            return out;
        }
    }
    Vec::new()
}

fn hl_color(h: Hl, accent: Rgba) -> Rgba {
    match h {
        Hl::Comment | Hl::Null | Hl::Dash => slate::S500,
        Hl::Key => accent,
        Hl::Bool => theme::hex(0x818cf8),
        Hl::Number => amber::A400,
        Hl::Str => emerald::E400,
    }
}

fn highlighted_yaml(text: &str, accent: Rgba) -> StyledText {
    let mut highlights = Vec::new();
    let mut offset = 0;
    for line in text.split('\n') {
        for (r, h) in highlight_line(line) {
            if !r.is_empty() {
                highlights.push((
                    offset + r.start..offset + r.end,
                    HighlightStyle { color: Some(hl_color(h, accent).into()), ..Default::default() },
                ));
            }
        }
        offset += line.len() + 1;
    }
    StyledText::new(text.to_string()).with_highlights(highlights)
}

pub struct HelmDetailView {
    app: Entity<AppState>,
    name: String,
    namespace: String,
    tab: Tab,
    values: String,
    manifest: String,
    history: Vec<HelmRevision>,
    notes: String,
    loading: bool,
    rolling_back: bool,
    task: Option<Task<()>>,
    scroll: ScrollHandle,
    gutter: scrollbar::Gutter,
    _subs: Vec<Subscription>,
}

impl HelmDetailView {
    pub fn new(app: Entity<AppState>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (name, namespace) =
            app.read(cx).helm_release.as_ref().map(|r| (r.name.clone(), r.namespace.clone())).unwrap_or_default();
        let subs = vec![cx.subscribe(&app, |_, app, ev: &AppEvent, cx| {
            if matches!(ev, AppEvent::Escape) {
                app.update(cx, |s, cx| s.back(cx));
            }
        })];
        let mut this = HelmDetailView {
            app,
            name,
            namespace,
            tab: Tab::Values,
            values: String::new(),
            manifest: String::new(),
            history: Vec::new(),
            notes: String::new(),
            loading: true,
            rolling_back: false,
            task: None,
            scroll: ScrollHandle::new(),
            gutter: Default::default(),
            _subs: subs,
        };
        this.fetch(cx);
        this
    }

    fn fetch(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        let (r, ns) = (self.name.clone(), self.namespace.clone());
        self.task = Some(match self.tab {
            Tab::Values => backend::run(cx, move |c| async move { c.get_helm_values(r, ns).await }, |this: &mut Self, res, cx| {
                this.values = res.unwrap_or_else(|e| format!("Error fetching values: {}", backend::format_error(&e)));
                this.loading = false;
                cx.notify();
            }),
            Tab::Manifest => backend::run(cx, move |c| async move { c.get_helm_manifest(r, ns).await }, |this: &mut Self, res, cx| {
                this.manifest = res.unwrap_or_else(|e| format!("Error fetching manifest: {}", backend::format_error(&e)));
                this.loading = false;
                cx.notify();
            }),
            Tab::History => backend::run(cx, move |c| async move { c.get_helm_history(r, ns).await }, |this: &mut Self, res, cx| {
                match res {
                    Ok(h) => this.history = h,
                    Err(e) => {
                        this.history.clear();
                        toast(cx, backend::format_error(&e), ToastKind::Error);
                    }
                }
                this.loading = false;
                cx.notify();
            }),
            // `helm get notes` (quirk 10.8, fixed: this used to show the values).
            Tab::Notes => backend::run(cx, move |c| async move { c.get_helm_notes(r, ns).await }, |this: &mut Self, res, cx| {
                this.notes = match res {
                    Ok(v) if !v.trim().is_empty() => v,
                    Ok(_) => "No notes available for this release.".into(),
                    Err(e) => format!("Error fetching notes: {}", backend::format_error(&e)),
                };
                this.loading = false;
                cx.notify();
            }),
        });
        cx.notify();
    }

    fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        if tab != self.tab {
            self.tab = tab;
            self.scroll = ScrollHandle::new();
            self.fetch(cx);
        }
    }

    fn request_rollback(&mut self, revision: String, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        let app = self.app.clone();
        let (name, ns) = (self.name.clone(), self.namespace.clone());
        let message = format!(
            "Are you sure you want to rollback \"{name}\" in namespace \"{ns}\" to revision {revision}? This will create a new revision."
        );
        self.app.update(cx, |s, cx| {
            s.confirm(
                ConfirmRequest {
                    title: "Rollback Helm Release".into(),
                    message,
                    confirm_label: "Rollback".into(),
                    cancel_label: "Cancel".into(),
                    variant: ConfirmVariant::Warning,
                    on_confirm: Rc::new(move |_, cx| {
                        let (name, ns, rev) = (name.clone(), ns.clone(), revision.clone());
                        let this = this.clone();
                        let app = app.clone();
                        this.update(cx, |v, cx| {
                            v.rolling_back = true;
                            cx.notify();
                        })
                        .ok();
                        let core = backend::core(cx);
                        cx.spawn(async move |cx| {
                            let res = core.rollback_helm_release(name.clone(), ns.clone(), rev.clone()).await;
                            let ok = res.is_ok();
                            cx.update(|cx| {
                                match res {
                                    Ok(out) => {
                                        let msg = if out.is_empty() { format!("Rolled back to revision {rev}") } else { out };
                                        toast(cx, msg, ToastKind::Success);
                                        app.update(cx, |s, cx| s.close_confirm(cx));
                                    }
                                    Err(e) => toast(cx, backend::format_error(&e), ToastKind::Error),
                                }
                                this.update(cx, |v, cx| {
                                    v.rolling_back = false;
                                    if ok && v.tab == Tab::History {
                                        v.fetch(cx);
                                    }
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

    fn copy(&self, cx: &mut Context<Self>) {
        let text = if self.tab == Tab::Values { self.values.clone() } else { self.manifest.clone() };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        toast(cx, "Copied to clipboard", ToastKind::Success);
    }

    fn button(id: &'static str) -> Stateful<Div> {
        div()
            .id(id)
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .px(px(12.))
            .py(px(6.))
            .rounded(px(6.))
            .border_1()
            .border_color(slate::S800)
            .hover(|s| s.bg(theme::hexa(0x1d2939, 0.3)))
            .text_sm_tw()
            .text_color(slate::S300)
            .cursor_pointer()
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let show_copy = matches!(self.tab, Tab::Values | Tab::Manifest);
        let field = |label: &str, value: &str| {
            div()
                .min_w_0()
                .child(div().text_10().line_height(px(20.)).text_color(slate::S500).mb(px(4.)).child(caps(label, 10.)))
                .child(div().text_xs_tw().font_mono().text_color(slate::S100).child(value.to_string()))
        };
        div()
            .flex_none()
            .p(px(16.))
            .border_b_1()
            .border_color(slate::S800)
            .bg(theme::hexa(0x101828, 0.5))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .mb(px(16.))
                    .child(
                        Self::button("helm-back")
                            .child(icon("arrow-left", 16.).text_color(slate::S300))
                            .child("Back")
                            .child(kbd("Esc").line_height(px(20.)).ml(px(6.)))
                            .on_click(cx.listener(|this, _, _, cx| this.app.update(cx, |s, cx| s.back(cx))))
                            .auto_id("helm-detail.back", "button", "Go back to Helm releases list"),
                    )
                    .child(div().flex().flex_row().items_center().gap(px(8.)).when(show_copy, |d| {
                        d.child(
                            Self::button("helm-copy")
                                .child(icon("copy", 16.).text_color(slate::S300))
                                .child("Copy")
                                .on_click(cx.listener(|this, _, _, cx| this.copy(cx)))
                                .auto_id("helm-detail.copy", "button", "Copy"),
                        )
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(16.))
                    .text_sm_tw()
                    .child(field("Release", &self.name).flex_1())
                    .child(field("Namespace", &self.namespace).flex_1())
                    .child(div().flex_1())
                    .child(div().flex_1()),
            )
    }

    fn render_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        div().flex().flex_row().flex_none().border_b_1().border_color(slate::S800).children(TABS.iter().map(|(t, id, label, ic)| {
            let t = *t;
            let active = t == self.tab;
            let color = if active { accent } else { slate::S400 };
            let group: SharedString = format!("helm-tab-{id}").into();
            div()
                .id(SharedString::from(format!("helm-tab-{id}")))
                .group(group.clone())
                .relative()
                .px(px(16.))
                .py(px(10.))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.))
                .text_sm_tw()
                .text_color(color)
                .when(!active, |d| d.hover(|s| s.text_color(slate::S200)))
                .cursor_pointer()
                .child(icon(ic, 14.).text_color(color).when(!active, |s| s.group_hover(group.clone(), |s| s.text_color(slate::S200))))
                .child(*label)
                .when(active, |d| d.child(div().absolute().bottom_0().left_0().right_0().h(px(2.)).rounded_full().bg(accent)))
                .on_click(cx.listener(move |this, _, _, cx| this.set_tab(t, cx)))
                .auto_id(format!("tab.{id}"), "tab", *label)
                .selected(active)
        }))
    }

    fn content_skeleton() -> impl IntoElement {
        const W: [f32; 10] = [80., 65., 90., 45., 75., 55., 85., 40., 70., 60.];
        div().p(px(16.)).flex().flex_col().gap(px(10.)).children(
            W.iter().enumerate().map(|(i, w)| div().opacity(1. - i as f32 * 0.06).child(skeleton_bar(("helm-csk", i), relative(w / 100.), 12.))),
        )
    }

    fn render_text_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let accent = theme::accent(cx);
        let body: AnyElement = if self.loading {
            Self::content_skeleton().into_any_element()
        } else {
            let inner = div().p(px(16.)).font_mono().text_xs_tw().line_height(px(19.));
            match self.tab {
                Tab::Values => {
                    let t = if self.values.is_empty() { "# No values configured" } else { &self.values };
                    inner.child(highlighted_yaml(t, accent)).into_any_element()
                }
                Tab::Manifest => {
                    let t = if self.manifest.is_empty() { "# No manifest available" } else { &self.manifest };
                    inner.child(highlighted_yaml(t, accent)).into_any_element()
                }
                _ => inner.text_color(slate::S300).child(self.notes.clone()).into_any_element(),
            }
        };
        let gutter = self.gutter.overflows();
        // Fills the content area inside its 16px inset (the wrapper pads), so the
        // bottom border is visible (quirk 10.8, fixed: was 100% + 32px margins).
        div()
            .flex_1()
            .min_h_0()
            .relative()
            .rounded(px(8.))
            .border_1()
            .border_color(slate::S800)
            .bg(theme::hexa(0x101828, 0.3))
            .overflow_hidden()
            .child(
                div()
                    .id("helm-content-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .when(gutter, |d| d.pr(px(scrollbar::WIDTH)))
                    .child(body),
            )
            .child(scrollbar::vertical(self.scroll.clone(), self.gutter.clone()))
            .auto_id(
                format!("helm-detail.{}", if self.tab == Tab::Notes { "notes" } else if self.tab == Tab::Values { "values" } else { "manifest" }),
                "region",
                // The shown text (the skeleton while loading).
                if self.loading {
                    String::new()
                } else {
                    match self.tab {
                        Tab::Values if self.values.is_empty() => "# No values configured".to_string(),
                        Tab::Values => self.values.clone(),
                        Tab::Manifest if self.manifest.is_empty() => "# No manifest available".to_string(),
                        Tab::Manifest => self.manifest.clone(),
                        _ => self.notes.clone(),
                    }
                },
            )
            .into_any_element()
    }

    fn history_widths(&self, window: &mut Window, avail: f32) -> Vec<f32> {
        let adv = 12. * theme::MONO_ADVANCE_EM;
        let mono = |s: &str| adv * s.chars().count() as f32;
        let heads = ["Revision", "Updated", "Status", "Chart", "App Version", "Description", "Actions"];
        let last = self.history.len().saturating_sub(1);
        let pill = 6. + 6. + measure(window, "latest", SANS, 10., FontWeight::MEDIUM) + 6.;
        let rollback_btn = 2. + 10. + 12. + 6. + measure(window, "Rollback", SANS, 12., FontWeight::NORMAL) + 10.;
        let mut w: Vec<f32> = heads
            .iter()
            .enumerate()
            .map(|(i, h)| {
                let head = measure_tracked(window, &h.to_uppercase(), SANS, 12., FontWeight::MEDIUM, 0.05);
                let content = self
                    .history
                    .iter()
                    .enumerate()
                    .map(|(ri, r)| match i {
                        0 => mono(&r.revision) + if ri == last { pill } else { 0. },
                        1 => measure(window, &format_helm_date(&r.updated), SANS, 12., FontWeight::NORMAL),
                        2 => 8. + 6. + 6. + measure(window, &r.status, SANS, 12., FontWeight::MEDIUM) + 8.,
                        3 => mono(&r.chart),
                        4 => mono(if r.app_version.is_empty() { "-" } else { &r.app_version }),
                        5 => measure(window, if r.description.is_empty() { "-" } else { &r.description }, SANS, 12., FontWeight::NORMAL).min(200. - 24.),
                        _ => {
                            if r.status.eq_ignore_ascii_case("deployed") {
                                0.
                            } else {
                                rollback_btn
                            }
                        }
                    })
                    .fold(0., f32::max);
                head.max(content) + 24.
            })
            .collect();
        let total: f32 = w.iter().sum();
        if total > 0. && total < avail {
            let f = avail / total;
            for x in w.iter_mut() {
                *x *= f;
            }
        }
        w
    }

    fn render_history(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let accent = theme::accent(cx);
        let outer = div().id("helm-history").flex_1().min_h_0().overflow_y_scroll();
        if self.loading {
            return outer
                .child(
                    div().rounded(px(8.)).glass().child(
                        div()
                            .p(px(16.))
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .child(skeleton_bar("helm-hsk-top", relative(1.), 16.))
                            .children((0..5).map(|i| {
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap(px(16.))
                                    .opacity(1. - i as f32 * 0.1)
                                    .children([48., 128., 80., 96.].iter().enumerate().map(move |(j, w)| skeleton_bar(("helm-hsk", i * 10 + j), px(*w).into(), 16.)))
                                    .child(div().flex_1().child(skeleton_bar(("helm-hsk-f", i), relative(1.), 16.)))
                            })),
                    ),
                )
                .into_any_element();
        }
        if self.history.is_empty() {
            return outer
                .child(
                    div()
                        .size_full()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(px(12.))
                        .text_color(slate::S400)
                        .child(icon("history", 48.).text_color(slate::S600))
                        .child(div().text_sm_tw().font_weight(FontWeight::MEDIUM).child("No revision history found"))
                        .auto_id("helm-detail.history.empty", "status", "No revision history found"),
                )
                .into_any_element();
        }
        let avail = f32::from(window.viewport_size().width) - crate::views::sidebar::WIDTH - 32. - 32. - 2.;
        let w = self.history_widths(window, avail);
        let heads = ["Revision", "Updated", "Status", "Chart", "App Version", "Description", "Actions"];
        let last = self.history.len() - 1;
        let header = div()
            .flex()
            .flex_row()
            .bg(theme::hexa(0x101828, 0.8))
            .border_b_1()
            .border_color(slate::S800)
            .children(heads.iter().enumerate().map(|(i, h)| {
                div()
                    .w(px(w[i]))
                    .flex_none()
                    .px(px(12.))
                    .py(px(8.))
                    .flex()
                    .flex_row()
                    .when(i == 6, |d| d.justify_end())
                    .text_xs_tw()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(slate::S400)
                    .child(caps(h, 12.))
            }));
        let rolling = self.rolling_back;
        let rows = self.history.iter().enumerate().map(|(ix, r)| {
            let deployed = r.status.eq_ignore_ascii_case("deployed");
            let cell = |i: usize| div().w(px(w[i])).flex_none().px(px(12.)).flex().flex_row().items_center().overflow_hidden().whitespace_nowrap();
            let rev = r.revision.clone();
            div()
                .flex()
                .flex_col()
                .child(div().h(px(1.)).when(ix > 0, |d| d.bg(theme::hexa(0x1e293b, 0.5))))
                .child(
                    div()
                        .id(("helm-hrow", ix))
                        .h(px(HIST_ROW_H))
                        .flex()
                        .flex_row()
                        .text_xs_tw()
                        .map(|d| if deployed { d.bg(theme::hexa(0x10b981, 0.05)) } else { d.hover(|s| s.bg(theme::hexa(0x1d2939, 0.3))) })
                        .child(
                            cell(0).font_mono().text_color(slate::S100).gap(px(6.)).child(r.revision.clone()).when(ix == last, |d| {
                                d.child(
                                    div()
                                        .px(px(6.))
                                        .py(px(2.))
                                        .rounded(px(4.))
                                        .text_10()
                                        .line_height(px(12.))
                                        .font_sans()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(accent)
                                        .child("latest")
                                        .auto_id("helm-detail.latest", "status", format!("latest {}", r.revision)),
                                )
                            }),
                        )
                        .child(
                            cell(1)
                                .text_color(slate::S300)
                                .child(format_helm_date(&r.updated))
                                .auto_id(format!("helm-detail.updated.{}", r.revision), "cell", format_helm_date(&r.updated)),
                        )
                        .child(cell(2).child(helm_status_badge(("helm-hping", ix), &r.status)))
                        .child(cell(3).font_mono().text_color(slate::S300).child(r.chart.clone()))
                        .child(cell(4).font_mono().text_color(slate::S400).child(if r.app_version.is_empty() { "-".to_string() } else { r.app_version.clone() }))
                        .child(cell(5).text_color(slate::S400).child(div().max_w(px(200. - 24.)).truncate().child(if r.description.is_empty() { "-".to_string() } else { r.description.clone() })))
                        .child(cell(6).justify_end().when(!deployed, |d| {
                            d.child(
                                div()
                                    .id(("helm-rollback", ix))
                                    .group("helm-rb")
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(6.))
                                    .px(px(10.))
                                    .py(px(4.))
                                    .rounded(px(6.))
                                    .border_1()
                                    .border_color(slate::S700)
                                    .text_xs_tw()
                                    .text_color(slate::S300)
                                    .map(|d| {
                                        if rolling {
                                            d.opacity(0.5)
                                        } else {
                                            d.cursor_pointer().hover(|s| {
                                                s.border_color(theme::hexa(0xf59e0b, 0.5)).bg(theme::hexa(0xf59e0b, 0.1)).text_color(amber::A400)
                                            })
                                        }
                                    })
                                    .child(icon("rotate-ccw", 12.).text_color(slate::S300).when(!rolling, |s| s.group_hover("helm-rb", |s| s.text_color(amber::A400))))
                                    .child("Rollback")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if !this.rolling_back {
                                            this.request_rollback(rev.clone(), cx);
                                        }
                                    }))
                                    .auto_id(format!("helm-detail.rollback.{}", r.revision), "button", "Rollback")
                                    .disabled(rolling),
                            )
                        })),
                )
                .auto_id(format!("helm-detail.row.{}", r.revision), "row", format!("{} {}", r.status, r.description))
        });
        outer
            .child(div().rounded(px(8.)).glass().overflow_hidden().child(header).children(rows).auto_id("helm-detail.history", "table", "History"))
            .into_any_element()
    }
}

pub const HIST_ROW_H: f32 = 36.;

impl Render for HelmDetailView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = if self.tab == Tab::History { self.render_history(window, cx) } else { self.render_text_panel(cx) };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::BACKGROUND)
            .child(self.render_header(cx))
            .child(self.render_tabs(cx))
            .child(div().flex_1().min_h_0().overflow_hidden().p(px(16.)).flex().flex_col().child(content))
            .with_animation("helm-detail-in", Animation::new(Duration::from_millis(300)).with_easing(ease_out_quint()), |el, t| {
                el.opacity(t).ml(px(20. * (1. - t)))
            })
            .auto_id("view.helm-detail", "region", "Helm Release")
    }
}

#[cfg(test)]
mod tests {
    use super::{highlight_line, Hl};

    #[test]
    fn yaml_highlight_classes() {
        assert_eq!(highlight_line("# Source: x"), vec![(0..11, Hl::Comment)]);
        assert_eq!(highlight_line("  replicaCount: 1"), vec![(2..14, Hl::Key), (16..17, Hl::Number)]);
        assert_eq!(highlight_line("enabled: TRUE"), vec![(0..7, Hl::Key), (9..13, Hl::Bool)]);
        assert_eq!(highlight_line("x: ~"), vec![(0..1, Hl::Key), (3..4, Hl::Null)]);
        assert_eq!(highlight_line("x:"), vec![(0..1, Hl::Key)]);
        assert_eq!(highlight_line("  - name"), vec![(0..4, Hl::Dash), (4..8, Hl::Str)]);
        assert_eq!(highlight_line("---"), vec![]);
        assert_eq!(highlight_line("v: -1.5"), vec![(0..1, Hl::Key), (3..7, Hl::Number)]);
    }
}
