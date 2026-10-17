//! Pod Metrics tab (spec 02 §7): CPU/memory cards with ring gauges and the
//! 2-minute trend chart (Recharts `LineChart` redrawn on a canvas).

use crate::automation::Automation;
use crate::backend;
use crate::state::resource_item::ResourceItem;
use crate::state::AppState;
use crate::theme::{self, slate, KoreStyled};
use crate::ui::icon::icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use serde_json::Value;
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

// ── Pure logic (unit-tested) ─────────────────────────────────────────────

/// Parse a Kubernetes quantity into its base unit (cores for CPU, bytes for
/// memory). Binary (`Ki`…`Ei`) and decimal (`n u m k M G T P E`) suffixes and
/// exponent notation (`12e6`) are supported; anything unparsable is 0.
pub fn parse_base_quantity(q: Option<&str>) -> f64 {
    let Some(q) = q.map(str::trim).filter(|q| !q.is_empty()) else { return 0. };
    const SUFFIXES: [(&str, f64); 15] = [
        ("Ki", 1024.),
        ("Mi", 1_048_576.),
        ("Gi", 1_073_741_824.),
        ("Ti", 1_099_511_627_776.),
        ("Pi", 1_125_899_906_842_624.),
        ("Ei", 1_152_921_504_606_846_976.),
        ("n", 1e-9),
        ("u", 1e-6),
        ("m", 1e-3),
        ("k", 1e3),
        ("M", 1e6),
        ("G", 1e9),
        ("T", 1e12),
        ("P", 1e15),
        ("E", 1e18),
    ];
    let num = |s: &str| s.parse::<f64>().ok().filter(|v| v.is_finite());
    // Plain numbers first, so exponent notation (`1e3`) is not read as a suffix.
    if let Some(v) = num(q) {
        return v;
    }
    for (suf, mult) in SUFFIXES {
        if let Some(v) = q.strip_suffix(suf).and_then(num) {
            return v * mult;
        }
    }
    0.
}

/// CPU quantity in millicores (`"2"` = 2000, `"250m"` = 250, `"1500000n"` = 1.5).
pub fn parse_cpu(q: Option<&str>) -> f64 {
    parse_base_quantity(q) * 1000.
}

/// Memory quantity in bytes (`"1024"` = 1024, `"1Mi"` = 1048576, `"1G"` = 1e9).
pub fn parse_memory(q: Option<&str>) -> f64 {
    parse_base_quantity(q)
}

/// JS `Number.prototype.toFixed` (round half away from zero on the decimal repr).
pub fn to_fixed(v: f64, digits: usize) -> String {
    format!("{:.*}", digits, v)
}

pub fn format_bytes(bytes: f64) -> String {
    if bytes == 0. {
        return "0 B".into();
    }
    const SIZES: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let i = (bytes.ln() / 1024f64.ln()).floor() as i32;
    let unit = SIZES.get(i.max(0) as usize).copied().unwrap_or("undefined");
    format!("{} {}", to_fixed(bytes / 1024f64.powi(i), 2), unit)
}

pub fn format_cpu(m: f64) -> String {
    if m == 0. {
        "0 m".into()
    } else if m < 1000. {
        format!("{} m", to_fixed(m, 0))
    } else {
        format!("{} cores", to_fixed(m / 1000., 2))
    }
}

/// Limits from the pod JSON: `status.allocatedResources`, else the sum of
/// container limits (falling back to requests).
pub fn pod_limits(pod: Option<&Value>) -> (f64, f64) {
    let Some(pod) = pod else { return (0., 0.) };
    if let Some(a) = pod["status"]["allocatedResources"].as_object() {
        return (
            parse_cpu(a.get("cpu").and_then(Value::as_str)),
            parse_memory(a.get("memory").and_then(Value::as_str)),
        );
    }
    let (mut cpu, mut mem) = (0., 0.);
    for c in pod["spec"]["containers"].as_array().into_iter().flatten() {
        let r = &c["resources"];
        let pick = |k: &str| r["limits"][k].as_str().filter(|s| !s.is_empty()).or(r["requests"][k].as_str().filter(|s| !s.is_empty()));
        if let Some(v) = pick("cpu") {
            cpu += parse_cpu(Some(v));
        }
        if let Some(v) = pick("memory") {
            mem += parse_memory(Some(v));
        }
    }
    (cpu, mem)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Usage {
    pub cpu: f64,
    pub cpu_limit: f64,
    pub mem: f64,
    pub mem_limit: f64,
}

/// Sum container usage and resolve limits with the usage×2 fallbacks.
pub fn compute_usage(metrics: &Value, limits: (f64, f64)) -> Usage {
    let (mut cpu, mut mem) = (0., 0.);
    for c in metrics["containers"].as_array().into_iter().flatten() {
        cpu += parse_cpu(c["usage"]["cpu"].as_str());
        mem += parse_memory(c["usage"]["memory"].as_str());
    }
    let cpu_limit = if limits.0 > 0. { limits.0 } else if cpu > 0. { cpu * 2. } else { 1000. };
    let mem_limit = if limits.1 > 0. { limits.1 } else if mem > 0. { mem * 2. } else { 512. * 1024. * 1024. };
    Usage { cpu, cpu_limit, mem, mem_limit }
}

/// Error string → displayed message.
pub fn map_error(e: &str) -> String {
    if e.contains("404") || e.contains("not found") {
        if e.contains("Metrics Server") || e.contains("metrics.k8s.io") {
            "Metrics Server not detected".into()
        } else {
            format!("Pod metrics not found: {e}")
        }
    } else if e.contains("Metrics Server") {
        "Metrics Server not detected".into()
    } else {
        format!("Error fetching metrics: {e}")
    }
}

/// Gauge stroke color by percentage.
pub fn gauge_color(p: f64) -> Rgba {
    if p > 80. {
        theme::hex(0xef4444)
    } else if p > 60. {
        theme::hex(0xf59e0b)
    } else {
        theme::hex(0x10b981)
    }
}

/// d3 `curveMonotoneX` tangents for points sorted by x.
pub fn monotone_tangents(p: &[(f32, f32)]) -> Vec<f32> {
    let n = p.len();
    let mut t = vec![0f32; n];
    if n < 2 {
        return t;
    }
    let sign = |v: f32| if v < 0. { -1. } else if v > 0. { 1. } else { 0. };
    let slope3 = |a: (f32, f32), b: (f32, f32), c: (f32, f32)| {
        let h0 = b.0 - a.0;
        let h1 = c.0 - b.0;
        let s0 = (b.1 - a.1) / if h0 != 0. { h0 } else if h1 < 0. { -0. } else { 0. };
        let s1 = (c.1 - b.1) / if h1 != 0. { h1 } else if h0 < 0. { -0. } else { 0. };
        let pp = (s0 * h1 + s1 * h0) / (h0 + h1);
        let v = (sign(s0) + sign(s1)) * s0.abs().min(s1.abs()).min(0.5 * pp.abs());
        if v.is_finite() { v } else { 0. }
    };
    let slope2 = |a: (f32, f32), b: (f32, f32), tt: f32| {
        let h = b.0 - a.0;
        if h != 0. { (3. * (b.1 - a.1) / h - tt) / 2. } else { tt }
    };
    if n == 2 {
        let s = (p[1].1 - p[0].1) / (p[1].0 - p[0].0).max(f32::EPSILON);
        return vec![s, s];
    }
    for i in 1..n - 1 {
        t[i] = slope3(p[i - 1], p[i], p[i + 1]);
    }
    t[0] = slope2(p[0], p[1], t[1]);
    t[n - 1] = slope2(p[n - 2], p[n - 1], t[n - 2]);
    t
}

/// Recharts `interval="preserveEnd"` tick selection for centered labels of
/// width `w` at positions `xs`, with `min_gap` (5px).
pub fn preserve_end_ticks(xs: &[f32], w: f32, min_gap: f32) -> Vec<usize> {
    let mut out = Vec::new();
    let mut last_start: Option<f32> = None;
    for i in (0..xs.len()).rev() {
        let start = xs[i] - w / 2.;
        let end = xs[i] + w / 2.;
        if last_start.is_none_or(|ls| end + min_gap <= ls) {
            out.push(i);
            last_start = Some(start);
        }
    }
    out.reverse();
    out
}

// ── View ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct Point {
    cpu: f64,
    mem: f64,
    label: SharedString,
    at: std::time::Instant,
}

pub struct PodMetrics {
    #[allow(dead_code)]
    app: Entity<AppState>,
    item: ResourceItem,
    namespace: String,
    pod: Option<Value>,
    usage: Option<Usage>,
    history: Vec<Point>,
    error: Option<String>,
    loading: bool,
    poll: Option<Task<()>>,
    describe: Option<Task<()>>,
    hover: Rc<Cell<Option<usize>>>,
}

impl PodMetrics {
    pub fn new(app: Entity<AppState>, item: ResourceItem, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let namespace = if item.namespace.is_empty() { "default".to_string() } else { item.namespace.clone() };
        let mut this = PodMetrics {
            app,
            item,
            namespace,
            pod: None,
            usage: None,
            history: Vec::new(),
            error: None,
            loading: true,
            poll: None,
            describe: None,
            hover: Rc::default(),
        };
        let (ns, name) = (this.namespace.clone(), this.item.name.clone());
        this.describe = Some(backend::run(
            cx,
            move |core| async move { core.describe_pod(ns, name).await },
            |this: &mut Self, res, cx| {
                if let Ok(v) = res {
                    this.pod = Some(v);
                    // Metrics fetching depends on the pod spec, so polling restarts
                    // (immediate fetch) when it arrives.
                    this.start_polling(cx);
                }
            },
        ));
        this.start_polling(cx);
        this
    }

    fn start_polling(&mut self, cx: &mut Context<Self>) {
        let core = backend::core(cx);
        let (ns, name) = (self.namespace.clone(), self.item.name.clone());
        self.poll = Some(cx.spawn(async move |this, cx| loop {
            let res = core.get_pod_metrics(ns.clone(), name.clone()).await;
            let ok = this.update(cx, |t, cx| t.apply(res, cx)).is_ok();
            if !ok {
                break;
            }
            let timer = cx.update(|cx| backend::sleep(cx, Duration::from_millis(5000)));
            timer.await;
        }));
    }

    fn apply(&mut self, res: Result<Value, kore_core::K8sError>, cx: &mut Context<Self>) {
        self.loading = false;
        match res {
            Ok(m) => {
                let u = compute_usage(&m, pod_limits(self.pod.as_ref()));
                self.usage = Some(u);
                self.error = None;
                let now = std::time::Instant::now();
                self.history.push(Point {
                    cpu: u.cpu / u.cpu_limit * 100.,
                    mem: u.mem / u.mem_limit * 100.,
                    label: chrono::Local::now().format("%H:%M:%S").to_string().into(),
                    at: now,
                });
                self.history.retain(|p| now.duration_since(p.at) < Duration::from_millis(120_000));
            }
            Err(e) => {
                self.error = Some(map_error(&backend::format_error(&e)));
                self.usage = None;
            }
        }
        cx.notify();
    }

    fn gauge_card(title: &'static str, sub: String, pct: f64, accent: Rgba) -> impl IntoElement {
        let color = gauge_color(pct);
        let id = format!("metrics.{}", if title.starts_with("CPU") { "cpu" } else { "memory" });
        card()
            .flex_1()
            .min_w_0()
            .child(
                div()
                    .mb(px(16.))
                    .child(div().mb(px(4.)).text_sm_tw().font_weight(FontWeight::SEMIBOLD).text_color(slate::S300).child(title))
                    .child(div().text_xs_tw().text_color(slate::S400).child(sub.clone()).auto_id(format!("{id}.usage"), "text", sub)),
            )
            .child(
                div()
                    .relative()
                    .h(px(128.))
                    .w_full()
                    .child(
                        canvas(
                            |_, _, _| {},
                            move |b, _, window, _| paint_gauge(b, pct, color, window),
                        )
                        .absolute()
                        .inset_0(),
                    )
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .text_2xl_tw()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(accent)
                                    .child(format!("{}%", to_fixed(pct, 1)))
                                    .auto_id(format!("{id}.percent"), "text", format!("{}%", to_fixed(pct, 1))),
                            ),
                    ),
            )
            .auto_id(id, "region", title)
    }

    fn chart(&self) -> impl IntoElement {
        let pts: Rc<Vec<Point>> = Rc::new(self.history.clone());
        let hover = self.hover.clone();
        let hover2 = self.hover.clone();
        let pts2 = pts.clone();
        canvas(
            move |bounds, window, _| {
                let hb = window.insert_hitbox(bounds, HitboxBehavior::Normal);
                (hb, layout_chart(bounds, &pts2, window))
            },
            move |bounds, (hb, lay), window, cx| {
                paint_chart(bounds, &pts, &lay, hover.get(), window, cx);
                let (h, l) = (hover2.clone(), lay.clone());
                window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, _| {
                    if phase != DispatchPhase::Bubble {
                        return;
                    }
                    let next = if hb.is_hovered(window) { l.nearest(e.position.x) } else { None };
                    if next != h.get() {
                        h.set(next);
                        window.refresh();
                    }
                });
            },
        )
        .w_full()
        .h(px(200.))
    }
}

fn card() -> Div {
    div().bg(theme::hexa(0x101828, 0.3)).border_1().border_color(slate::S800).rounded(px(8.)).p(px(16.))
}

fn paint_gauge(b: Bounds<Pixels>, pct: f64, color: Rgba, window: &mut Window) {
    // viewBox 100×100, preserveAspectRatio meet, centered.
    let s = f32::from(b.size.width.min(b.size.height)) / 100.;
    let c = b.center();
    let r = 40. * s;
    let w = 8. * s;
    let circle = |from: f32, sweep: f32| {
        let mut pb = PathBuilder::stroke(px(w));
        let steps = ((sweep.abs() / std::f32::consts::TAU) * 180.).ceil().max(2.) as usize;
        for i in 0..=steps {
            let a = from + sweep * i as f32 / steps as f32;
            let p = point(c.x + px(r * a.cos()), c.y + px(r * a.sin()));
            if i == 0 {
                pb.move_to(p);
            } else {
                pb.line_to(p);
            }
        }
        pb.build().ok()
    };
    let start = -std::f32::consts::FRAC_PI_2;
    if let Some(p) = circle(0., std::f32::consts::TAU) {
        window.paint_path(p, theme::hexa(0x1e293b, 0.3));
    }
    let frac = (pct.min(100.) / 100.) as f32;
    if frac > 0. {
        let sweep = std::f32::consts::TAU * frac;
        if let Some(p) = circle(start, sweep) {
            window.paint_path(p, color);
        }
        // Round line caps.
        for a in [start, start + sweep] {
            let ctr = point(c.x + px(r * a.cos()), c.y + px(r * a.sin()));
            window.paint_quad(fill(Bounds::centered_at(ctr, size(px(w), px(w))), color).corner_radii(px(w / 2.)));
        }
    }
}

const TICK_FONT: f32 = 10.;
const AXIS: u32 = 0x94a3b8;

#[derive(Clone)]
struct ChartLayout {
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
    xs: Vec<f32>,
    label_w: f32,
}

impl ChartLayout {
    fn nearest(&self, x: Pixels) -> Option<usize> {
        let x = f32::from(x);
        if self.xs.is_empty() || x < self.left - 1. || x > self.right + 1. {
            return None;
        }
        self.xs.iter().enumerate().min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs())).map(|(i, _)| i)
    }
}

fn layout_chart(b: Bounds<Pixels>, pts: &[Point], window: &mut Window) -> ChartLayout {
    // Recharts margins 5, YAxis width 60, XAxis height 30.
    let ox = f32::from(b.origin.x);
    let oy = f32::from(b.origin.y);
    let w = f32::from(b.size.width);
    let left = ox + 5. + 60.;
    let right = ox + w - 5.;
    let top = oy + 5.;
    let bottom = oy + 200. - 5. - 30.;
    let n = pts.len();
    let xs = (0..n)
        .map(|i| if n == 1 { (left + right) / 2. } else { left + (right - left) * i as f32 / (n - 1) as f32 })
        .collect();
    let label_w = pts.first().map(|p| crate::ui::text::measure(window, &p.label, theme::SANS, TICK_FONT, FontWeight::NORMAL)).unwrap_or(40.);
    ChartLayout { left, right, top, bottom, xs, label_w }
}

fn text_line(window: &mut Window, text: &str, color: Rgba, size: f32, weight: FontWeight) -> ShapedLine {
    let run = TextRun {
        len: text.len(),
        font: Font { family: theme::SANS.into(), weight, ..font(theme::SANS) },
        color: color.into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window.text_system().shape_line(SharedString::from(text.to_string()), px(size), &[run], None)
}

fn dashed(from: gpui_kit::Point<Pixels>, to: gpui_kit::Point<Pixels>, color: Rgba, window: &mut Window) {
    let mut pb = PathBuilder::stroke(px(1.)).dash_array(&[px(3.), px(3.)]);
    pb.move_to(from);
    pb.line_to(to);
    if let Ok(p) = pb.build() {
        window.paint_path(p, color);
    }
}

fn solid(from: gpui_kit::Point<Pixels>, to: gpui_kit::Point<Pixels>, color: Rgba, window: &mut Window) {
    let mut pb = PathBuilder::stroke(px(1.));
    pb.move_to(from);
    pb.line_to(to);
    if let Ok(p) = pb.build() {
        window.paint_path(p, color);
    }
}

fn paint_chart(_b: Bounds<Pixels>, pts: &[Point], l: &ChartLayout, hover: Option<usize>, window: &mut Window, cx: &mut App) {
    let grid = theme::hex(0x334155);
    let axis = theme::hex(AXIS);
    let y_of = |v: f64| l.bottom - (l.bottom - l.top) * (v.clamp(-1e9, 1e9) as f32 / 100.);
    let lh = px(TICK_FONT * 1.2);
    // Grid.
    for t in [0., 25., 50., 75., 100.] {
        let y = y_of(t);
        dashed(point(px(l.left), px(y)), point(px(l.right), px(y)), grid, window);
    }
    for &x in &l.xs {
        dashed(point(px(x), px(l.top)), point(px(x), px(l.bottom)), grid, window);
    }
    // Y axis: line, ticks, labels.
    solid(point(px(l.left), px(l.top)), point(px(l.left), px(l.bottom)), axis, window);
    for t in [0, 25, 50, 75, 100] {
        let y = y_of(t as f64);
        solid(point(px(l.left - 6.), px(y)), point(px(l.left), px(y)), axis, window);
        let s = t.to_string();
        let line = text_line(window, &s, axis, TICK_FONT, FontWeight::NORMAL);
        let w = f32::from(line.width);
        let _ = line.paint(point(px(l.left - 8. - w), px(y) - lh / 2.), lh, TextAlign::Left, None, window, cx);
    }
    // `%` label rotated -90° at insideLeft (drawn as paths).
    let lx = l.left - 60. + 5. + 5.;
    let cy = (l.top + l.bottom) / 2.;
    paint_rotated_percent(lx, cy, axis, window);
    // X axis: line, ticks, labels (preserveEnd).
    solid(point(px(l.left), px(l.bottom)), point(px(l.right), px(l.bottom)), axis, window);
    for &x in &l.xs {
        solid(point(px(x), px(l.bottom)), point(px(x), px(l.bottom + 6.)), axis, window);
    }
    let chart_right = l.right + 5.;
    for i in preserve_end_ticks(&l.xs, l.label_w, 5.) {
        let line = text_line(window, &pts[i].label, axis, TICK_FONT, FontWeight::NORMAL);
        let w = f32::from(line.width);
        let mut x = l.xs[i] - w / 2.;
        if x + w > chart_right {
            x = chart_right - w;
        }
        let _ = line.paint(point(px(x), px(l.bottom + 8.)), lh, TextAlign::Left, None, window, cx);
    }
    // Lines.
    for (key, color) in [(0, theme::hex(0x10b981)), (1, theme::hex(0x3b82f6))] {
        let p: Vec<(f32, f32)> =
            pts.iter().enumerate().map(|(i, pt)| (l.xs[i], y_of(if key == 0 { pt.cpu } else { pt.mem }))).collect();
        if p.len() < 2 {
            continue;
        }
        let t = monotone_tangents(&p);
        let mut pb = PathBuilder::stroke(px(2.));
        pb.move_to(point(px(p[0].0), px(p[0].1)));
        for i in 0..p.len() - 1 {
            let (x0, y0) = p[i];
            let (x1, y1) = p[i + 1];
            let dx = (x1 - x0) / 3.;
            pb.cubic_bezier_to(
                point(px(x1), px(y1)),
                point(px(x0 + dx), px(y0 + dx * t[i])),
                point(px(x1 - dx), px(y1 - dx * t[i + 1])),
            );
        }
        if let Ok(path) = pb.build() {
            window.paint_path(path, color);
        }
    }
    // Tooltip.
    if let Some(i) = hover.filter(|i| *i < pts.len()) {
        let x = l.xs[i];
        solid(point(px(x), px(l.top)), point(px(x), px(l.bottom)), theme::hex(0xcccccc), window);
        let rows = [
            (format!("CPU % : {}", pts[i].cpu), theme::hex(0x10b981)),
            (format!("Memory % : {}", pts[i].mem), theme::hex(0x3b82f6)),
        ];
        let label = text_line(window, &pts[i].label, theme::hex(AXIS), 16., FontWeight::NORMAL);
        let lines: Vec<ShapedLine> = rows.iter().map(|(t, c)| text_line(window, t, *c, 16., FontWeight::NORMAL)).collect();
        let lh = px(24.);
        let w = lines.iter().map(|l| f32::from(l.width)).fold(f32::from(label.width), f32::max) + 20.;
        let h = 10. + 24. + 2. * (24. + 4.) + 10.;
        let mut bx = x + 10.;
        if bx + w > l.right {
            bx = x - 10. - w;
        }
        let by = l.top + 10.;
        let r = Bounds::new(point(px(bx), px(by)), size(px(w), px(h)));
        window.paint_quad(
            fill(r, theme::hex(0x0f172a)).corner_radii(px(4.)).border_widths(px(1.)).border_color(theme::hex(0x334155)),
        );
        let _ = label.paint(point(px(bx + 10.), px(by + 10.)), lh, TextAlign::Left, None, window, cx);
        for (k, line) in lines.iter().enumerate() {
            let _ = line.paint(point(px(bx + 10.), px(by + 10. + 24. + 4. + k as f32 * 28.)), lh, TextAlign::Left, None, window, cx);
        }
    }
}

/// A 10px `%` glyph rotated -90° (Recharts YAxis label), drawn as paths.
fn paint_rotated_percent(cx_: f32, cy: f32, color: Rgba, window: &mut Window) {
    // Unrotated glyph box ~8×7 (w×h); rotated, it's 7 wide × 8 tall.
    let ring = |x: f32, y: f32, window: &mut Window| {
        let mut pb = PathBuilder::stroke(px(0.9));
        for i in 0..=16 {
            let a = std::f32::consts::TAU * i as f32 / 16.;
            let p = point(px(x + 1.3 * a.cos()), px(y + 1.5 * a.sin()));
            if i == 0 {
                pb.move_to(p);
            } else {
                pb.line_to(p);
            }
        }
        if let Ok(p) = pb.build() {
            window.paint_path(p, color);
        }
    };
    // Rotated: the top-left ring ends up bottom-left, the bottom-right ring top-right.
    ring(cx_ - 1.8, cy + 2.2, window);
    ring(cx_ + 1.8, cy - 2.2, window);
    let mut pb = PathBuilder::stroke(px(0.9));
    pb.move_to(point(px(cx_ - 3.), px(cy - 3.5)));
    pb.line_to(point(px(cx_ + 3.), px(cy + 3.5)));
    if let Ok(p) = pb.build() {
        window.paint_path(p, color);
    }
}

impl Render for PodMetrics {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        if self.loading && self.usage.is_none() && self.error.is_none() {
            return div()
                .size_full()
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
                        .child(icon("activity", 20.).text_color(slate::S400).with_animation(
                            "metrics-pulse",
                            Animation::new(Duration::from_millis(2000)).repeat(),
                            |s, t| s.opacity(1. - 0.5 * (1. - (2. * t - 1.).abs())),
                        ))
                        .child("Loading metrics..."),
                )
                .auto_id("view.pod-metrics", "region", "Loading metrics...")
                .into_any_element();
        }
        if let Some(err) = &self.error {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .text_color(slate::S400)
                        .child(icon("activity", 48.).text_color(slate::S400).opacity(0.5).mb(px(16.)))
                        .child(div().text_lg_tw().child(err.clone()))
                        .child(
                            div()
                                .mt(px(8.))
                                .text_sm_tw()
                                .text_color(slate::S500)
                                .child("Make sure the Metrics Server is installed in your cluster"),
                        ),
                )
                .auto_id("view.pod-metrics", "region", err.clone())
                .into_any_element();
        }
        let Some(u) = self.usage else { return div().into_any_element() };
        let cpu_p = u.cpu / u.cpu_limit * 100.;
        let mem_p = u.mem / u.mem_limit * 100.;
        let wide = true; // md:grid-cols-2 (window ≥ 768px; the app's minimum is wider)
        div()
            .id("pod-metrics-scroll")
            .size_full()
            .overflow_y_scroll()
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(24.))
            .child(
                div()
                    .flex()
                    .when(wide, |d| d.flex_row())
                    .when(!wide, |d| d.flex_col())
                    .gap(px(16.))
                    .child(Self::gauge_card(
                        "CPU Usage",
                        format!("{} / {}", format_cpu(u.cpu), format_cpu(u.cpu_limit)),
                        cpu_p,
                        accent,
                    ))
                    .child(Self::gauge_card(
                        "Memory Usage",
                        format!("{} / {}", format_bytes(u.mem), format_bytes(u.mem_limit)),
                        mem_p,
                        accent,
                    )),
            )
            .when(!self.history.is_empty(), |d| {
                d.child(
                    card()
                        .child(
                            div()
                                .mb(px(16.))
                                .text_sm_tw()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(slate::S300)
                                .child("Usage Trend (Last 2 Minutes)"),
                        )
                        .child(self.chart())
                        .auto_id("metrics.trend", "img", "Usage Trend (Last 2 Minutes)"),
                )
            })
            .auto_id("view.pod-metrics", "region", "Metrics")
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        compute_usage, format_bytes, format_cpu, map_error, monotone_tangents, parse_cpu, parse_memory, pod_limits, preserve_end_ticks,
        to_fixed,
    };
    use serde_json::json;

    #[test]
    fn quantities() {
        assert_eq!(parse_cpu(None), 0.);
        assert_eq!(parse_cpu(Some("250m")), 250.);
        assert!((parse_cpu(Some("1500000n")) - 1.5).abs() < 1e-9);
        assert_eq!(parse_cpu(Some("2")), 2000.); // cores
        assert_eq!(parse_cpu(Some("0.5")), 500.); // fractional cores
        assert!((parse_cpu(Some("100u")) - 0.1).abs() < 1e-9);
        assert_eq!(parse_memory(Some("128Ki")), 131072.);
        assert_eq!(parse_memory(Some("1Mi")), 1048576.);
        // Plain memory numbers are bytes (previously multiplied by 1000).
        assert_eq!(parse_memory(Some("1024")), 1024.);
        assert_eq!(parse_memory(Some("1G")), 1e9);
        assert_eq!(parse_memory(Some("1k")), 1000.);
        assert_eq!(parse_memory(Some("12e6")), 12e6);
        assert_eq!(parse_memory(Some("1E")), 1e18);
        assert_eq!(parse_memory(Some("abc")), 0.);
        assert_eq!(parse_memory(Some("")), 0.);
    }

    #[test]
    fn formatting() {
        assert_eq!(format_cpu(0.), "0 m");
        assert_eq!(format_cpu(2.4), "2 m");
        assert_eq!(format_cpu(1500.), "1.50 cores");
        assert_eq!(format_bytes(0.), "0 B");
        assert_eq!(format_bytes(335872.), "328.00 KB");
        assert_eq!(format_bytes(12.34 * 1024. * 1024.), "12.34 MB");
        assert_eq!(to_fixed(50., 1), "50.0");
    }

    #[test]
    fn usage_and_limits() {
        let m = json!({"containers": [{"usage": {"cpu": "1500000n", "memory": "328Ki"}}]});
        let u = compute_usage(&m, (0., 0.));
        assert_eq!(u.cpu_limit, u.cpu * 2.);
        assert_eq!(u.mem_limit, u.mem * 2.);
        let empty = compute_usage(&json!({}), (0., 0.));
        assert_eq!(empty.cpu_limit, 1000.);
        assert_eq!(empty.mem_limit, 512. * 1024. * 1024.);
        let pod = json!({"spec": {"containers": [
            {"resources": {"limits": {"cpu": "500m"}, "requests": {"memory": "64Mi"}}},
            {"resources": {"requests": {"cpu": "250m"}}}
        ]}});
        assert_eq!(pod_limits(Some(&pod)), (750., 64. * 1048576.));
        let alloc = json!({"status": {"allocatedResources": {"cpu": "1", "memory": "1Gi"}}, "spec": {"containers": []}});
        assert_eq!(pod_limits(Some(&alloc)), (1000., 1073741824.));
        assert_eq!(pod_limits(None), (0., 0.));
        // Plain byte limits stay bytes.
        let bytes = json!({"spec": {"containers": [{"resources": {"limits": {"cpu": "1", "memory": "268435456"}}}]}});
        assert_eq!(pod_limits(Some(&bytes)), (1000., 268435456.));
        let u = compute_usage(&json!({"containers": [{"usage": {"cpu": "0", "memory": "1048576"}}]}), (0., 0.));
        assert_eq!(u.mem, 1048576.);
    }

    #[test]
    fn errors() {
        assert_eq!(map_error("Kubernetes error: 404 page not found (metrics.k8s.io)"), "Metrics Server not detected");
        assert_eq!(map_error("Validation error: Metrics Server unavailable"), "Metrics Server not detected");
        assert_eq!(map_error("pod not found"), "Pod metrics not found: pod not found");
        assert_eq!(map_error("boom"), "Error fetching metrics: boom");
    }

    #[test]
    fn chart_math() {
        // Flat line → zero tangents.
        assert_eq!(monotone_tangents(&[(0., 5.), (10., 5.), (20., 5.)]), vec![0., 0., 0.]);
        // Monotone: tangent at a local extremum is 0.
        let t = monotone_tangents(&[(0., 0.), (10., 10.), (20., 0.)]);
        assert_eq!(t[1], 0.);
        // preserveEnd keeps the last label and drops overlapping ones.
        assert_eq!(preserve_end_ticks(&[0., 10., 20., 100.], 40., 5.), vec![2, 3]);
        assert_eq!(preserve_end_ticks(&[0., 50., 100.], 40., 5.), vec![0, 1, 2]);
    }
}
