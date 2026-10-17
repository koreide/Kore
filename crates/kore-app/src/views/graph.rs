//! Resource dependency graph (Task 6; spec 03 §2): hierarchical layout,
//! SVG-like canvas rendering (dot grid, bezier edges with arrowheads, node
//! cards), hover highlight + tooltip, wheel zoom, drag pan, Fit, kind chips and
//! name filter, legend. Node click navigates to the details view.

use crate::automation::{registry, Automation};
use crate::backend;
use crate::state::resource_item::ResourceItem;
use crate::state::types::ResourceKind;
use crate::state::AppState;
use crate::theme::{self, red, slate, KoreStyled, MONO, SANS};
use crate::ui::icon::icon;
use crate::ui::input::{InputEvent, InputStyle, TextInput};
use crate::ui::mono_text::{self, mono_text};
use crate::ui::text::measure;
use crate::ui::toast::{toast, ToastKind};
use crate::ui::widgets::spinning_icon;
use crate::views::rbac::{forbidden_banner, ForbiddenBanner};
use crate::views::graph_layout::{self as gl, Layout, Vis, NODE_HEIGHT, NODE_WIDTH};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use kore_core::state::graph::ResourceGraph;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

/// GPUI renders the system mono font ~2.5% wider than the 0.601em design pitch;
/// mono text is drawn at this fraction of its nominal size so widths (chips,
/// labels) match.
const MONO_FIX: f32 = 0.975;

/// Graph data + derived layout, shared with the paint closure.
struct Model {
    graph: ResourceGraph,
    layout: Layout,
    /// node id → index into `layout.nodes`
    index: HashMap<String, usize>,
    /// Lucide SVG bytes per icon, re-stroked at 1.5 (`strokeWidth={1.5}`).
    icons: HashMap<&'static str, Rc<Vec<u8>>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Transform {
    x: f32,
    y: f32,
    s: f32,
}

pub struct GraphView {
    app: Entity<AppState>,
    namespace: Option<String>,
    model: Option<Rc<Model>>,
    loading: bool,
    error: Option<String>,
    t: Transform,
    /// (mouse start, transform at start)
    pan: Option<(Point<Pixels>, Transform)>,
    /// Node under the mouse at mousedown (click = down+up on the same node).
    down_node: Option<usize>,
    hovered: Option<usize>,
    /// Last pointer position seen by mouse-move (GPUI re-dispatches moves at an
    /// unchanged position after a frame; SVG mouseenter only fires on real motion).
    last_move: Option<Point<Pixels>>,
    disabled: HashSet<String>,
    query: String,
    input: Entity<TextInput>,
    bounds: Rc<Cell<Bounds<Pixels>>>,
    last_node_count: usize,
    load_task: Option<Task<()>>,
    fit_task: Option<Task<()>>,
    /// "Permission Denied" analysis of a 403 load error (spec 03 §7.8).
    forbidden: Entity<ForbiddenBanner>,
    _subs: Vec<Subscription>,
}

impl GraphView {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            TextInput::new(
                "Filter by name...",
                InputStyle {
                    text_color: slate::S300,
                    placeholder_color: slate::S600,
                    font_family: MONO.into(),
                    font_size: 11. * MONO_FIX,
                    line_height: 16.5,
                    padding: (24., 24., 4., 4.),
                    ..Default::default()
                },
                window,
                cx,
            )
        });
        let namespace = ns_of(app.read(cx));
        let (forbidden, forbidden_sub) = forbidden_banner("graph.forbidden", cx);
        let subs = vec![
            forbidden_sub,
            cx.observe(&app, |this: &mut Self, app, cx| {
                let ns = ns_of(app.read(cx));
                if ns != this.namespace {
                    this.namespace = ns;
                    this.load(cx);
                }
            }),
            cx.subscribe(&input, |this, _, ev: &InputEvent, cx| {
                if let InputEvent::Change(s) = ev {
                    this.query = s.clone();
                }
                cx.notify();
            }),
        ];
        let mut v = GraphView {
            app,
            namespace,
            model: None,
            loading: true,
            error: None,
            t: Transform { x: 0., y: 0., s: 1. },
            pan: None,
            down_node: None,
            hovered: None,
            last_move: None,
            disabled: HashSet::new(),
            query: String::new(),
            input,
            bounds: Rc::new(Cell::new(Bounds::default())),
            last_node_count: 0,
            load_task: None,
            fit_task: None,
            forbidden,
            _subs: subs,
        };
        v.load(cx);
        v
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        let ns = self.namespace.clone();
        self.load_task = Some(backend::run(
            cx,
            move |core| async move { core.build_resource_graph(ns).await },
            |this: &mut Self, res, cx| {
                this.loading = false;
                match res {
                    Ok(g) => {
                        let layout = gl::compute_layout(&g.nodes, &g.edges);
                        let index = layout.nodes.iter().enumerate().map(|(i, n)| (n.node.id.clone(), i)).collect();
                        this.hovered = None;
                        this.model = Some(Rc::new(Model { graph: g, layout, index, icons: load_icons() }));
                        let n = this.model.as_ref().map(|m| m.layout.nodes.len()).unwrap_or(0);
                        if n != this.last_node_count {
                            this.last_node_count = n;
                            if n > 0 {
                                this.schedule_fit(cx);
                            }
                        }
                    }
                    Err(e) => this.error = Some(backend::format_error(&e)),
                }
                let err = this.error.clone();
                this.forbidden.update(cx, |b, cx| b.set_error(err.as_deref(), cx));
                cx.notify();
            },
        ));
        cx.notify();
    }

    /// Fit runs 50ms after the layout node count changes (container measured).
    fn schedule_fit(&mut self, cx: &mut Context<Self>) {
        let t = backend::sleep(cx, Duration::from_millis(50));
        self.fit_task = Some(cx.spawn(async move |this, cx| {
            t.await;
            this.update(cx, |this, cx| this.fit(cx)).ok();
        }));
    }

    fn fit(&mut self, cx: &mut Context<Self>) {
        let Some(m) = &self.model else { return };
        let b = self.bounds.get();
        if let Some((x, y, s)) = gl::fit(f32::from(b.size.width), f32::from(b.size.height), m.layout.width, m.layout.height) {
            self.t = Transform { x, y, s };
            cx.notify();
        }
    }

    /// Mouse position → graph coordinates.
    fn to_graph(&self, p: Point<Pixels>) -> (f32, f32) {
        let b = self.bounds.get();
        let mx = f32::from(p.x - b.origin.x);
        let my = f32::from(p.y - b.origin.y);
        ((mx - self.t.x) / self.t.s, (my - self.t.y) / self.t.s)
    }

    fn node_at(&self, p: Point<Pixels>) -> Option<usize> {
        let m = self.model.as_ref()?;
        let (gx, gy) = self.to_graph(p);
        // Topmost first (later nodes paint above earlier ones).
        m.layout
            .nodes
            .iter()
            .enumerate()
            .rev()
            .find(|(_, n)| gx >= n.x && gx <= n.x + NODE_WIDTH && gy >= n.y && gy <= n.y + NODE_HEIGHT)
            .map(|(i, _)| i)
    }

    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(m) = &self.model else { return };
        let n = &m.layout.nodes[ix].node;
        // A ReplicaSet opens its owning Deployment (quirk 10.11, fixed).
        let (kind, name, namespace) = match gl::click_target(n, &m.graph.edges) {
            gl::ClickTarget::Open { kind, name, namespace } => (kind, name, namespace),
            gl::ClickTarget::Message(msg) => {
                toast(cx, msg, ToastKind::Info);
                return;
            }
        };
        // Node kind mapping; unknown kinds → pods.
        let kind = match kind.as_str() {
            "Deployment" => ResourceKind::Deployments,
            "Service" => ResourceKind::Services,
            "Ingress" => ResourceKind::Ingresses,
            "Job" => ResourceKind::Jobs,
            "CronJob" => ResourceKind::Cronjobs,
            _ => ResourceKind::Pods,
        };
        let item = ResourceItem { name, namespace, ..Default::default() };
        self.app.update(cx, |a, cx| a.open_resource(Some(kind), item, cx));
    }

    fn set_hover(&mut self, h: Option<usize>, cx: &mut Context<Self>) {
        if self.hovered != h {
            self.hovered = h;
            cx.notify();
        }
    }
}

fn ns_of(a: &AppState) -> Option<String> {
    if a.namespace == "*" {
        None
    } else {
        Some(a.namespace.clone())
    }
}

/// The 8 kind icons with the stroke width the graph uses (1.5).
fn load_icons() -> HashMap<&'static str, Rc<Vec<u8>>> {
    let mut out = HashMap::new();
    for name in ["box", "network", "container", "globe", "layers", "timer", "clock", "server"] {
        let path = format!("lucide/{name}.svg");
        if let Ok(Some(bytes)) = crate::assets::Assets.load(&path) {
            let s = String::from_utf8_lossy(&bytes).replace("stroke-width=\"2\"", "stroke-width=\"1.5\"");
            out.insert(name, Rc::new(s.into_bytes()));
        }
    }
    out
}

fn with_alpha(c: Rgba, a: f32) -> Rgba {
    Rgba { a: c.a * a, ..c }
}

/// Paint the whole graph (background, grid, edges, nodes) for transform `t`.
#[allow(clippy::too_many_arguments)]
fn paint_graph(
    b: Bounds<Pixels>,
    m: &Model,
    t: Transform,
    hovered: Option<usize>,
    vis: &HashMap<String, Vis>,
    accent: Rgba,
    window: &mut Window,
    cx: &mut App,
) {
    let filter_active = !vis.is_empty();
    // Background: radial-gradient(circle at 50% 50%, #0f1a2e 0%, #0b1221 100%),
    // circle → farthest-corner. Approximated with concentric discs (the two
    // stops differ by at most 13 levels per channel, so 24 steps are smooth).
    window.with_content_mask(Some(ContentMask { bounds: b }), |window| {
        let c = b.center();
        let r_max = (f32::from(b.size.width).powi(2) + f32::from(b.size.height).powi(2)).sqrt() / 2.;
        let from = theme::hex(0x0f1a2e);
        let to = theme::hex(0x0b1221);
        window.paint_quad(fill(b, to));
        const STEPS: usize = 64;
        for i in (0..STEPS).rev() {
            let f = (i as f32 + 0.5) / STEPS as f32; // radius fraction of this disc's outer edge
            let r = r_max * (i as f32 + 1.) / STEPS as f32;
            let col = Rgba {
                r: from.r + (to.r - from.r) * f,
                g: from.g + (to.g - from.g) * f,
                b: from.b + (to.b - from.b) * f,
                a: 1.,
            };
            let rb = Bounds::new(point(c.x - px(r), c.y - px(r)), size(px(2. * r), px(2. * r)));
            window.paint_quad(fill(rb, col).corner_radii(px(r)));
        }

        // Dot grid: one r=0.5 circle at (20,20) per 40×40 cell, transformed.
        let cell = 40. * t.s;
        if cell >= 8. {
            let dot = (t.s).max(0.5);
            let w = f32::from(b.size.width);
            let h = f32::from(b.size.height);
            let ox = (t.x + 20. * t.s).rem_euclid(cell);
            let oy = (t.y + 20. * t.s).rem_euclid(cell);
            let grid = theme::hex(0x1e293b);
            let mut y = oy;
            while y < h + cell {
                let mut x = ox;
                while x < w + cell {
                    let p = point(b.origin.x + px(x - dot / 2.), b.origin.y + px(y - dot / 2.));
                    window.paint_quad(fill(Bounds::new(p, size(px(dot), px(dot))), grid).corner_radii(px(dot / 2.)));
                    x += cell;
                }
                y += cell;
            }
        }

        let to_screen = |x: f32, y: f32| point(b.origin.x + px(t.x + x * t.s), b.origin.y + px(t.y + y * t.s));

        // Neighbors of the hovered node.
        let mut connected: HashSet<&str> = HashSet::new();
        if let Some(h) = hovered {
            let id = m.layout.nodes[h].node.id.as_str();
            connected.insert(id);
            for e in &m.graph.edges {
                if e.source == id {
                    connected.insert(e.target.as_str());
                }
                if e.target == id {
                    connected.insert(e.source.as_str());
                }
            }
        }

        // Edges.
        let edge_color = theme::hex(0x334155);
        for e in &m.graph.edges {
            let (Some(&si), Some(&ti)) = (m.index.get(&e.source), m.index.get(&e.target)) else { continue };
            let (src, tgt) = (&m.layout.nodes[si], &m.layout.nodes[ti]);
            let hl = hovered.is_some() && connected.contains(e.source.as_str()) && connected.contains(e.target.as_str());
            let filter_dim =
                filter_active && (vis.get(&e.source) == Some(&Vis::Hidden) || vis.get(&e.target) == Some(&Vis::Hidden));
            let mut op = if hl { 0.9 } else { 0.5 };
            if filter_dim {
                op = 0.05;
            } else if hovered.is_some() && !hl {
                op = 0.15;
            }
            let sw = if hl { 2. } else { 1. };
            let color = if hl { accent } else { edge_color };
            let [p0, c1, c2, p3] = gl::edge_curve(src, tgt);
            let mut pb = PathBuilder::stroke(px(sw * t.s));
            if e.relation != "owns" {
                pb = pb.dash_array(&[px(4. * sw * t.s), px(3. * sw * t.s)]);
            }
            pb.move_to(to_screen(p0.0, p0.1));
            pb.cubic_bezier_to(to_screen(p3.0, p3.1), to_screen(c1.0, c1.1), to_screen(c2.0, c2.1));
            if let Ok(path) = pb.build() {
                window.paint_path(path, with_alpha(color, op));
            }
            // Arrowhead marker (8×6, ref (7,3), orient auto → pointing down; units = stroke width).
            let k = sw;
            let (tx, ty) = p3;
            let mut ab = PathBuilder::fill();
            ab.move_to(to_screen(tx + 3. * k, ty - 7. * k));
            ab.line_to(to_screen(tx, ty + k));
            ab.line_to(to_screen(tx - 3. * k, ty - 7. * k));
            ab.close();
            if let Ok(path) = ab.build() {
                let a = if hl { 0.8 } else { 0.6 };
                window.paint_path(path, with_alpha(color, a * op));
            }
        }

        // Nodes.
        let text_system = window.text_system().clone();
        for (i, n) in m.layout.nodes.iter().enumerate() {
            let node = &n.node;
            let is_hovered = hovered == Some(i);
            let fvis = vis.get(&node.id);
            let f_hidden = filter_active && fvis == Some(&Vis::Hidden);
            let f_conn = filter_active && fvis == Some(&Vis::Connected);
            let mut op: f32 = 1.;
            if f_hidden {
                op = 0.08;
            } else if f_conn {
                op = 0.6;
            }
            if hovered.is_some() && !connected.contains(node.id.as_str()) && !f_hidden {
                op = op.min(0.3);
            }
            let kc = gl::kind_color(&node.kind);
            let status = theme::hex(gl::status_color(&node.status));
            let s = t.s;
            let rect = |x: f32, y: f32, w: f32, h: f32| Bounds::new(to_screen(x, y), size(px(w * s), px(h * s)));

            if is_hovered {
                // Glow: stroke 1 (centered) at opacity .4.
                window.paint_quad(
                    quad(rect(n.x - 3.5, n.y - 3.5, 187., 63.), px(12.5 * s), transparent_black(), px(s), with_alpha(status, 0.4 * op), BorderStyle::Solid),
                );
            }
            let sw = if is_hovered { 2. } else { 1.5 };
            let body_op = if is_hovered { 1. } else { 0.9 } * op;
            window.paint_quad(quad(
                rect(n.x - sw / 2., n.y - sw / 2., NODE_WIDTH + sw, NODE_HEIGHT + sw),
                px((10. + sw / 2.) * s),
                with_alpha(theme::hex(kc.fill), body_op),
                px(sw * s),
                with_alpha(status, body_op),
                BorderStyle::Solid,
            ));
            // Status dot.
            window.paint_quad(fill(rect(n.x + 168. - 3.5, n.y + 12. - 3.5, 7., 7.), with_alpha(status, op)).corner_radii(px(3.5 * s)));
            // Kind icon (14px, centered in a 16px box at +10,+8).
            let icon_name = gl::kind_icon(&node.kind);
            if let Some(bytes) = m.icons.get(icon_name) {
                let ib = rect(n.x + 11., n.y + 9., 14., 14.);
                window
                    .paint_svg(
                        ib,
                        SharedString::from(format!("kore-graph/{icon_name}")),
                        Some(bytes.as_slice()),
                        TransformationMatrix::unit(),
                        with_alpha(theme::hex(kc.text), op).into(),
                        cx,
                    )
                    .ok();
            }
            // Kind label (9px mono 500, opacity .8) and name (11px mono 500).
            let mono = Font { weight: FontWeight::MEDIUM, ..font(MONO) };
            let mut text = |txt: String, x: f32, baseline: f32, size_px: f32, color: Rgba| {
                let sz = px(size_px * s);
                let run = TextRun { len: txt.len(), font: mono.clone(), color: color.into(), background_color: None, underline: None, strikethrough: None };
                let line = text_system.shape_line(txt.into(), sz, &[run], None);
                let lh = line.ascent + line.descent;
                let origin = to_screen(x, baseline);
                let origin = point(origin.x, origin.y - line.ascent);
                line.paint(origin, lh, TextAlign::Left, None, window, cx).ok();
            };
            text(node.kind.clone(), n.x + 30., n.y + 19., 9. * MONO_FIX, with_alpha(theme::hex(kc.text), 0.8 * op));
            text(gl::display_name(&node.name), n.x + 10., n.y + 40., 11. * MONO_FIX, with_alpha(theme::hex(0xe2e8f0), op));
        }
    });
}

fn overlay_box() -> Div {
    div().bg(theme::hexa(0x101828, 0.8)).border_1().border_color(slate::S800).rounded(px(8.))
}

impl GraphView {
    /// The hover tooltip: glass, radius 8, `px 12, py 8`, 12px, max-width 240;
    /// the name (mono medium slate-100, broken anywhere), then the kind
    /// (slate-400) and status (medium, status color) with gap 12, then
    /// "ns: {namespace}" (slate-500, margin-top 2). Pointer-transparent; fades
    /// in with y 4→0 over 150ms.
    fn render_tooltip(&self, n: &gl::LayoutNode, window: &mut Window) -> AnyElement {
        let node = &n.node;
        let status = if node.status.is_empty() { "Unknown".to_string() } else { node.status.clone() };
        let inner_max = gl::TOOLTIP_MAX_W - 2. * gl::TOOLTIP_PAD_X;
        let cols = (inner_max / mono_text::pitch(px(12.)).as_f32()).floor() as usize;
        let rows = gl::break_all(&node.name, cols);
        let name_w = rows.iter().map(|r| mono_text::width(r.chars().count(), 12.)).fold(0., f32::max);
        let row_w = measure(window, &node.kind, SANS, 12., FontWeight::NORMAL) + 12. + measure(window, &status, SANS, 12., FontWeight::MEDIUM);
        let ns_line = (!node.namespace.is_empty()).then(|| format!("ns: {}", node.namespace));
        let ns_w = ns_line.as_ref().map(|t| measure(window, t, SANS, 12., FontWeight::NORMAL)).unwrap_or(0.);
        let content_w = name_w.max(row_w).max(ns_w).min(inner_max).ceil();
        let w = content_w + 2. * gl::TOOLTIP_PAD_X + 2.;
        let h = 2. * gl::TOOLTIP_PAD_Y + 2. + rows.len() as f32 * 16. + 4. + 16. + if ns_line.is_some() { 18. } else { 0. };
        let canvas_w = f32::from(self.bounds.get().size.width);
        let (x, y) = gl::tooltip_origin(n, (self.t.x, self.t.y, self.t.s), w, h, canvas_w);
        let status_color = theme::hex(gl::status_color(&node.status));
        div()
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(w))
            .glass()
            .rounded(px(8.))
            .px(px(gl::TOOLTIP_PAD_X))
            .py(px(gl::TOOLTIP_PAD_Y))
            .text_xs_tw()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .mb(px(4.))
                    .font_mono()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(slate::S100)
                    .children(rows.into_iter().map(mono_text)),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(12.))
                    .text_color(slate::S400)
                    .child(node.kind.clone())
                    .child(div().font_weight(FontWeight::MEDIUM).text_color(status_color).child(status.clone()))
                    .auto_id("graph.tooltip.kind", "text", format!("{} {status}", node.kind)),
            )
            .children(ns_line.map(|t| div().mt(px(2.)).text_color(slate::S500).child(t.clone()).auto_id("graph.tooltip.ns", "text", t)))
            .with_animation(
                SharedString::from(format!("graph-tip-{}", node.id)),
                Animation::new(Duration::from_millis(150)),
                move |el, t| el.opacity(t).top(px(y + 4. * (1. - t))),
            )
            .auto_id("graph.tooltip", "tooltip", node.name.clone())
            .into_any_element()
    }

    fn render_states(&self, accent: Rgba, cx: &App) -> Option<AnyElement> {
        let base = || div().size_full().flex().flex_col().items_center().justify_center().gap(px(12.));
        if self.loading {
            return Some(
                base()
                    .child(spinning_icon("graph-loading", "loader-2", 32., accent))
                    .child(div().text_sm_tw().text_color(slate::S400).child("Building resource graph..."))
                    .auto_id("view.graph", "region", "Graph")
                    .into_any_element(),
            );
        }
        if let Some(err) = &self.error {
            return Some(
                base()
                    .text_color(red::R400)
                    .child(div().text_sm_tw().font_weight(FontWeight::MEDIUM).child("Failed to load resource graph"))
                    .child(div().text_xs_tw().text_color(slate::S500).max_w(px(448.)).text_center().child(err.clone()))
                    .when(self.forbidden.read(cx).visible(), |d| d.child(div().mt(px(8.)).w(px(560.)).max_w_full().child(self.forbidden.clone())))
                    .auto_id("view.graph", "region", "Graph")
                    .into_any_element(),
            );
        }
        let empty = self.model.as_ref().map(|m| m.graph.nodes.is_empty()).unwrap_or(true);
        if empty {
            let msg = match &self.namespace {
                Some(ns) => format!("No resources in namespace \"{ns}\""),
                None => "Select a namespace to view the resource graph".to_string(),
            };
            return Some(
                base()
                    .text_color(slate::S400)
                    .child(icon("inbox", 48.).text_color(slate::S600))
                    .child(div().text_sm_tw().font_weight(FontWeight::MEDIUM).child("No resources found"))
                    .child(div().text_xs_tw().text_color(slate::S500).child(msg))
                    .auto_id("view.graph", "region", "Graph")
                    .into_any_element(),
            );
        }
        None
    }
}

impl Render for GraphView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        if let Some(el) = self.render_states(accent, cx) {
            return el;
        }
        let m = self.model.clone().expect("model");
        let vis = Rc::new(gl::visibility(&m.graph.nodes, &m.graph.edges, &self.disabled, &self.query));
        let t = self.t;
        let hovered = self.hovered;
        let bounds_cell = self.bounds.clone();

        let cursor = if self.pan.is_some() {
            CursorStyle::ClosedHand
        } else if hovered.is_some() {
            CursorStyle::PointingHand
        } else {
            CursorStyle::OpenHand
        };

        let m_paint = m.clone();
        let vis_paint = vis.clone();
        let surface = div()
            .id("graph-canvas")
            .absolute()
            .inset_0()
            .cursor(cursor)
            .child(
                canvas(
                    move |b, _, _| bounds_cell.set(b),
                    move |b, _, window, cx| paint_graph(b, &m_paint, t, hovered, &vis_paint, accent, window, cx),
                )
                .size_full(),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, _, cx| {
                    this.pan = Some((e.position, this.t));
                    this.down_node = this.node_at(e.position);
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                if let Some((start, t0)) = this.pan {
                    if e.pressed_button == Some(MouseButton::Left) {
                        let d = e.position - start;
                        this.t = Transform { x: t0.x + f32::from(d.x), y: t0.y + f32::from(d.y), s: t0.s };
                        cx.notify();
                    } else {
                        this.pan = None;
                        cx.notify();
                    }
                }
                if this.last_move != Some(e.position) {
                    this.last_move = Some(e.position);
                    let h = this.node_at(e.position);
                    this.set_hover(h, cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, e: &MouseUpEvent, _, cx| {
                    let start = this.pan.take().map(|(p, _)| p);
                    let up = this.node_at(e.position);
                    let down = this.down_node.take();
                    cx.notify();
                    // A pan that starts and ends on the same node is not a
                    // click (quirk fixed): at most 4px of movement opens it.
                    let moved = start.map(|p| gl::is_drag(f32::from(e.position.x - p.x), f32::from(e.position.y - p.y))).unwrap_or(false);
                    if let (Some(a), Some(b), false) = (down, up, moved) {
                        if a == b {
                            this.select(a, cx);
                        }
                    }
                }),
            )
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !*hovered {
                    // mouseleave ends the pan; leaving the node group clears hover.
                    this.pan = None;
                    this.down_node = None;
                    this.set_hover(None, cx);
                    cx.notify();
                }
            }))
            .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, window, cx| {
                let dy = match e.delta {
                    ScrollDelta::Pixels(p) => f32::from(p.y),
                    ScrollDelta::Lines(l) => l.y,
                };
                let dx = match e.delta {
                    ScrollDelta::Pixels(p) => f32::from(p.x),
                    ScrollDelta::Lines(l) => l.x,
                };
                if dy == 0. && dx == 0. {
                    return;
                }
                let b = this.bounds.get();
                let mx = f32::from(e.position.x - b.origin.x);
                let my = f32::from(e.position.y - b.origin.y);
                // GPUI +y = wheel up (browser deltaY < 0 → zoom in).
                let (x, y, s) = gl::zoom_at((this.t.x, this.t.y, this.t.s), mx, my, -dy);
                this.t = Transform { x, y, s };
                // Hover only changes on mouse movement
                // (mouseenter/leave), not when zoom moves a node under the cursor.
                cx.notify();
                // Present the new transform promptly even when this is the last
                // wheel event of a burst (no further input to wake the loop).
                window.refresh();
            }));

        // Automation: one invisible element per node (layout-neutral).
        let node_ids: Vec<AnyElement> = if registry::enabled() {
            m.layout
                .nodes
                .iter()
                .map(|n| {
                    div()
                        .absolute()
                        .left(px(t.x + n.x * t.s))
                        .top(px(t.y + n.y * t.s))
                        .w(px(NODE_WIDTH * t.s))
                        .h(px(NODE_HEIGHT * t.s))
                        .auto_id(format!("graph.node.{}", n.node.id), "button", n.node.name.clone())
                        // Dimmed by the kind/name filter (anything but a direct match).
                        .disabled(!vis.is_empty() && vis.get(&n.node.id) != Some(&gl::Vis::Direct))
                        .into_any_element()
                })
                .collect()
        } else {
            Vec::new()
        };

        // Overlays.
        let badge = overlay_box()
            .id("graph-badge")
            .occlude()
            .absolute()
            .top(px(12.))
            .left(px(12.))
            .px(px(8.))
            .py(px(4.))
            .text_size(px(10. * MONO_FIX)).line_height(px(15.))
            .text_color(slate::S500)
            .font_mono()
            .child(format!("{} nodes \u{b7} {} edges", m.graph.nodes.len(), m.graph.edges.len()))
            .auto_id("graph.count", "text", format!("{} nodes \u{b7} {} edges", m.graph.nodes.len(), m.graph.edges.len()));

        let toolbar = div()
            .absolute()
            .top(px(12.))
            .right(px(12.))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.))
            .child(
                overlay_box()
                    .id("graph-fit")
                    .occlude()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .px(px(10.))
                    .py(px(6.))
                    .text_xs_tw()
                    .text_color(slate::S300)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::hexa(0x1d2939, 0.3)))
                    .child(icon("maximize-2", 14.).text_color(slate::S300))
                    .child("Fit")
                    .on_click(cx.listener(|this, _, _, cx| this.fit(cx)))
                    .auto_id("graph.fit", "button", "Fit"),
            )
            .child(
                div()
                    .px(px(6.))
                    .text_size(px(10. * MONO_FIX)).line_height(px(15.))
                    .font_mono()
                    .text_color(slate::S600)
                    .child(format!("{}%", (t.s * 100.).round() as i64))
                    .auto_id("graph.zoom", "text", format!("{}%", (t.s * 100.).round() as i64)),
            );

        let kinds = gl::unique_kinds(&m.graph.nodes);
        let chips = overlay_box()
            .id("graph-chips")
            .occlude()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.))
            .px(px(8.))
            .py(px(6.))
            .children(kinds.into_iter().map(|k| {
                let kc = gl::kind_color(&k);
                let enabled = !self.disabled.contains(&k);
                let k2 = k.clone();
                div()
                    .id(SharedString::from(format!("graph-chip-{k}")))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(4.))
                    .px(px(6.))
                    .py(px(2.))
                    .rounded(px(4.))
                    .text_size(px(10. * MONO_FIX)).line_height(px(15.))
                    .font_mono()
                    .border_1()
                    .cursor_pointer()
                    .map(|d| {
                        if enabled {
                            d.bg(theme::hex(kc.fill)).border_color(theme::hex(kc.stroke)).text_color(theme::hex(kc.text))
                        } else {
                            d.border_color(theme::hex(0x334155)).text_color(theme::hex(0x475569)).opacity(0.5)
                        }
                    })
                    .child(
                        div()
                            .size(px(6.))
                            .rounded_full()
                            .bg(if enabled { theme::hex(kc.stroke) } else { theme::hex(0x475569) }),
                    )
                    .child(k.clone())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.disabled.remove(&k2) {
                            this.disabled.insert(k2.clone());
                        }
                        cx.notify();
                    }))
                    .auto_id(format!("graph.chip.{k}"), "button", k.clone())
                    .selected(enabled)
            }));

        let focused = self.input.read(cx).is_focused(window);
        let query_empty = self.query.is_empty();
        let search = div()
            .id("graph-search")
            .occlude()
            .relative()
            .flex()
            .items_center()
            .w(px(160.))
            .child(div().absolute().left(px(8.)).top_0().bottom_0().flex().items_center().child(icon("search", 12.).text_color(slate::S500)))
            .child(
                div()
                    .w_full()
                    .bg(theme::hexa(0x101828, 0.8))
                    .border_1()
                    .border_color(slate::S800)
                    .rounded(px(8.))
                    .when(focused, |d| {
                        d.shadow(vec![BoxShadow {
                            color: theme::Theme::get(cx).focus_ring().into(),
                            offset: point(px(0.), px(0.)),
                            blur_radius: px(0.),
                            spread_radius: px(2.),
                            inset: false,
                        }])
                    })
                    .child(self.input.clone()),
            )
            .when(!query_empty, |d| {
                d.child(
                    div().absolute().right(px(6.)).top_0().bottom_0().flex().items_center().child(
                        div()
                            .id("graph-search-clear")
                            .group("graph-clear")
                            .p(px(2.))
                            .cursor_pointer()
                            .child(icon("x", 12.).text_color(slate::S500).group_hover("graph-clear", |s| s.text_color(slate::S300)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.query.clear();
                                this.input.update(cx, |i, cx| i.set_text("", cx));
                                cx.notify();
                            }))
                            .auto_id("graph.filter.clear", "button", "Clear filter"),
                    ),
                )
            })
            .auto_id("graph.filter", "textbox", "Filter by name...");

        let filter_bar = div()
            .absolute()
            .top(px(46.))
            .left(px(12.))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .child(chips)
            .child(search);

        let legend = overlay_box()
            .id("graph-legend")
            .occlude()
            .absolute()
            .bottom(px(12.))
            .left(px(12.))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(12.))
            .px(px(12.))
            .py(px(8.))
            .children(gl::LEGEND_KINDS.iter().map(|k| {
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .child(div().size(px(10.)).rounded(px(2.)).bg(theme::hex(gl::kind_color(k).stroke)))
                    .child(div().text_10().text_color(slate::S400).child(*k))
            }));

        // Hover tooltip (quirk 10.21, fixed): name, kind + status, namespace,
        // centered above the hovered node.
        let tooltip = hovered.map(|h| self.render_tooltip(&m.layout.nodes[h], window));

        div()
            .size_full()
            .relative()
            .overflow_hidden()
            .child(surface)
            .children(node_ids)
            .child(toolbar)
            .child(legend)
            .child(badge)
            .child(filter_bar)
            .children(tooltip)
            .with_animation("graph-fade", Animation::new(Duration::from_millis(300)), |el, t| el.opacity(t))
            .auto_id("view.graph", "region", "Graph")
            .into_any_element()
    }
}
