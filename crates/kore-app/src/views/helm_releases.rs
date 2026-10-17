//! Helm releases list (spec 03 §4): availability check, skeleton, error and
//! empty states, sortable table (default Name asc), ↑/↓/Enter navigation.

use crate::automation::Automation;
use crate::backend;
use crate::keys;
use crate::state::AppState;
use crate::theme::{self, amber, emerald, misc, red, slate, KoreStyled, SANS};
use crate::ui::icon::icon;
use crate::ui::text::{caps, measure, measure_tracked};
use crate::ui::widgets::{ping, skeleton_bar};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use crate::views::rbac::{forbidden_banner, ForbiddenBanner};
use kore_core::state::helm::{parse_helm_time, HelmRelease};
use std::cmp::Ordering;
use std::time::Duration;

/// Helm status badge colors: (dot, text, bg, pulse) (spec 03 §4.3).
pub fn helm_status_colors(status: &str) -> (Rgba, Rgba, Rgba, bool) {
    let s = status.to_lowercase();
    if s == "deployed" {
        (emerald::E400, emerald::E400, theme::hexa(0x10b981, 0.1), true)
    } else if s == "failed" {
        (red::R400, red::R400, theme::hexa(0xef4444, 0.1), false)
    } else if s.starts_with("pending") {
        (amber::A400, amber::A400, theme::hexa(0xf59e0b, 0.1), false)
    } else if s == "superseded" {
        (slate::S400, slate::S400, theme::hexa(0x64748b, 0.1), false)
    } else if s == "uninstalling" {
        (misc::ORANGE400, misc::ORANGE400, theme::hexa(0xf97316, 0.1), false)
    } else {
        (slate::S400, slate::S300, theme::hexa(0x64748b, 0.1), false)
    }
}

/// Inline-flex pill: 6px dot (+ping when deployed) and the status text.
pub fn helm_status_badge(id: impl Into<ElementId>, status: &str) -> Div {
    let (dot, text, bg, pulse) = helm_status_colors(status);
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.))
        .px(px(8.))
        .py(px(2.))
        .rounded_full()
        .bg(bg)
        .text_xs_tw()
        .font_weight(FontWeight::MEDIUM)
        .text_color(text)
        .child(
            div()
                .relative()
                .size(px(6.))
                .flex_none()
                .when(pulse, |d| d.child(ping(id, dot)))
                .child(div().absolute().inset_0().rounded_full().bg(dot)),
        )
        .child(status.to_string())
}

/// Locale-style short date in local time (month short, day numeric, 2-digit
/// hour and minute). Both Helm formats are recognized: RFC 3339 (`helm
/// history`) and Go's time string (`helm list`, e.g. `2026-10-16 19:02:43.123456
/// +0300 IDT`; quirk 10.9, fixed). Anything else is shown raw; empty is "-".
pub fn format_helm_date(raw: &str) -> String {
    format_helm_date_in(raw, &chrono::Local)
}

/// `format_helm_date` in a given time zone (unit tests use UTC).
pub fn format_helm_date_in<Tz: chrono::TimeZone>(raw: &str, tz: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    if raw.trim().is_empty() {
        return "-".into();
    }
    match parse_helm_time(raw) {
        Some(d) => d.with_timezone(tz).format("%b %-d, %I:%M %p").to_string(),
        None => raw.to_string(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Col {
    Name,
    Namespace,
    Revision,
    Status,
    Chart,
    AppVersion,
    Updated,
}

pub const COLS: [(Col, &str, &str); 7] = [
    (Col::Name, "name", "Name"),
    (Col::Namespace, "namespace", "Namespace"),
    (Col::Revision, "revision", "Revision"),
    (Col::Status, "status", "Status"),
    (Col::Chart, "chart", "Chart"),
    (Col::AppVersion, "app_version", "App Version"),
    (Col::Updated, "updated", "Updated"),
];

fn sort_value(r: &HelmRelease, c: Col) -> &str {
    match c {
        Col::Name => &r.name,
        Col::Namespace => &r.namespace,
        Col::Revision => &r.revision,
        Col::Status => &r.status,
        Col::Chart => &r.chart,
        Col::AppVersion => &r.app_version,
        Col::Updated => &r.updated,
    }
}

/// TanStack "alphanumeric": case-insensitive, digit runs compared numerically.
pub fn alphanumeric(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let chunks = |s: &str| -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for ch in s.chars() {
            match out.last_mut() {
                Some(last) if last.chars().last().map(|c| c.is_ascii_digit()) == Some(ch.is_ascii_digit()) => last.push(ch),
                _ => out.push(ch.to_string()),
            }
        }
        out
    };
    let (ca, cb) = (chunks(&a), chunks(&b));
    for (x, y) in ca.iter().zip(cb.iter()) {
        let o = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(p), Ok(q)) => p.cmp(&q),
            _ => x.cmp(y),
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    ca.len().cmp(&cb.len())
}

pub fn sort_releases(rows: &mut [HelmRelease], col: Col, desc: bool) {
    rows.sort_by(|a, b| {
        let o = match (col, parse_helm_time(&a.updated), parse_helm_time(&b.updated)) {
            // Chronological, whatever offset each timestamp carries.
            (Col::Updated, Some(x), Some(y)) => x.cmp(&y),
            _ => alphanumeric(sort_value(a, col), sort_value(b, col)),
        };
        if desc {
            o.reverse()
        } else {
            o
        }
    });
}

pub struct HelmReleasesView {
    app: Entity<AppState>,
    helm_ready: Option<bool>,
    loading: bool,
    error: Option<String>,
    releases: Vec<HelmRelease>,
    sort: (Col, bool),
    selected: i64,
    namespace: String,
    task: Option<Task<()>>,
    /// "Permission Denied" analysis of a 403 from helm (spec 03 §7.8).
    forbidden: Entity<ForbiddenBanner>,
    _subs: Vec<Subscription>,
}

impl HelmReleasesView {
    pub fn new(app: Entity<AppState>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let namespace = app.read(cx).namespace.clone();
        let (forbidden, forbidden_sub) = forbidden_banner("helm.forbidden", cx);
        let subs = vec![
            forbidden_sub,
            cx.observe(&app, |this: &mut Self, app, cx| {
                let ns = app.read(cx).namespace.clone();
                if ns != this.namespace {
                    this.namespace = ns;
                    this.fetch(cx);
                }
                cx.notify();
            }),
            keys::on_window_key(cx, Self::on_key),
        ];
        let mut this = HelmReleasesView {
            app,
            helm_ready: None,
            loading: true,
            error: None,
            releases: Vec::new(),
            sort: (Col::Name, false),
            selected: -1,
            namespace,
            task: None,
            forbidden,
            _subs: subs,
        };
        this.task = Some(backend::run(cx, |core| async move { core.helm_available().await }, |this: &mut Self, res, cx| {
            let ok = res.unwrap_or(false);
            this.helm_ready = Some(ok);
            if ok {
                this.fetch(cx);
            } else {
                this.loading = false;
            }
            cx.notify();
        }));
        this
    }

    fn ns_arg(&self) -> Option<String> {
        (self.namespace != "*").then(|| self.namespace.clone())
    }

    fn fetch(&mut self, cx: &mut Context<Self>) {
        if self.helm_ready != Some(true) {
            return;
        }
        self.loading = true;
        self.error = None;
        let ns = self.ns_arg();
        self.task = Some(backend::run(cx, move |core| async move { core.list_helm_releases(ns).await }, |this: &mut Self, res, cx| {
            match res {
                Ok(r) => this.releases = r,
                Err(e) => this.error = Some(backend::format_error(&e)),
            }
            let err = this.error.clone();
            this.forbidden.update(cx, |b, cx| b.set_error(err.as_deref(), cx));
            this.loading = false;
            cx.notify();
        }));
        cx.notify();
    }

    fn rows(&self) -> Vec<HelmRelease> {
        let mut r = self.releases.clone();
        sort_releases(&mut r, self.sort.0, self.sort.1);
        r
    }

    fn open(&mut self, release: HelmRelease, cx: &mut Context<Self>) {
        self.app.update(cx, |a, cx| a.open_helm_release(release, cx));
    }

    fn on_key(&mut self, k: &keys::Key, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if keys::text_entry_focused(window) || !k.plain() || k.shift {
            return false;
        }
        if self.app.read(cx).modal_open() {
            return false;
        }
        let rows = self.rows();
        let n = rows.len() as i64;
        match k.key.as_str() {
            "down" => {
                self.selected = if self.selected < n - 1 { self.selected + 1 } else { self.selected };
                cx.notify();
                true
            }
            "up" => {
                self.selected = if self.selected > 0 { self.selected - 1 } else { 0 };
                cx.notify();
                true
            }
            "enter" if self.selected >= 0 && self.selected < n => {
                self.open(rows[self.selected as usize].clone(), cx);
                true
            }
            _ => false,
        }
    }

    /// HTML auto table layout: natural column widths, extra space distributed
    /// proportionally (`min-w-full`).
    fn widths(&self, rows: &[HelmRelease], avail: f32, window: &mut Window) -> Vec<f32> {
        let adv = 12. * theme::MONO_ADVANCE_EM;
        let mono = |s: &str| adv * s.chars().count() as f32;
        let mut widths: Vec<f32> = COLS
            .iter()
            .map(|(c, _, label)| {
                let head = measure_tracked(window, &label.to_uppercase(), SANS, 12., FontWeight::MEDIUM, 0.05) + 4. + 14.;
                let content = rows
                    .iter()
                    .map(|r| match c {
                        Col::Name => mono(&r.name),
                        Col::Namespace => mono(&r.namespace),
                        Col::Chart => mono(&r.chart),
                        Col::AppVersion => mono(if r.app_version.is_empty() { "-" } else { &r.app_version }),
                        Col::Revision => measure(window, &r.revision, SANS, 12., FontWeight::NORMAL),
                        Col::Updated => measure(window, &format_helm_date(&r.updated), SANS, 12., FontWeight::NORMAL),
                        Col::Status => 8. + 6. + 6. + measure(window, &r.status, SANS, 12., FontWeight::MEDIUM) + 8.,
                    })
                    .fold(0., f32::max);
                head.max(content) + 24.
            })
            .collect();
        let total: f32 = widths.iter().sum();
        if total > 0. && total < avail {
            let f = avail / total;
            for w in widths.iter_mut() {
                *w *= f;
            }
        }
        widths
    }

    fn render_table(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let accent = theme::accent(cx);
        let rows = self.rows();
        let avail = f32::from(window.viewport_size().width) - crate::views::sidebar::WIDTH - 32. - 2.;
        let widths = self.widths(&rows, avail, window);
        let (sort_col, desc) = self.sort;
        let header = div()
            .flex()
            .flex_row()
            .flex_none()
            .bg(theme::hexa(0x101828, 0.8))
            .border_b_1()
            .border_color(slate::S800)
            .children(COLS.iter().zip(widths.iter()).map(|((c, id, label), w)| {
                let c = *c;
                let sorted = (sort_col == c).then_some(desc);
                div()
                    .id(SharedString::from(format!("helm-th-{id}")))
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
                    .child(caps(label, 12.))
                    .child(match sorted {
                        Some(false) => icon("chevron-up", 14.).text_color(accent),
                        Some(true) => icon("chevron-down", 14.).text_color(accent),
                        None => icon("chevrons-up-down", 14.).text_color(slate::S600),
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.sort = if this.sort.0 == c { (c, !this.sort.1) } else { (c, false) };
                        cx.notify();
                    }))
                    .auto_id(format!("helm.header.{id}"), "columnheader", label.to_uppercase())
                    .selected(sorted.is_some())
            }));
        let body = rows.iter().enumerate().map(|(ix, r)| {
            let selected = ix as i64 == self.selected;
            let color = if selected { theme::hex(0xffffff) } else { slate::S100 };
            let cell = |w: f32| div().w(px(w)).flex_none().px(px(12.)).flex().flex_row().items_center().overflow_hidden().whitespace_nowrap();
            let rel = r.clone();
            div()
                .id(("helm-row", ix))
                .flex()
                .flex_col()
                .cursor_pointer()
                .text_xs_tw()
                .text_color(color)
                .child(div().h(px(1.)).flex_none().when(ix > 0, |d| d.bg(theme::hexa(0x1e293b, 0.5))))
                .child(
                    div()
                        .h(px(ROW_H))
                        .flex()
                        .flex_row()
                        .map(|d| if selected { d.bg(theme::hexa(0x334155, 0.6)) } else { d.hover(|s| s.bg(theme::hexa(0x1d2939, 0.3))) })
                        .child(cell(widths[0]).font_mono().child(r.name.clone()))
                        .child(cell(widths[1]).font_mono().child(r.namespace.clone()))
                        .child(cell(widths[2]).child(r.revision.clone()))
                        .child(cell(widths[3]).child(helm_status_badge(("helm-ping", ix), &r.status)))
                        .child(cell(widths[4]).font_mono().text_color(slate::S300).child(r.chart.clone()))
                        .child(
                            cell(widths[5])
                                .font_mono()
                                .text_color(slate::S400)
                                .child(if r.app_version.is_empty() { "-".to_string() } else { r.app_version.clone() }),
                        )
                        .child(
                            cell(widths[6])
                                .child(format_helm_date(&r.updated))
                                .auto_id(format!("helm.updated.{}/{}", r.namespace, r.name), "cell", format_helm_date(&r.updated)),
                        )
                        // The displayed cells, " | "-joined (for tests and snapshots).
                        .auto_id(
                            format!("helm.cells.{}/{}", r.namespace, r.name),
                            "group",
                            [
                                r.name.clone(),
                                r.namespace.clone(),
                                r.revision.clone(),
                                r.status.clone(),
                                r.chart.clone(),
                                if r.app_version.is_empty() { "-".to_string() } else { r.app_version.clone() },
                                format_helm_date(&r.updated),
                            ]
                            .join(" | "),
                        ),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.selected = ix as i64;
                    this.open(rel.clone(), cx);
                }))
                .auto_id(format!("helm.row.{}/{}", r.namespace, r.name), "row", r.name.clone())
                .selected(selected)
        });
        div()
            .id("helm-table")
            .size_full()
            .rounded(px(8.))
            .glass()
            .overflow_hidden()
            .child(div().id("helm-scroll").size_full().overflow_scroll().flex().flex_col().child(header).children(body))
            .with_animation("helm-fade", Animation::new(Duration::from_millis(300)), |el, t| el.opacity(t))
            .auto_id("view.helm", "table", "Helm Releases")
            .into_any_element()
    }
}

pub const ROW_H: f32 = 36.;

fn circle(icon_name: &str, color: Rgba) -> impl IntoElement {
    div().p(px(16.)).rounded_full().bg(theme::hexa(0x1e293b, 0.5)).child(icon(icon_name, 48.).text_color(color))
}

fn fade_up(id: &'static str, el: Div) -> AnyElement {
    el.with_animation(id, Animation::new(Duration::from_millis(300)).with_easing(ease_out_quint()), |el, t| {
        el.opacity(t).mt(px(10. * (1. - t)))
    })
    .into_any_element()
}

impl Render for HelmReleasesView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.helm_ready == Some(false) {
            return fade_up(
                "helm-na",
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(16.))
                    .text_color(slate::S400)
                    .child(circle("alert-circle", slate::S500))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .child(div().text_sm_tw().font_weight(FontWeight::MEDIUM).text_color(slate::S300).mb(px(4.)).child("Helm Not Available"))
                            .child(
                                div()
                                    .max_w(px(320.))
                                    .text_xs_tw()
                                    .text_color(slate::S500)
                                    .text_center()
                                    .child("The Helm CLI was not found on your system. Install Helm to manage releases."),
                            ),
                    )
                    .child(div().auto_id("helm.unavailable", "status", "Helm Not Available")),
            );
        }
        if self.loading {
            return div()
                .size_full()
                .rounded(px(8.))
                .glass()
                .overflow_hidden()
                .child(div().p(px(16.)).flex().flex_col().gap(px(12.)).children((0..8).map(|i| {
                    let ws = [128., 96., 48., 80., 112., 64.];
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(16.))
                        .opacity(1. - i as f32 * 0.08)
                        .children(ws.iter().enumerate().map(move |(j, w)| skeleton_bar(("helm-sk", i * 10 + j), px(*w).into(), 16.)))
                        .child(div().flex_1().child(skeleton_bar(("helm-sk-f", i), relative(1.), 16.)))
                })))
                .auto_id("helm.skeleton", "progressbar", "Loading")
                .into_any_element();
        }
        if let Some(err) = &self.error {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(12.))
                .child(icon("alert-circle", 48.).text_color(red::R400))
                .child(div().text_sm_tw().font_weight(FontWeight::MEDIUM).text_color(red::R400).child("Failed to load Helm releases"))
                .child(div().max_w(px(384.)).text_xs_tw().text_color(slate::S500).text_center().child(err.clone()))
                .when(self.forbidden.read(cx).visible(), |d| d.child(div().mt(px(8.)).w(px(560.)).max_w_full().child(self.forbidden.clone())))
                .auto_id("helm.error", "alert", "Failed to load Helm releases")
                .into_any_element();
        }
        if self.releases.is_empty() {
            let sub = if self.namespace != "*" {
                format!("No releases in namespace \"{}\"", self.namespace)
            } else {
                "Try selecting a namespace or deploying a chart".to_string()
            };
            return fade_up(
                "helm-empty",
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(12.))
                    .text_color(slate::S400)
                    .child(circle("package", slate::S600))
                    .child(div().text_sm_tw().font_weight(FontWeight::MEDIUM).child("No Helm releases found"))
                    .child(div().text_xs_tw().text_color(slate::S500).child(sub))
                    .child(div().auto_id("helm.empty", "status", "No Helm releases found")),
            );
        }
        self.render_table(window, cx)
    }
}

#[cfg(test)]
mod tests {
    use super::{alphanumeric, format_helm_date, format_helm_date_in, sort_releases, Col};
    use kore_core::state::helm::HelmRelease;
    use std::cmp::Ordering;

    #[test]
    fn alphanumeric_sorts_numbers_numerically() {
        assert_eq!(alphanumeric("rev10", "rev9"), Ordering::Greater);
        assert_eq!(alphanumeric("Alpha", "beta"), Ordering::Less);
        assert_eq!(alphanumeric("2", "10"), Ordering::Less);
    }

    #[test]
    fn helm_list_and_history_dates_format_alike() {
        let utc = chrono::Utc;
        // helm list (Go time string) and helm history (RFC 3339) render the same way.
        assert_eq!(format_helm_date_in("2026-10-16 19:02:43.123456 +0300 IDT", &utc), "Oct 16, 04:02 PM");
        assert_eq!(format_helm_date_in("2026-10-16T19:02:43.123456+03:00", &utc), "Oct 16, 04:02 PM");
        assert_eq!(format_helm_date_in("2026-10-15 10:00:00.123456 +0000 UTC", &utc), "Oct 15, 10:00 AM");
        assert_eq!(format_helm_date(""), "-");
        assert_eq!(format_helm_date("garbage"), "garbage");
        assert!(!format_helm_date("2026-10-15 10:00:00.123456 +0000 UTC").contains("UTC"));
    }

    #[test]
    fn updated_sorts_chronologically_across_offsets() {
        let rel = |name: &str, updated: &str| HelmRelease {
            name: name.into(),
            namespace: "ns".into(),
            revision: "1".into(),
            updated: updated.into(),
            status: "deployed".into(),
            chart: "c".into(),
            app_version: "1".into(),
        };
        // b is earlier in absolute time although its local string sorts later.
        let mut rows = vec![rel("a", "2026-10-16 10:00:00 +0000 UTC"), rel("b", "2026-10-16 12:00:00 +0300 IDT")];
        sort_releases(&mut rows, Col::Updated, false);
        assert_eq!(rows[0].name, "b");
    }
}
