//! Resource graph layout (port of `src/lib/graph-layout.ts`, spec 03 §2.2),
//! colors (§2.3) and filter visibility (§2.10). Pure logic, unit-tested.

use kore_core::state::graph::{GraphEdge, GraphNode};
use std::collections::{HashMap, HashSet};

pub const NODE_WIDTH: f32 = 180.;
pub const NODE_HEIGHT: f32 = 56.;
pub const LAYER_GAP_Y: f32 = 100.;
pub const NODE_GAP_X: f32 = 40.;
pub const PADDING: f32 = 60.;

/// `KIND_LAYER_ORDER` (unknown kinds → 2).
pub fn layer_for_kind(kind: &str) -> u32 {
    match kind {
        "Ingress" => 0,
        "Service" => 1,
        "Deployment" | "CronJob" => 2,
        "ReplicaSet" | "Job" => 3,
        "Pod" => 4,
        _ => 2,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KindColor {
    pub fill: u32,
    pub stroke: u32,
    pub text: u32,
}

/// `KIND_COLORS` keys in declaration order (the legend lists all of them).
pub const LEGEND_KINDS: [&str; 7] = ["Deployment", "Service", "Pod", "Ingress", "Job", "CronJob", "ReplicaSet"];

pub fn kind_color(kind: &str) -> KindColor {
    let (fill, stroke, text) = match kind {
        "Deployment" => (0x1e3a5f, 0x3b82f6, 0x93c5fd),
        "Service" => (0x2d1b4e, 0xa855f7, 0xd8b4fe),
        "Pod" => (0x1a3a2a, 0x10b981, 0x6ee7b7),
        "Ingress" => (0x0c3642, 0x22d3ee, 0x67e8f9),
        "Job" | "CronJob" => (0x3b2a0a, 0xf97316, 0xfdba74),
        _ => (0x1e293b, 0x64748b, 0x94a3b8),
    };
    KindColor { fill, stroke, text }
}

/// `getStatusBorderColor`.
pub fn status_color(status: &str) -> u32 {
    match status.to_lowercase().as_str() {
        "running" | "ready" | "active" | "succeeded" | "complete" | "completed" | "available" => 0x10b981,
        "pending" | "containercreating" | "init" | "waiting" => 0xf59e0b,
        "failed" | "crashloopbackoff" | "error" | "imagepullbackoff" | "evicted" | "oomkilled" => 0xef4444,
        _ => 0x64748b,
    }
}

/// Lucide icon per kind (`KindIcon`).
pub fn kind_icon(kind: &str) -> &'static str {
    match kind {
        "Pod" => "box",
        "Service" => "network",
        "Deployment" => "container",
        "Ingress" => "globe",
        "ReplicaSet" => "layers",
        "Job" => "timer",
        "CronJob" => "clock",
        _ => "server",
    }
}

/// Name as drawn on a node: > 20 chars → first 18 + "...".
pub fn display_name(name: &str) -> String {
    if name.chars().count() > 20 {
        let head: String = name.chars().take(18).collect();
        format!("{head}...")
    } else {
        name.to_string()
    }
}

#[derive(Clone, Debug)]
pub struct LayoutNode {
    pub node: GraphNode,
    /// Top-left corner, graph coordinates.
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub nodes: Vec<LayoutNode>,
    pub width: f32,
    pub height: f32,
}

fn layer_width(n: usize) -> f32 {
    n as f32 * NODE_WIDTH + (n as f32 - 1.) * NODE_GAP_X
}

/// `computeLayout`, reproduced exactly (including JS stable sort and the
/// "score 0 when no parent is placed" rule).
pub fn compute_layout(nodes: &[GraphNode], edges: &[GraphEdge]) -> Layout {
    if nodes.is_empty() {
        return Layout::default();
    }
    let mut layers: Vec<(u32, Vec<&GraphNode>)> = Vec::new();
    for n in nodes {
        let l = layer_for_kind(&n.kind);
        match layers.iter_mut().find(|(k, _)| *k == l) {
            Some((_, v)) => v.push(n),
            None => layers.push((l, vec![n])),
        }
    }
    layers.sort_by_key(|(k, _)| *k);

    let mut parents: HashMap<&str, Vec<&str>> = HashMap::new();
    for e in edges {
        parents.entry(e.target.as_str()).or_default().push(e.source.as_str());
    }
    let max_w = layers.iter().map(|(_, v)| layer_width(v.len())).fold(0f32, f32::max);
    let total_w = max_w + PADDING * 2.;

    let mut out: Vec<LayoutNode> = Vec::with_capacity(nodes.len());
    for (li, (_, layer)) in layers.iter_mut().enumerate() {
        let pos: HashMap<&str, f32> = out.iter().map(|n| (n.node.id.as_str(), n.x)).collect();
        let score = |n: &GraphNode| -> f32 {
            let ps = parents.get(n.id.as_str());
            let (mut sum, mut cnt) = (0f32, 0u32);
            for p in ps.into_iter().flatten() {
                if let Some(x) = pos.get(p) {
                    sum += x;
                    cnt += 1;
                }
            }
            if cnt > 0 {
                sum / cnt as f32
            } else {
                0.
            }
        };
        let mut scored: Vec<(f32, &GraphNode)> = layer.iter().map(|n| (score(n), *n)).collect();
        // Vec::sort_by is stable, like Array.prototype.sort.
        scored.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let w = layer_width(scored.len());
        let start_x = (total_w - w) / 2.;
        let y = PADDING + li as f32 * (NODE_HEIGHT + LAYER_GAP_Y);
        for (i, (_, n)) in scored.into_iter().enumerate() {
            out.push(LayoutNode { node: n.clone(), x: start_x + i as f32 * (NODE_WIDTH + NODE_GAP_X), y });
        }
    }
    let l = layers.len() as f32;
    Layout { nodes: out, width: total_w, height: PADDING * 2. + l * NODE_HEIGHT + (l - 1.) * LAYER_GAP_Y }
}

/// Edge bezier: bottom-center of source → top-center of target.
/// Returns (start, ctrl1, ctrl2, end) in graph coordinates.
pub fn edge_curve(src: &LayoutNode, tgt: &LayoutNode) -> [(f32, f32); 4] {
    let sx = src.x + NODE_WIDTH / 2.;
    let sy = src.y + NODE_HEIGHT;
    let tx = tgt.x + NODE_WIDTH / 2.;
    let ty = tgt.y;
    let mid = (sy + ty) / 2.;
    [(sx, sy), (sx, mid), (tx, mid), (tx, ty)]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vis {
    Direct,
    Connected,
    Hidden,
}

/// Filter visibility per node id (empty map when no filter is active).
pub fn visibility(nodes: &[GraphNode], edges: &[GraphEdge], disabled: &HashSet<String>, query: &str) -> HashMap<String, Vis> {
    let mut vis = HashMap::new();
    if disabled.is_empty() && query.is_empty() {
        return vis;
    }
    let q = query.to_lowercase();
    let mut direct: HashSet<&str> = HashSet::new();
    for n in nodes {
        if !disabled.contains(&n.kind) && (q.is_empty() || n.name.to_lowercase().contains(&q)) {
            direct.insert(n.id.as_str());
            vis.insert(n.id.clone(), Vis::Direct);
        }
    }
    for e in edges {
        if direct.contains(e.source.as_str()) && !direct.contains(e.target.as_str()) {
            vis.entry(e.target.clone()).or_insert(Vis::Connected);
        }
        if direct.contains(e.target.as_str()) && !direct.contains(e.source.as_str()) {
            vis.entry(e.source.clone()).or_insert(Vis::Connected);
        }
    }
    for n in nodes {
        vis.entry(n.id.clone()).or_insert(Vis::Hidden);
    }
    vis
}

/// Distinct kinds present, stably sorted by layer.
pub fn unique_kinds(nodes: &[GraphNode]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for n in nodes {
        if !seen.contains(&n.kind) {
            seen.push(n.kind.clone());
        }
    }
    seen.sort_by_key(|k| layer_for_kind(k));
    seen
}

/// Wheel zoom anchored at (mx, my): returns the new (tx, ty, s).
pub fn zoom_at(t: (f32, f32, f32), mx: f32, my: f32, delta_y: f32) -> (f32, f32, f32) {
    let factor = if delta_y > 0. { 0.92 } else { 1.08 };
    let s = (t.2 * factor).clamp(0.15, 4.);
    let k = s / t.2;
    (mx - (mx - t.0) * k, my - (my - t.1) * k, s)
}

/// Fit to view.
pub fn fit(w: f32, h: f32, gw: f32, gh: f32) -> Option<(f32, f32, f32)> {
    if gw == 0. || gh == 0. {
        return None;
    }
    let s = ((w - 40.) / gw).min((h - 40.) / gh).min(1.5);
    Some(((w - gw * s) / 2., (h - gh * s) / 2., s))
}

/// What a node click opens (spec 03 §0.4).
#[derive(Clone, Debug, PartialEq)]
pub enum ClickTarget {
    /// Open the details view for `(graph kind, name, namespace)`.
    Open { kind: String, name: String, namespace: String },
    /// Nothing to open: show this message instead.
    Message(String),
}

/// Resolves a node click. ReplicaSets have no details view in the app, so a
/// ReplicaSet click opens the Deployment that owns it (the `owns` edge into
/// it); a ReplicaSet without an owning Deployment only explains that (quirk
/// 10.11, fixed: it used to open a deployment detail named after the RS).
pub fn click_target(node: &GraphNode, edges: &[GraphEdge]) -> ClickTarget {
    if node.kind != "ReplicaSet" {
        return ClickTarget::Open { kind: node.kind.clone(), name: node.name.clone(), namespace: node.namespace.clone() };
    }
    let owner = edges
        .iter()
        .filter(|e| e.target == node.id && e.relation == "owns")
        .find_map(|e| e.source.strip_prefix("Deployment/"))
        .and_then(|rest| rest.split_once('/'));
    match owner {
        Some((ns, name)) => ClickTarget::Open { kind: "Deployment".into(), name: name.into(), namespace: ns.into() },
        None => ClickTarget::Message(format!("ReplicaSet {} has no owning Deployment to open", node.name)),
    }
}

/// A mouse-down → mouse-up movement larger than 4px is a pan, not a click.
pub fn is_drag(dx: f32, dy: f32) -> bool {
    dx * dx + dy * dy > 16.
}

/// Hover tooltip box geometry (spec 03 §2.7).
pub const TOOLTIP_MAX_W: f32 = 240.;
pub const TOOLTIP_PAD_X: f32 = 12.;
pub const TOOLTIP_PAD_Y: f32 = 8.;
pub const TOOLTIP_GAP: f32 = 8.;

/// Top-left of the hover tooltip (`w`×`h`) in canvas coordinates: centered
/// above the node with an 8px gap; flipped below the node when it would leave
/// the canvas top, and kept 4px inside the canvas horizontally.
pub fn tooltip_origin(node: &LayoutNode, t: (f32, f32, f32), w: f32, h: f32, canvas_w: f32) -> (f32, f32) {
    let (tx, ty, s) = t;
    let cx = (node.x + NODE_WIDTH / 2.) * s + tx;
    let top = node.y * s + ty - TOOLTIP_GAP - h;
    let y = if top < 4. { (node.y + NODE_HEIGHT) * s + ty + TOOLTIP_GAP } else { top };
    let x = (cx - w / 2.).min(canvas_w - w - 4.).max(4.);
    (x, y)
}

/// Splits a name into rows of at most `cols` chars (CSS `break-all`).
pub fn break_all(name: &str, cols: usize) -> Vec<String> {
    let chars: Vec<char> = name.chars().collect();
    if chars.is_empty() {
        return vec![String::new()];
    }
    chars.chunks(cols.max(1)).map(|c| c.iter().collect()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(kind: &str, name: &str) -> GraphNode {
        GraphNode { id: format!("{kind}/ns/{name}"), name: name.into(), namespace: "ns".into(), kind: kind.into(), status: String::new() }
    }
    fn e(s: &GraphNode, t: &GraphNode, rel: &str) -> GraphEdge {
        GraphEdge { source: s.id.clone(), target: t.id.clone(), relation: rel.into() }
    }

    #[test]
    fn layout_layers_and_sizes() {
        let p1 = n("Pod", "a-1");
        let p2 = n("Pod", "b-1");
        let rs_a = n("ReplicaSet", "a");
        let rs_b = n("ReplicaSet", "b");
        let svc = n("Service", "s");
        // Backend order: pods first.
        let nodes = vec![p1.clone(), p2.clone(), rs_b.clone(), rs_a.clone(), svc.clone()];
        let edges = vec![e(&rs_a, &p1, "owns"), e(&rs_b, &p2, "owns"), e(&svc, &p2, "selects")];
        let l = compute_layout(&nodes, &edges);
        assert_eq!(l.width, 2. * 180. + 40. + 120.);
        assert_eq!(l.height, 120. + 3. * 56. + 2. * 100.);
        let get = |id: &str| l.nodes.iter().find(|x| x.node.id == id).unwrap().clone();
        // Service layer (index 0) centered.
        assert_eq!(get(&svc.id).x, (520. - 180.) / 2.);
        assert_eq!(get(&svc.id).y, 60.);
        // RS layer: no placed parents → score 0, keeps input order (b before a).
        assert_eq!(get(&rs_b.id).x, 60.);
        assert_eq!(get(&rs_a.id).x, 280.);
        assert_eq!(get(&rs_b.id).y, 60. + 156.);
        // Pods: p1's parent rs_a at 280, p2's parents rs_b(60)+svc(170) avg 115 → p2 first.
        assert_eq!(get(&p2.id).x, 60.);
        assert_eq!(get(&p1.id).x, 280.);
        assert_eq!(get(&p1.id).y, 60. + 2. * 156.);
    }

    #[test]
    fn edge_path_points() {
        let a = LayoutNode { node: n("Service", "s"), x: 10., y: 20. };
        let b = LayoutNode { node: n("Pod", "p"), x: 100., y: 176. };
        assert_eq!(edge_curve(&a, &b), [(100., 76.), (100., 126.), (190., 126.), (190., 176.)]);
    }

    #[test]
    fn filter_visibility() {
        let d = n("Deployment", "db");
        let rs = n("ReplicaSet", "db-1");
        let p = n("Pod", "db-1-x");
        let w = n("Deployment", "web");
        let nodes = vec![p.clone(), d.clone(), w.clone(), rs.clone()];
        let edges = vec![e(&d, &rs, "owns"), e(&rs, &p, "owns")];
        assert!(visibility(&nodes, &edges, &HashSet::new(), "").is_empty());
        let v = visibility(&nodes, &edges, &HashSet::new(), "DB-1");
        assert_eq!(v[&rs.id], Vis::Direct);
        assert_eq!(v[&p.id], Vis::Direct);
        assert_eq!(v[&d.id], Vis::Connected);
        assert_eq!(v[&w.id], Vis::Hidden);
        let mut dis = HashSet::new();
        dis.insert("Pod".to_string());
        let v = visibility(&nodes, &edges, &dis, "");
        assert_eq!(v[&p.id], Vis::Connected);
        assert_eq!(v[&rs.id], Vis::Direct);
    }

    #[test]
    fn kinds_colors_names() {
        let nodes = vec![n("Pod", "a"), n("Ingress", "i"), n("CronJob", "c"), n("Deployment", "d"), n("Job", "j")];
        assert_eq!(unique_kinds(&nodes), vec!["Ingress", "CronJob", "Deployment", "Job", "Pod"]);
        assert_eq!(status_color("CrashLoopBackOff"), 0xef4444);
        assert_eq!(status_color("ClusterIP"), 0x64748b);
        assert_eq!(display_name("abcdefghijklmnopqrstu"), "abcdefghijklmnopqr...");
        assert_eq!(display_name("abcdefghijklmnopqrst"), "abcdefghijklmnopqrst");
    }

    #[test]
    fn replicaset_click_opens_owning_deployment() {
        let d = n("Deployment", "web");
        let rs = n("ReplicaSet", "web-7d9f8c");
        let p = n("Pod", "web-7d9f8c-x");
        let edges = vec![e(&d, &rs, "owns"), e(&rs, &p, "owns")];
        assert_eq!(
            click_target(&rs, &edges),
            ClickTarget::Open { kind: "Deployment".into(), name: "web".into(), namespace: "ns".into() }
        );
        // Other kinds open themselves.
        assert_eq!(
            click_target(&p, &edges),
            ClickTarget::Open { kind: "Pod".into(), name: "web-7d9f8c-x".into(), namespace: "ns".into() }
        );
        // An orphan ReplicaSet has nothing to open.
        let orphan = n("ReplicaSet", "lonely");
        assert!(matches!(click_target(&orphan, &edges), ClickTarget::Message(m) if m.contains("lonely")));
    }

    #[test]
    fn tooltip_placement() {
        let node = LayoutNode { node: n("Pod", "p"), x: 100., y: 200. };
        // Centered above the node: center x = 190, top = 200 - 8 - 50.
        assert_eq!(tooltip_origin(&node, (0., 0., 1.), 120., 50., 1000.), (130., 142.));
        // Near the top edge: flipped below the node.
        let top = LayoutNode { node: n("Pod", "p"), x: 100., y: 10. };
        assert_eq!(tooltip_origin(&top, (0., 0., 1.), 120., 50., 1000.), (130., 74.));
        // Clamped inside the canvas horizontally.
        assert_eq!(tooltip_origin(&node, (-150., 0., 1.), 120., 50., 1000.).0, 4.);
        assert_eq!(tooltip_origin(&node, (900., 0., 1.), 120., 50., 1000.).0, 876.);
        assert_eq!(break_all("abcdef", 4), vec!["abcd", "ef"]);
        assert!(!is_drag(3., 2.));
        assert!(is_drag(40., 0.));
        assert_eq!(break_all("", 4), vec![""]);
    }

    #[test]
    fn zoom_and_fit() {
        let (x, y, s) = zoom_at((0., 0., 1.), 100., 50., -1.);
        assert!((s - 1.08).abs() < 1e-6);
        assert!((x - (100. - 100. * 1.08)).abs() < 1e-4);
        assert!((y - (50. - 50. * 1.08)).abs() < 1e-4);
        assert_eq!(zoom_at((0., 0., 4.), 0., 0., -1.).2, 4.);
        let (tx, ty, s) = fit(1040., 840., 1000., 400.).unwrap();
        assert_eq!(s, 1.);
        assert_eq!((tx, ty), (20., 220.));
        assert_eq!(fit(100., 100., 0., 0.), None);
    }
}
