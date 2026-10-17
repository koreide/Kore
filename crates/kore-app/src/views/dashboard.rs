//! Cluster dashboard (Task 6; spec 03 §1): health score ring + breakdown,
//! pod health donut (Recharts look), node status bars, restart hotlist,
//! pending pods, recent warnings; single and multi-cluster modes; 30s polling.

use crate::automation::Automation;
use crate::backend;
use crate::state::resource_item::{format_relative_age, ResourceItem};
use crate::state::types::ResourceKind;
use crate::state::AppState;
use crate::theme::{self, amber, emerald, red, slate, KoreStyled};
use crate::ui::icon::icon;
use crate::ui::mono_text::{mono_paragraph, mono_text};
use crate::ui::scrollbar;
use crate::ui::text::tracked;
use crate::ui::widgets::spinning_icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use kore_core::state::dashboard::{ClusterHealth, ClusterHealthEntry};
use std::cell::Cell;
use std::collections::HashMap;
use std::f32::consts::PI;
use std::rc::Rc;
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_secs(30);

// ── Pure helpers (k8s-utils.ts) ─────────────────────────────────────────

/// JS `parseFloat`: the longest numeric prefix, NaN → None.
fn parse_float_prefix(s: &str) -> Option<f64> {
    let s = s.trim_start();
    let b = s.as_bytes();
    let mut end = 0;
    let mut seen_digit = false;
    let mut seen_dot = false;
    let mut seen_exp = false;
    while end < b.len() {
        let c = b[end];
        match c {
            b'0'..=b'9' => seen_digit = true,
            b'+' | b'-' if end == 0 || matches!(b[end - 1], b'e' | b'E') => {}
            b'.' if !seen_dot && !seen_exp => seen_dot = true,
            b'e' | b'E' if seen_digit && !seen_exp => {
                // Only an exponent if digits follow.
                let rest = &b[end + 1..];
                let rest = rest.strip_prefix(b"+").or_else(|| rest.strip_prefix(b"-")).unwrap_or(rest);
                if !rest.first().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                    break;
                }
                seen_exp = true;
            }
            _ => break,
        }
        end += 1;
    }
    if !seen_digit {
        return None;
    }
    s[..end].parse::<f64>().ok()
}

/// `parseKubeQuantity`.
pub fn parse_kube_quantity(q: &str) -> f64 {
    let t = q.trim();
    let num = parse_float_prefix(t);
    let n = num.unwrap_or(f64::NAN);
    if t.ends_with("Ki") {
        n * 1024.
    } else if t.ends_with("Mi") {
        n * 1024. * 1024.
    } else if t.ends_with("Gi") {
        n * 1024f64.powi(3)
    } else if t.ends_with("Ti") {
        n * 1024f64.powi(4)
    } else if t.ends_with('m') {
        n / 1000.
    } else if t.ends_with('n') {
        n / 1e9
    } else {
        num.unwrap_or(0.)
    }
}

/// Relative "last seen" label for a warning ("5m ago"; "-" when unknown).
pub fn last_seen_label(ts: &str) -> String {
    match format_relative_age(Some(ts)) {
        s if s == "-" => s,
        s => format!("{s} ago"),
    }
}

/// Human CPU quantity: millicores below one core ("250m"), else cores with
/// at most one decimal ("2", "3.8").
pub fn format_cpu(q: &str) -> String {
    let cores = parse_kube_quantity(q);
    if !cores.is_finite() {
        return q.to_string();
    }
    if cores < 1. {
        format!("{}m", (cores * 1000.).round() as i64)
    } else {
        let s = format!("{cores:.1}");
        s.strip_suffix(".0").map(String::from).unwrap_or(s)
    }
}

/// Human memory quantity in binary units ("512Mi", "1.5Gi").
pub fn format_memory(q: &str) -> String {
    let b = parse_kube_quantity(q);
    if !b.is_finite() {
        return q.to_string();
    }
    let (gi, mi) = (1024f64.powi(3), 1024f64.powi(2));
    if b >= gi {
        let s = format!("{:.1}", b / gi);
        format!("{}Gi", s.strip_suffix(".0").unwrap_or(&s))
    } else if b >= mi {
        format!("{}Mi", (b / mi).round() as i64)
    } else {
        format!("{}Ki", (b / 1024.).round() as i64)
    }
}

/// `parseUsagePercent`.
pub fn usage_percent(usage: &str, capacity: &str) -> f64 {
    if usage.is_empty() || capacity.is_empty() {
        return 0.;
    }
    let u = parse_kube_quantity(usage);
    let c = parse_kube_quantity(capacity);
    if c == 0. {
        return 0.;
    }
    // Math.round (half up); NaN → NaN → CSS width invalid → treat as 0.
    let r = (u / c * 100. + 0.5).floor();
    if r.is_nan() {
        0.
    } else {
        r.min(100.)
    }
}

pub fn score_label(score: u32) -> &'static str {
    if score >= 80 {
        "Healthy"
    } else if score >= 50 {
        "Degraded"
    } else {
        "Critical"
    }
}

/// (text-400, base-500 hex) for a score.
fn score_colors(score: u32) -> (Rgba, u32) {
    if score >= 80 {
        (emerald::E400, 0x10b981)
    } else if score >= 50 {
        (amber::A400, 0xf59e0b)
    } else {
        (red::R400, 0xef4444)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BreakdownRow {
    pub label: &'static str,
    pub detail: String,
    pub value: String,
    pub negative: bool,
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n != 1 { "s" } else { "" })
}

/// `ScoreBreakdown` rows.
pub fn breakdown_rows(h: &ClusterHealth) -> Vec<BreakdownRow> {
    let ready = h.nodes.iter().filter(|n| n.status == "Ready").count();
    let node_score = ready as f64 / h.nodes.len().max(1) as f64 * 30.;
    let pod_score = h.pods.running as f64 / h.pods.total.max(1) as f64 * 40.;
    let crash = (h.pods.crash_looping as f64 * 5.).min(15.);
    let pending = (h.pods.pending as f64 * 2.).min(10.);
    let warn = (h.recent_warnings.len() as f64 * 0.5).min(5.);
    let mut rows = vec![
        BreakdownRow {
            label: "Node health",
            detail: format!("{ready}/{} ready", h.nodes.len()),
            value: format!("+{}", to_fixed1(node_score)),
            negative: false,
        },
        BreakdownRow {
            label: "Pod health",
            detail: format!("{}/{} running", h.pods.running, h.pods.total),
            value: format!("+{}", to_fixed1(pod_score)),
            negative: false,
        },
        BreakdownRow { label: "Base score", detail: String::new(), value: "+30.0".into(), negative: false },
    ];
    if crash > 0. {
        rows.push(BreakdownRow {
            label: "CrashLoop penalty",
            detail: plural(h.pods.crash_looping as usize, "pod"),
            value: format!("-{}", to_fixed1(crash)),
            negative: true,
        });
    }
    if pending > 0. {
        rows.push(BreakdownRow {
            label: "Pending penalty",
            detail: plural(h.pods.pending as usize, "pod"),
            value: format!("-{}", to_fixed1(pending)),
            negative: true,
        });
    }
    if warn > 0. {
        rows.push(BreakdownRow {
            label: "Warning penalty",
            detail: plural(h.recent_warnings.len(), "event"),
            value: format!("-{}", to_fixed1(warn)),
            negative: true,
        });
    }
    rows
}

/// JS `Number.prototype.toFixed(1)` for non-negative finite values.
fn to_fixed1(v: f64) -> String {
    format!("{:.1}", (v * 10.).round() / 10.)
}

/// Pie data: (name, value, color), non-zero only, in Recharts order.
pub fn pod_data(h: &ClusterHealth) -> Vec<(&'static str, u32, u32)> {
    [
        ("Running", h.pods.running, 0x10b981),
        ("Pending", h.pods.pending, 0xf59e0b),
        ("Failed", h.pods.failed, 0xef4444),
        ("CrashLooping", h.pods.crash_looping, 0xa855f7),
        ("Succeeded", h.pods.succeeded, 0x64748b),
    ]
    .into_iter()
    .filter(|d| d.1 > 0)
    .collect()
}

/// Recharts `Pie` sector angles (degrees, counter-clockwise from 3 o'clock):
/// startAngle 0, endAngle 360, paddingAngle 2, minAngle 0.
pub fn pie_sectors(values: &[u32]) -> Vec<(f32, f32)> {
    let sum: f64 = values.iter().map(|v| *v as f64).sum();
    let non_zero = values.iter().filter(|v| **v != 0).count() as f64;
    let pad = 2.0f64;
    let real_total = 360. - non_zero * pad; // absDelta >= 360 → padding for every non-zero item
    let mut out: Vec<(f32, f32)> = Vec::with_capacity(values.len());
    let mut prev_end = 0f64;
    for (i, v) in values.iter().enumerate() {
        let percent = if sum > 0. { *v as f64 / sum } else { 0. };
        let start = if i > 0 { prev_end + pad * if *v != 0 { 1. } else { 0. } } else { 0. };
        let end = start + percent * real_total;
        out.push((start as f32, end as f32));
        prev_end = end;
    }
    out
}

/// Recharts entry animation: each sector grows by `t` of its sweep, laid out
/// one after another from the first sector's start (+ padding between).
fn animated_sectors(sectors: &[(f32, f32)], t: f32) -> Vec<(f32, f32)> {
    let mut cur = sectors.first().map(|s| s.0).unwrap_or(0.);
    sectors
        .iter()
        .enumerate()
        .map(|(i, (s, e))| {
            let pad = if i > 0 { 2. } else { 0. };
            let a = cur + pad;
            let b = cur + (e - s) * t + pad;
            cur = b;
            (a, b)
        })
        .collect()
}

/// CSS `ease` = cubic-bezier(.25,.1,.25,1).
fn css_ease(x: f32) -> f32 {
    cubic_bezier(0.25, 0.1, 0.25, 1.0, x)
}

fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    if x <= 0. {
        return 0.;
    }
    if x >= 1. {
        return 1.;
    }
    let bx = |t: f32| 3. * (1. - t).powi(2) * t * x1 + 3. * (1. - t) * t * t * x2 + t * t * t;
    let by = |t: f32| 3. * (1. - t).powi(2) * t * y1 + 3. * (1. - t) * t * t * y2 + t * t * t;
    let (mut lo, mut hi) = (0f32, 1f32);
    for _ in 0..30 {
        let mid = (lo + hi) / 2.;
        if bx(mid) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    by((lo + hi) / 2.)
}

// ── View ────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct Cluster {
    context: Option<String>,
    health: ClusterHealth,
}

#[derive(Default)]
struct CardState {
    breakdown: bool,
    lists: HashMap<&'static str, (ScrollHandle, scrollbar::Gutter)>,
    pie_hover: Option<(usize, Point<Pixels>)>,
    pie_bounds: Rc<Cell<Bounds<Pixels>>>,
}

pub struct DashboardView {
    app: Entity<AppState>,
    multi: bool,
    clusters: Option<Vec<Cluster>>,
    loading: bool,
    error: Option<String>,
    /// When data first arrived (pie entry animation start).
    loaded_at: Option<Instant>,
    cards: HashMap<String, CardState>,
    scroll: ScrollHandle,
    gutter: scrollbar::Gutter,
    fetch_task: Option<Task<()>>,
    poll_task: Option<Task<()>>,
    _subs: Vec<Subscription>,
}

impl DashboardView {
    pub fn new(app: Entity<AppState>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let multi = app.read(cx).multi_cluster;
        let subs = vec![cx.observe(&app, |this: &mut Self, app, cx| {
            let m = app.read(cx).multi_cluster;
            if m != this.multi {
                this.multi = m;
                this.start(cx);
            }
            cx.notify();
        })];
        let mut v = DashboardView {
            app,
            multi,
            clusters: None,
            loading: true,
            error: None,
            loaded_at: None,
            cards: HashMap::new(),
            scroll: ScrollHandle::new(),
            gutter: scrollbar::Gutter::default(),
            fetch_task: None,
            poll_task: None,
            _subs: subs,
        };
        v.start(cx);
        v
    }

    /// Fetch now and every 30s (restarted when `multiCluster` changes).
    fn start(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.fetch(cx);
        self.poll_task = Some(cx.spawn(async move |this, cx| loop {
            let t = cx.update(|cx| backend::sleep(cx, POLL));
            t.await;
            if this.update(cx, |this, cx| this.fetch(cx)).is_err() {
                break;
            }
        }));
    }

    fn fetch(&mut self, cx: &mut Context<Self>) {
        let multi = self.multi;
        self.fetch_task = Some(backend::run(
            cx,
            move |core| async move {
                if multi {
                    core.get_cluster_health_multi_cluster().await.map(|m| {
                        m.clusters
                            .into_iter()
                            .map(|ClusterHealthEntry { context, health }| Cluster { context: Some(context), health })
                            .collect::<Vec<_>>()
                    })
                } else {
                    core.get_cluster_health().await.map(|h| vec![Cluster { context: None, health: h }])
                }
            },
            move |this: &mut Self, res, cx| {
                match res {
                    Ok(c) => {
                        if this.clusters.is_none() {
                            this.loaded_at = Some(Instant::now());
                        }
                        this.clusters = Some(c);
                        this.error = None;
                    }
                    Err(e) => this.error = Some(backend::format_error(&e)),
                }
                this.loading = false;
                cx.notify();
            },
        ));
    }

    fn navigate(&self, name: String, namespace: String, cx: &mut Context<Self>) {
        let item = ResourceItem { name, namespace, ..Default::default() };
        self.app.update(cx, |a, cx| a.open_resource(Some(ResourceKind::Pods), item, cx));
    }

    fn card_state(&mut self, key: &str) -> &mut CardState {
        self.cards.entry(key.to_string()).or_default()
    }
}

/// Columns of the card grid for the window width (md 768, xl 1280).
fn grid_columns(window: &Window) -> usize {
    let w = f32::from(window.viewport_size().width);
    if w >= 1280. {
        3
    } else if w >= 768. {
        2
    } else {
        1
    }
}

fn shadow_lg(color: Rgba) -> Vec<BoxShadow> {
    vec![
        BoxShadow { color: color.into(), offset: point(px(0.), px(10.)), blur_radius: px(15.), spread_radius: px(-3.), inset: false },
        BoxShadow { color: color.into(), offset: point(px(0.), px(4.)), blur_radius: px(6.), spread_radius: px(-4.), inset: false },
    ]
}

/// `DashboardCard` chrome with the fade-up entry animation.
fn card(id: SharedString, title: &str, icon_name: &str, accent: Rgba, body: impl IntoElement) -> AnyElement {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .child(
            div()
                .id(id.clone())
                .flex_1()
                .min_w_0()
                .relative()
                .glass()
                .rounded(px(8.))
                .p(px(16.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .mb(px(12.))
                        .child(icon(icon_name, 16.).text_color(accent))
                        .child(
                            div()
                                .text_sm_tw()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(slate::S200)
                                .child(tracked(title.to_uppercase(), 14., 0.025)),
                        ),
                )
                .child(div().flex_1().min_h_0().flex().flex_col().child(body))
                .with_animation(id, Animation::new(Duration::from_millis(300)).with_easing(ease_out_quad), |el, t| {
                    el.opacity(t).top(px(12. * (1. - t)))
                }),
        )
        .auto_id(format!("dashboard.card.{}", title.to_lowercase().replace(' ', "-")), "region", title.to_string())
        .into_any_element()
}

fn empty_body(text: &'static str) -> AnyElement {
    div()
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .text_sm_tw()
        .text_color(slate::S500)
        .child(text)
        .into_any_element()
}

/// A `max-h-48 overflow-auto` list with a custom scrollbar.
fn scroll_list(id: SharedString, handle: &ScrollHandle, gutter: &scrollbar::Gutter, gap: f32, items: Vec<AnyElement>) -> AnyElement {
    let g = gutter.overflows();
    div()
        .relative()
        .flex()
        .flex_row()
        .max_h(px(192.))
        .child(
            div()
                .id(id)
                .flex_1()
                .min_w_0()
                .max_h(px(192.))
                .overflow_y_scroll()
                .track_scroll(handle)
                .flex()
                .flex_col()
                .gap(px(gap))
                .children(items),
        )
        .when(g, |d| d.child(div().w(px(scrollbar::WIDTH)).flex_none()))
        .child(scrollbar::vertical(handle.clone(), gutter.clone()))
        .into_any_element()
}

impl DashboardView {
    fn health_card(&mut self, key: &str, h: &ClusterHealth, accent: Rgba, cx: &mut Context<Self>) -> AnyElement {
        let (color, base) = score_colors(h.score);
        let show = self.card_state(key).breakdown;
        let k = key.to_string();
        let toggle = div()
            .id(SharedString::from(format!("{key}-bd-toggle")))
            .mt(px(8.))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.))
            .text_10()
            .text_color(slate::S500)
            .cursor_pointer()
            .hover(|s| s.text_color(slate::S300))
            .group("bd-toggle")
            .child(icon("info", 12.).text_color(slate::S500).group_hover("bd-toggle", |s| s.text_color(slate::S300)))
            .child(if show { "Hide breakdown" } else { "How is this calculated?" })
            .on_click(cx.listener(move |this, _, _, cx| {
                let s = this.card_state(&k);
                s.breakdown = !s.breakdown;
                cx.notify();
            }))
            .auto_id(format!("dashboard.breakdown-toggle.{key}"), "button", if show { "Hide breakdown" } else { "How is this calculated?" });

        let top = div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .py(px(16.))
            .child(
                div()
                    .size(px(112.))
                    .flex_none()
                    .rounded_full()
                    .border_4()
                    .border_color(theme::hexa(base, 0.3))
                    .shadow(shadow_lg(theme::hexa(base, 0.1)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .text_size(px(36.))
                            .line_height(px(40.))
                            .font_weight(FontWeight::BOLD)
                            .font_mono()
                            .text_color(color)
                            .child(h.score.to_string())
                            .auto_id(format!("dashboard.score.{key}"), "text", h.score.to_string()),
                    ),
            )
            .child(
                div()
                    .mt(px(12.))
                    .text_sm_tw()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(color)
                    .child(score_label(h.score))
                    .auto_id(format!("dashboard.score-label.{key}"), "text", score_label(h.score)),
            )
            .child(
                div()
                    .mt(px(4.))
                    .text_xs_tw()
                    .text_color(slate::S500)
                    .child(format!("{} pods across {} nodes", h.pods.total, h.nodes.len()))
                    .auto_id(format!("dashboard.summary.{key}"), "text", format!("{} pods across {} nodes", h.pods.total, h.nodes.len())),
            )
            .child(toggle);

        let breakdown = show.then(|| {
            let rows = breakdown_rows(h);
            div()
                .id(SharedString::from(format!("{key}-bd")))
                .mt(px(12.))
                .pt(px(12.))
                .border_t_1()
                .border_color(theme::hexa(0x1e293b, 0.6))
                .flex()
                .flex_col()
                .gap(px(6.))
                .text_11()
                .line_height(px(16.5))
                .font_mono()
                .children(rows.into_iter().enumerate().map(|(i, r)| {
                    let label = [r.label, r.detail.as_str(), r.value.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" ");
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .child(div().flex_1().text_color(slate::S400).child(mono_text(r.label)))
                        .when(!r.detail.is_empty(), |d| d.child(div().text_color(slate::S600).child(mono_text(r.detail))))
                        .child(
                            div()
                                .w(px(48.))
                                .flex_none()
                                .flex()
                                .justify_end()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(if r.negative { theme::hexa(0xf87171, 0.8) } else { theme::hexa(0x34d399, 0.8) })
                                .child(mono_text(r.value)),
                        )
                        .auto_id(format!("dashboard.breakdown.{key}.{i}"), "text", label)
                }))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .pt(px(6.))
                        .border_t_1()
                        .border_color(theme::hexa(0x1e293b, 0.4))
                        .child(div().flex_1().text_color(slate::S300).font_weight(FontWeight::MEDIUM).child(mono_text("Total")))
                        .child(
                            div()
                                .w(px(48.))
                                .flex_none()
                                .flex()
                                .justify_end()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(color)
                                .child(mono_text(h.score.to_string())),
                        )
                        .auto_id(format!("dashboard.breakdown.{key}.total"), "text", format!("Total {}", h.score)),
                )
                .child(
                    div()
                        .text_10()
                        .line_height(px(16.25))
                        .text_color(slate::S600)
                        .pt(px(4.))
                        .child(mono_paragraph(
                            "Score is clamped to 0-100. Node & pod ratios scale up to 30 and 40 points respectively. \
                             Penalties are capped at -15 (crash), -10 (pending), -5 (warnings).",
                        ).line_height(px(16.25)))
                        .auto_id(format!("dashboard.breakdown.{key}.note"), "text", "Score is clamped to 0-100."),
                )
                .with_animation(
                    SharedString::from(format!("{key}-bd-anim")),
                    Animation::new(Duration::from_millis(200)),
                    |el, t| el.opacity(t),
                )
        });

        card(format!("{key}-health").into(), "Cluster Health", "heart", accent, div().flex().flex_col().child(top).children(breakdown))
    }

    fn pod_card(&mut self, key: &str, h: &ClusterHealth, accent: Rgba, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let data = pod_data(h);
        if data.is_empty() {
            return card(format!("{key}-pods").into(), "Pod Health", "activity", accent, empty_body("No pods found"));
        }
        let sectors = pie_sectors(&data.iter().map(|d| d.1).collect::<Vec<_>>());
        let elapsed = self.loaded_at.map(|t| t.elapsed().as_secs_f32()).unwrap_or(0.);
        let t = ((elapsed - 0.4) / 1.5).clamp(0., 1.);
        if t < 1. {
            window.request_animation_frame();
        }
        let shown = animated_sectors(&sectors, css_ease(t));
        let colors: Vec<u32> = data.iter().map(|d| d.2).collect();
        let st = self.card_state(key);
        let bounds_cell = st.pie_bounds.clone();
        let hover = st.pie_hover;
        let k_move = key.to_string();
        let k_leave = key.to_string();
        let sectors_hit = shown.clone();

        let chart_canvas = {
            let shown = shown.clone();
            let bc = bounds_cell.clone();
            canvas(
                move |b, _, _| bc.set(b),
                move |b, _, window, _| {
                    let c = b.center();
                    for (i, (a0, a1)) in shown.iter().enumerate() {
                        if (a1 - a0).abs() < 0.01 {
                            continue;
                        }
                        if let Some(p) = sector_path(c, 35., 60., *a0, *a1) {
                            window.paint_path(p, theme::hex(colors[i]));
                        }
                    }
                },
            )
            .size_full()
        };

        let chart = div()
            .id(SharedString::from(format!("{key}-pie")))
            .relative()
            .size(px(160.))
            .flex_none()
            .child(chart_canvas)
            .on_mouse_move(cx.listener(move |this, e: &MouseMoveEvent, _, cx| {
                let b = this.card_state(&k_move).pie_bounds.get();
                let hit = sector_at(b, e.position, &sectors_hit);
                let st = this.card_state(&k_move);
                let new = hit.map(|i| (i, e.position - b.origin));
                if st.pie_hover.map(|h| h.0) != new.map(|h| h.0) {
                    st.pie_hover = new;
                    cx.notify();
                }
            }))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if !*hovered {
                    this.card_state(&k_leave).pie_hover = None;
                    cx.notify();
                }
            }))
            .when_some(hover, |d, (i, _)| {
                let (name, value, color) = data[i];
                // Recharts positions the tooltip at the sector's mid point + 10px.
                let (a0, a1) = shown[i];
                let mid = ((a0 + a1) / 2.).to_radians();
                let r = (35. + 60.) / 2.;
                let x = 80. + r * mid.cos();
                let y = 80. - r * mid.sin();
                d.child(
                    deferred(
                        div()
                            .absolute()
                            .left(px(x + 10.))
                            .top(px(y + 10.))
                            .bg(theme::SURFACE)
                            .border_1()
                            .border_color(slate::S700)
                            .rounded(px(4.))
                            .px(px(12.))
                            .py(px(8.))
                            .text_xs_tw()
                            .shadow(shadow_lg(theme::hexa(0, 0.1)))
                            .flex()
                            .flex_row()
                            .items_center()
                            .whitespace_nowrap()
                            .child(div().size(px(8.)).rounded_full().mr(px(8.)).bg(theme::hex(color)))
                            .child(div().text_color(slate::S300).child(format!("{name}: ")))
                            .child(div().text_color(slate::S100).font_weight(FontWeight::SEMIBOLD).child(value.to_string())),
                    )
                    .with_priority(1),
                )
            });

        let legend = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .text_xs_tw()
            .children(data.iter().map(|(name, value, color)| {
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(div().size(px(10.)).flex_none().rounded_full().bg(theme::hex(*color)))
                    .child(div().text_color(slate::S400).child(*name))
                    .child(div().ml_auto().font_mono().text_color(slate::S200).child(value.to_string()))
                    .auto_id(format!("dashboard.pod-legend.{key}.{name}"), "text", value.to_string())
            }))
            .child(
                div()
                    .border_t_1()
                    .border_color(slate::S800)
                    .pt(px(4.))
                    .mt(px(4.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(div().text_color(slate::S500).child("Total"))
                    .child(div().ml_auto().font_mono().text_color(slate::S300).child(h.pods.total.to_string()))
                    .auto_id(format!("dashboard.pod-legend.{key}.Total"), "text", h.pods.total.to_string()),
            );

        card(
            format!("{key}-pods").into(),
            "Pod Health",
            "activity",
            accent,
            div().flex().flex_row().items_center().gap(px(16.)).child(chart).child(legend),
        )
    }

    fn list_handles(&mut self, key: &str, list: &'static str) -> (ScrollHandle, scrollbar::Gutter) {
        self.card_state(key).lists.entry(list).or_insert_with(|| (ScrollHandle::new(), scrollbar::Gutter::default())).clone()
    }

    fn node_card(&mut self, key: &str, h: &ClusterHealth, accent: Rgba) -> AnyElement {
        if h.nodes.is_empty() {
            return card(format!("{key}-nodes").into(), "Node Status", "server", accent, empty_body("No node data"));
        }
        let (handle, gutter) = self.list_handles(key, "nodes");
        // Live usage (metrics-server) against allocatable; "?" when node
        // metrics are unavailable (empty bar). Quirk 10.12, fixed.
        let bar = |node: &str, label: &'static str, usage: Option<&str>, alloc: &str, fmt: fn(&str) -> String, color: Rgba| {
            let usage = usage.unwrap_or("");
            let pct = usage_percent(usage, alloc) as f32;
            let u = if usage.is_empty() { "?".to_string() } else { fmt(usage) };
            let c = if alloc.is_empty() { "?".to_string() } else { fmt(alloc) };
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .text_10()
                .line_height(px(15.))
                .text_color(slate::S500)
                .child(div().w(px(32.)).flex_none().child(label))
                .child(
                    div()
                        .flex_1()
                        .h(px(6.))
                        .bg(slate::S800)
                        .rounded_full()
                        .overflow_hidden()
                        .child(div().h_full().w(relative(pct / 100.)).rounded_full().bg(color)),
                )
                .child(
                    div()
                        .w(px(96.))
                        .flex_none()
                        .text_right()
                        .text_color(slate::S400)
                        .child(format!("{u} / {c}"))
                        .auto_id(format!("dashboard.node.{node}.{}", label.to_lowercase()), "text", format!("{u} / {c}")),
                )
        };
        let items: Vec<AnyElement> = h
            .nodes
            .iter()
            .map(|n| {
                let color = theme::hex(match n.status.as_str() {
                    "Ready" => 0x10b981,
                    "NotReady" => 0xef4444,
                    _ => 0xf59e0b,
                });
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .text_xs_tw()
                            .child(div().max_w(relative(0.62)).truncate().font_mono().text_color(slate::S300).child(n.name.clone()))
                            .child(div().font_weight(FontWeight::MEDIUM).text_color(color).child(n.status.clone()))
                            .auto_id(format!("dashboard.node.{}", n.name), "text", n.status.clone()),
                    )
                    .child(bar(&n.name, "CPU", n.cpu_usage.as_deref(), &n.cpu_allocatable, format_cpu, color))
                    .child(bar(&n.name, "Mem", n.memory_usage.as_deref(), &n.memory_allocatable, format_memory, color))
                    .into_any_element()
            })
            .collect();
        card(
            format!("{key}-nodes").into(),
            "Node Status",
            "server",
            accent,
            scroll_list(format!("{key}-nodes-list").into(), &handle, &gutter, 12., items),
        )
    }

    fn hotlist_card(&mut self, key: &str, h: &ClusterHealth, accent: Rgba, cx: &mut Context<Self>) -> AnyElement {
        if h.restart_hotlist.is_empty() {
            return card(format!("{key}-hot").into(), "Restart Hotlist", "refresh-cw", accent, empty_body("No restarts detected"));
        }
        let (handle, gutter) = self.list_handles(key, "hot");
        let items: Vec<AnyElement> = h
            .restart_hotlist
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let g: SharedString = format!("{key}-hot-{i}").into();
                let (name, ns) = (p.name.clone(), p.namespace.clone());
                let rc = if p.restarts >= 10 {
                    red::R400
                } else if p.restarts >= 5 {
                    amber::A400
                } else {
                    slate::S400
                };
                div()
                    .id(g.clone())
                    .group(g.clone())
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .px(px(8.))
                    .py(px(6.))
                    .rounded(px(4.))
                    .text_xs_tw()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::hexa(0x1e293b, 0.6)))
                    .child(div().w(px(16.)).flex_none().flex().justify_end().font_mono().text_color(slate::S500).child((i + 1).to_string()))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_mono()
                            .text_color(slate::S300)
                            .group_hover(g.clone(), |s| s.text_color(accent))
                            .child(p.name.clone()),
                    )
                    .child(div().max_w(px(80.)).truncate().text_10().text_color(slate::S500).child(p.namespace.clone()))
                    .child(div().font_mono().font_weight(FontWeight::SEMIBOLD).text_color(rc).child(p.restarts.to_string()))
                    .on_click(cx.listener(move |this, _, _, cx| this.navigate(name.clone(), ns.clone(), cx)))
                    .auto_id(format!("dashboard.hotlist.{}/{}", p.namespace, p.name), "button", p.name.clone())
                    .into_any_element()
            })
            .collect();
        card(
            format!("{key}-hot").into(),
            "Restart Hotlist",
            "refresh-cw",
            accent,
            scroll_list(format!("{key}-hot-list").into(), &handle, &gutter, 4., items),
        )
    }

    fn pending_card(&mut self, key: &str, h: &ClusterHealth, accent: Rgba, cx: &mut Context<Self>) -> AnyElement {
        if h.pending_pods.is_empty() {
            return card(format!("{key}-pending").into(), "Pending Pods", "clock", accent, empty_body("No pending pods"));
        }
        let (handle, gutter) = self.list_handles(key, "pending");
        let items: Vec<AnyElement> = h
            .pending_pods
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let g: SharedString = format!("{key}-pend-{i}").into();
                let (name, ns) = (p.name.clone(), p.namespace.clone());
                div()
                    .id(g.clone())
                    .group(g.clone())
                    .w_full()
                    .px(px(8.))
                    .py(px(6.))
                    .rounded(px(4.))
                    .text_xs_tw()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::hexa(0x1e293b, 0.6)))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .font_mono()
                                    .text_color(slate::S300)
                                    .group_hover(g.clone(), |s| s.text_color(accent))
                                    .child(p.name.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .ml(px(8.))
                                    .text_10()
                                    .text_color(slate::S600)
                                    .child(format_relative_age(Some(p.age.as_str())))
                                    .auto_id(format!("dashboard.pending.{}/{}.age", p.namespace, p.name), "text", format_relative_age(Some(p.age.as_str()))),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(4.))
                            .mt(px(2.))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_10()
                                    .text_color(theme::hexa(0xfbbf24, 0.7))
                                    .child(if p.reason.is_empty() { "Unknown".to_string() } else { p.reason.clone() })
                                    .auto_id(
                                        format!("dashboard.pending.{}/{}.reason", p.namespace, p.name),
                                        "text",
                                        if p.reason.is_empty() { "Unknown".to_string() } else { p.reason.clone() },
                                    ),
                            )
                            .child(div().ml_auto().max_w(px(80.)).truncate().text_10().text_color(slate::S600).child(p.namespace.clone())),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.navigate(name.clone(), ns.clone(), cx)))
                    .auto_id(format!("dashboard.pending.{}/{}", p.namespace, p.name), "button", p.name.clone())
                    .into_any_element()
            })
            .collect();
        card(
            format!("{key}-pending").into(),
            "Pending Pods",
            "clock",
            accent,
            scroll_list(format!("{key}-pending-list").into(), &handle, &gutter, 8., items),
        )
    }

    fn warnings_card(&mut self, key: &str, h: &ClusterHealth, accent: Rgba) -> AnyElement {
        if h.recent_warnings.is_empty() {
            return card(format!("{key}-warn").into(), "Recent Warnings", "skull", accent, empty_body("No recent warnings"));
        }
        let (handle, gutter) = self.list_handles(key, "warn");
        let items: Vec<AnyElement> = h
            .recent_warnings
            .iter()
            .enumerate()
            .map(|(i, w)| {
                div()
                    .px(px(8.))
                    .py(px(6.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(theme::hexa(0xf59e0b, 0.1))
                    .bg(theme::hexa(0xf59e0b, 0.05))
                    .text_xs_tw()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(6.))
                            .child(icon("alert-triangle", 12.).text_color(amber::A400))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(amber::A300)
                                    .child(w.reason.clone())
                                    .auto_id(format!("dashboard.warning.{i}"), "text", w.reason.clone()),
                            )
                            .when(w.count > 1, |d| {
                                d.child(div().ml_auto().flex_none().font_mono().text_10().text_color(slate::S500).child(format!("x{}", w.count)))
                            }),
                    )
                    .child(
                        div()
                            .mt(px(2.))
                            .text_color(slate::S400)
                            .line_height(relative(1.625))
                            .line_clamp(2)
                            .text_ellipsis()
                            .child(crate::ui::text::hyphen_wrap(&w.message)),
                    )
                    .child(
                        div()
                            .mt(px(4.))
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .text_10()
                            .line_height(px(15.))
                            .text_color(slate::S600)
                            .child(div().max_w(relative(0.6)).truncate().child(w.involved_object.clone()).auto_id(format!("dashboard.warning.{i}.object"), "text", w.involved_object.clone()))
                            .child(div().flex_none().child(last_seen_label(&w.last_seen)).auto_id(format!("dashboard.warning.{i}.seen"), "text", last_seen_label(&w.last_seen))),
                    )
                    .into_any_element()
            })
            .collect();
        card(
            format!("{key}-warn").into(),
            "Recent Warnings",
            "skull",
            accent,
            scroll_list(format!("{key}-warn-list").into(), &handle, &gutter, 8., items),
        )
    }

    /// The 6-card grid (`grid-cols-1 md:2 xl:3 gap-4`, Recent Warnings spans 2 at md).
    fn cards_grid(&mut self, key: &str, h: &ClusterHealth, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let accent = theme::accent(cx);
        let cards = vec![
            self.health_card(key, h, accent, cx),
            self.pod_card(key, h, accent, window, cx),
            self.node_card(key, h, accent),
            self.hotlist_card(key, h, accent, cx),
            self.pending_card(key, h, accent, cx),
            self.warnings_card(key, h, accent),
        ];
        let cols = grid_columns(window);
        // Rows of (cards, span of the last one) following CSS grid auto-placement.
        let mut rows: Vec<Vec<AnyElement>> = Vec::new();
        let mut it = cards.into_iter();
        match cols {
            3 => {
                let a: Vec<_> = it.by_ref().take(3).collect();
                let b: Vec<_> = it.collect();
                rows.push(a);
                rows.push(b);
            }
            2 => {
                let v: Vec<_> = it.collect();
                let mut v = v.into_iter();
                rows.push(v.by_ref().take(2).collect());
                rows.push(v.by_ref().take(2).collect());
                let fifth = v.next().unwrap();
                rows.push(vec![fifth, div().flex_1().min_w_0().into_any_element()]);
                rows.push(v.collect());
            }
            _ => rows.extend(it.map(|c| vec![c])),
        }
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .children(rows.into_iter().map(|r| div().flex().flex_row().gap(px(16.)).children(r)))
            .into_any_element()
    }
}

/// Closed annulus-sector path; angles in degrees, counter-clockwise from
/// 3 o'clock (Recharts `polarToCartesian`: y = cy - r·sin).
fn sector_path(c: Point<Pixels>, ri: f32, ro: f32, a0: f32, a1: f32) -> Option<Path<Pixels>> {
    let pt = |r: f32, a: f32| {
        let rad = a.to_radians();
        point(c.x + px(r * rad.cos()), c.y - px(r * rad.sin()))
    };
    let sweep = a1 - a0;
    let steps = ((sweep.abs() / 2.).ceil() as usize).max(2);
    let mut b = PathBuilder::fill();
    b.move_to(pt(ro, a0));
    for i in 1..=steps {
        b.line_to(pt(ro, a0 + sweep * i as f32 / steps as f32));
    }
    for i in (0..=steps).rev() {
        b.line_to(pt(ri, a0 + sweep * i as f32 / steps as f32));
    }
    b.close();
    b.build().ok()
}

fn sector_at(b: Bounds<Pixels>, p: Point<Pixels>, sectors: &[(f32, f32)]) -> Option<usize> {
    let c = b.center();
    let dx = f32::from(p.x - c.x);
    let dy = f32::from(c.y - p.y);
    let r = (dx * dx + dy * dy).sqrt();
    if !(35. ..=60.).contains(&r) {
        return None;
    }
    let mut a = dy.atan2(dx) * 180. / PI;
    if a < 0. {
        a += 360.;
    }
    sectors.iter().position(|(s, e)| a >= *s && a <= *e)
}

fn ease_out_quad(t: f32) -> f32 {
    1. - (1. - t) * (1. - t)
}

impl Render for DashboardView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        let root = div().size_full().relative();

        let Some(clusters) = self.clusters.clone() else {
            if self.loading {
                return root
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(slate::S400)
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .child(spinning_icon("dash-loading", "loader-2", 20., slate::S400))
                            .child("Loading cluster health..."),
                    )
                    .auto_id("view.dashboard", "region", "Dashboard");
            }
            let err = self.error.clone().unwrap_or_default();
            return root
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .text_color(slate::S400)
                        .child(icon("alert-triangle", 48.).text_color(theme::hexa(0xfbbf24, 0.5)).mb(px(16.)))
                        .child(div().text_lg_tw().text_center().child(err))
                        .child(
                            div()
                                .id("dash-retry")
                                .mt(px(16.))
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(4.))
                                .text_sm_tw()
                                .text_color(accent)
                                .cursor_pointer()
                                .child(icon("refresh-cw", 14.).text_color(accent))
                                .child("Retry")
                                .on_click(cx.listener(|this, _, _, cx| this.fetch(cx)))
                                .auto_id("dashboard.retry", "button", "Retry"),
                        ),
                )
                .auto_id("view.dashboard", "region", "Dashboard");
        };

        let multi = self.multi && clusters.iter().all(|c| c.context.is_some());
        let content: AnyElement = if multi {
            div()
                .flex()
                .flex_col()
                .gap(px(24.))
                .children(clusters.iter().enumerate().map(|(i, c)| {
                    let ctx = c.context.clone().unwrap_or_default();
                    let (text, base) = score_colors(c.health.score);
                    let grid = self.cards_grid(&format!("c{i}-{ctx}"), &c.health, window, cx);
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(12.))
                                .mb(px(12.))
                                .child(
                                    div()
                                        .text_base_tw()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(slate::S200)
                                        .font_mono()
                                        .child(ctx.clone()),
                                )
                                .child(
                                    div()
                                        .text_xs_tw()
                                        .font_mono()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .px(px(8.))
                                        .py(px(2.))
                                        .rounded_full()
                                        .border_1()
                                        .bg(theme::hexa(base, 0.15))
                                        .text_color(text)
                                        .border_color(theme::hexa(base, 0.3))
                                        .child(format!("{} - {}", c.health.score, score_label(c.health.score)))
                                        .auto_id(
                                            format!("dashboard.cluster.{ctx}.score"),
                                            "status",
                                            format!("{} - {}", c.health.score, score_label(c.health.score)),
                                        ),
                                )
                                .auto_id(format!("dashboard.cluster.{ctx}"), "heading", ctx.clone()),
                        )
                        .child(grid)
                }))
                .into_any_element()
        } else {
            match clusters.first() {
                Some(c) => {
                    let h = c.health.clone();
                    self.cards_grid("single", &h, window, cx)
                }
                None => div().into_any_element(),
            }
        };

        let gutter = self.gutter.overflows();
        root.flex()
            .flex_row()
            .child(
                div()
                    .id("dashboard-scroll")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .p(px(16.))
                    .child(div().child(content).with_animation("dash-fade", Animation::new(Duration::from_millis(300)), |el, t| el.opacity(t))),
            )
            .when(gutter, |d| d.child(div().w(px(scrollbar::WIDTH)).flex_none()))
            .child(scrollbar::vertical(self.scroll.clone(), self.gutter.clone()))
            .auto_id("view.dashboard", "region", "Dashboard")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        animated_sectors, breakdown_rows, css_ease, format_cpu, format_memory, last_seen_label, parse_kube_quantity, pie_sectors, pod_data,
        score_label, usage_percent, ClusterHealth,
    };
    use kore_core::state::dashboard::{NodeHealth, PodHealth, WarningEvent};

    #[test]
    fn quantities() {
        assert_eq!(parse_kube_quantity("8"), 8.);
        assert_eq!(parse_kube_quantity("16Ki"), 16. * 1024.);
        assert_eq!(parse_kube_quantity("250m"), 0.25);
        assert_eq!(parse_kube_quantity("abc"), 0.);
        assert_eq!(usage_percent("8", "8"), 100.);
        assert_eq!(usage_percent("16273476Ki", "16375876Ki"), 99.);
        assert_eq!(usage_percent("1", "3"), 33.);
        assert_eq!(usage_percent("", "3"), 0.);
        assert_eq!(usage_percent("5", "0"), 0.);
        assert_eq!(usage_percent("1", "8"), 13.); // 12.5 rounds half up
    }

    #[test]
    fn human_quantities_and_ages() {
        assert_eq!(format_cpu("250000000n"), "250m");
        assert_eq!(format_cpu("3800m"), "3.8");
        assert_eq!(format_cpu("8"), "8");
        assert_eq!(format_memory("16273476Ki"), "15.5Gi");
        assert_eq!(format_memory("1024Mi"), "1Gi");
        assert_eq!(format_memory("524288Ki"), "512Mi");
        assert_eq!(usage_percent("250000000n", "4"), 6.);
        let ts = (chrono::Utc::now() - chrono::Duration::minutes(5)).to_rfc3339();
        assert_eq!(last_seen_label(&ts), "5m ago");
        assert_eq!(last_seen_label(""), "-");
    }

    #[test]
    fn pie_angles_match_recharts() {
        let s = pie_sectors(&[1]);
        assert_eq!(s, vec![(0., 358.)]);
        let s = pie_sectors(&[1, 1]);
        assert_eq!(s, vec![(0., 178.), (180., 358.)]);
        let a = animated_sectors(&s, 0.5);
        assert_eq!(a, vec![(0., 89.), (91., 180.)]);
        assert_eq!(animated_sectors(&s, 1.), s);
        assert!((css_ease(0.5) - 0.8024).abs() < 0.01);
    }

    #[test]
    fn breakdown() {
        let h = ClusterHealth {
            score: 61,
            pods: PodHealth { running: 10, pending: 1, failed: 0, succeeded: 1, crash_looping: 1, total: 13 },
            nodes: vec![NodeHealth {
                name: "n".into(),
                status: "Ready".into(),
                cpu_capacity: "8".into(),
                memory_capacity: "1Ki".into(),
                cpu_allocatable: "8".into(),
                memory_allocatable: "1Ki".into(),
                cpu_usage: Some("1".into()),
                memory_usage: None,
            }],
            restart_hotlist: vec![],
            pending_pods: vec![],
            recent_warnings: vec![
                WarningEvent {
                    reason: "BackOff".into(),
                    message: String::new(),
                    involved_object: String::new(),
                    namespace: String::new(),
                    count: 1,
                    last_seen: String::new(),
                };
                3
            ],
        };
        let rows = breakdown_rows(&h);
        let v: Vec<(&str, String, String)> = rows.iter().map(|r| (r.label, r.detail.clone(), r.value.clone())).collect();
        assert_eq!(
            v,
            vec![
                ("Node health", "1/1 ready".into(), "+30.0".into()),
                ("Pod health", "10/13 running".into(), "+30.8".into()),
                ("Base score", "".into(), "+30.0".into()),
                ("CrashLoop penalty", "1 pod".into(), "-5.0".into()),
                ("Pending penalty", "1 pod".into(), "-2.0".into()),
                ("Warning penalty", "3 events".into(), "-1.5".into()),
            ]
        );
        assert_eq!(score_label(80), "Healthy");
        assert_eq!(score_label(79), "Degraded");
        assert_eq!(score_label(49), "Critical");
        assert_eq!(pod_data(&h).iter().map(|d| d.0).collect::<Vec<_>>(), vec!["Running", "Pending", "CrashLooping", "Succeeded"]);
    }
}
