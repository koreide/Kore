//! Resource Graph (spec 03): count badge, chips, filter, wheel zoom, drag pan,
//! Fit, hover tooltip and node clicks.

use super::support::KoreExt;
use crate::testing::Kore;
use gpui_kit::{point, px, Modifiers, MouseButton, Pixels, Point, ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase};
use kore_automation::Bounds;
use serde_json::json;

const KINDS: [&str; 7] = ["Ingress", "Service", "Deployment", "CronJob", "ReplicaSet", "Job", "Pod"];
const TICKER: &str = "graph.node.CronJob/kore-demo/ticker";
const CRASHLOOP: &str = "graph.node.Pod/kore-demo/crashloop";

fn open_graph(cx: &mut TestAppContext) -> Kore {
    let mut k = Kore::boot(cx);
    k.action("set_namespace", json!({"namespace": "kore-demo"}));
    k.click("sidebar.view.graph");
    k.wait_until("graph view", |k| k.state()["view"] == "graph");
    k.wait_for_element(TICKER);
    // Normalize the initial (deferred) fit.
    k.click("graph.fit");
    k
}

fn node_bounds(k: &mut Kore) -> Vec<(String, Bounds)> {
    k.elements().into_iter().filter(|e| e.id.starts_with("graph.node.")).map(|e| (e.id, e.bounds)).collect()
}

fn dimmed(k: &mut Kore) -> Vec<String> {
    k.elements().into_iter().filter(|e| e.id.starts_with("graph.node.") && e.disabled).map(|e| e.id).collect()
}

fn zoom(k: &mut Kore) -> i64 {
    let l = k.label("graph.zoom").expect("zoom label");
    l.strip_suffix('%').and_then(|n| n.parse().ok()).unwrap_or_else(|| panic!("zoom label {l:?}"))
}

fn at(x: f32, y: f32) -> Point<Pixels> {
    point(px(x), px(y))
}

/// An empty spot of the canvas (left of the ingress row, below the toolbar).
fn background(k: &mut Kore) -> Point<Pixels> {
    let v = k.wait_for_element("view.graph").bounds;
    let p = at(v.x + 40., v.y + 200.);
    assert!(
        !node_bounds(k).iter().any(|(_, b)| p.x.as_f32() >= b.x && p.x.as_f32() <= b.x + b.w && p.y.as_f32() >= b.y && p.y.as_f32() <= b.y + b.h),
        "background point hits a node"
    );
    p
}

fn wheel(k: &mut Kore, p: Point<Pixels>, dy: f32) {
    k.cx.simulate_event(ScrollWheelEvent {
        position: p,
        delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
        modifiers: Modifiers::default(),
        touch_phase: TouchPhase::Moved,
    });
    k.frame();
}

/// Scenario 30-cluster-graph: count badge, the kind chips (all on), Fit, the
/// name filter; the wheel zooms (label changes), dragging the background pans
/// the nodes, Fit restores zoom and positions.
#[gpui_kit::test]
fn graph_zoom_pan_and_fit(cx: &mut TestAppContext) {
    let mut k = open_graph(&mut *cx);
    let count = k.label("graph.count").unwrap();
    let re = regex::Regex::new(r"^(\d+) nodes · (\d+) edges$").unwrap();
    let caps = re.captures(&count).unwrap_or_else(|| panic!("count badge {count:?}"));
    let nodes: usize = caps[1].parse().unwrap();
    assert!(caps[2].parse::<usize>().unwrap() > 0);
    assert_eq!(nodes, node_bounds(&mut k).len());
    for kind in KINDS {
        assert!(k.is_selected(&format!("graph.chip.{kind}")), "{kind} chip enabled");
    }
    assert_eq!(k.label("graph.filter").as_deref(), Some("Filter by name..."));
    assert!(dimmed(&mut k).is_empty());

    let z0 = zoom(&mut k);
    let b0 = node_bounds(&mut k);

    // Wheel up over the web Ingress: zoom in.
    let ing = k.wait_for_element("graph.node.Ingress/kore-demo/web").bounds;
    wheel(&mut k, at(ing.x + ing.w / 2., ing.y + ing.h / 2.), 200.);
    k.wait_until("zoomed in", |k| zoom(k) > z0);
    let after_zoom = k.wait_for_element(TICKER).bounds;
    assert!(after_zoom.w > b0.iter().find(|(id, _)| id == TICKER).unwrap().1.w, "nodes grow with zoom");

    // Drag the background: every node moves by the drag delta.
    let before = k.wait_for_element(TICKER).bounds;
    let p = background(&mut k);
    let q = at(p.x.as_f32() + 100., p.y.as_f32() + 60.);
    k.cx.simulate_mouse_down(p, MouseButton::Left, Modifiers::default());
    k.cx.simulate_mouse_move(at(p.x.as_f32() + 50., p.y.as_f32() + 30.), Some(MouseButton::Left), Modifiers::default());
    k.cx.simulate_mouse_move(q, Some(MouseButton::Left), Modifiers::default());
    k.cx.simulate_mouse_up(q, MouseButton::Left, Modifiers::default());
    k.frame();
    let moved = k.wait_for_element(TICKER).bounds;
    assert!((moved.x - before.x - 100.).abs() < 0.5 && (moved.y - before.y - 60.).abs() < 0.5, "pan {before:?} -> {moved:?}");
    assert_eq!(k.state()["view"], "graph", "a drag is not a click");

    k.click("graph.fit");
    k.wait_until("fit restores zoom", |k| zoom(k) == z0);
    assert_eq!(node_bounds(&mut k), b0, "Fit restores the layout");
}

/// Scenario 30-cluster-graph-filter: the Pod chip dims pods (layout unchanged)
/// and toggles back; the name filter dims non-matching nodes and its X clears
/// it; clicking a Pod node opens its details.
#[gpui_kit::test]
fn graph_kind_chip_and_name_filter_dim_nodes(cx: &mut TestAppContext) {
    let mut k = open_graph(&mut *cx);
    let b0 = node_bounds(&mut k);

    k.click("graph.chip.Pod");
    assert!(!k.is_selected("graph.chip.Pod"));
    let d = dimmed(&mut k);
    let pods: Vec<String> = b0.iter().map(|(id, _)| id.clone()).filter(|id| id.starts_with("graph.node.Pod/")).collect();
    assert!(!pods.is_empty());
    for p in &pods {
        assert!(d.contains(p), "{p} dimmed");
    }
    assert!(d.iter().all(|id| id.starts_with("graph.node.Pod/")), "only pods dimmed: {d:?}");
    assert_eq!(node_bounds(&mut k), b0, "layout unchanged");
    k.click("graph.chip.Pod");
    assert!(k.is_selected("graph.chip.Pod"));
    assert!(dimmed(&mut k).is_empty());

    k.click("graph.filter");
    k.type_text("crashloop");
    k.wait_until("name filter applied", |k| !dimmed(k).is_empty());
    let d = dimmed(&mut k);
    assert!(!d.contains(&CRASHLOOP.to_string()));
    assert_eq!(d.len(), b0.len() - 1, "everything but crashloop dimmed");
    assert_eq!(node_bounds(&mut k), b0, "layout unchanged");
    k.click("graph.filter.clear");
    k.wait_until("filter cleared", |k| dimmed(k).is_empty());
    k.wait_gone("graph.filter.clear");

    k.click(CRASHLOOP);
    k.wait_until("pod details", |k| k.state()["view"] == "details" && k.state()["selected"]["name"] == "crashloop");
    assert_eq!(k.state()["kind"], "pods");
    k.wait_for_element("tab.logs");
}

/// Scenario 30-cluster-bug-graph-replicaset-tooltip (quirk 10.21, fixed):
/// hovering a node shows the tooltip (name, kind, status, namespace); moving
/// off the node hides it.
#[gpui_kit::test]
fn graph_hover_shows_tooltip(cx: &mut TestAppContext) {
    let mut k = open_graph(&mut *cx);
    k.hover(TICKER);
    k.wait_label("graph.tooltip", "ticker", |l| l == "ticker");
    assert_eq!(k.label("graph.tooltip.kind").as_deref(), Some("CronJob Active"));
    assert_eq!(k.label("graph.tooltip.ns").as_deref(), Some("ns: kore-demo"));
    let p = background(&mut k);
    k.cx.simulate_mouse_move(p, None, Modifiers::default());
    k.frame();
    k.wait_gone("graph.tooltip");
}

/// Scenario 30-cluster-bug-graph-replicaset-tooltip (quirk 10.11, fixed):
/// clicking a ReplicaSet node opens its owning Deployment (web).
#[gpui_kit::test]
fn graph_replicaset_click_opens_owning_deployment(cx: &mut TestAppContext) {
    let mut k = open_graph(&mut *cx);
    let rs = k
        .elements()
        .into_iter()
        .find(|e| e.id.starts_with("graph.node.ReplicaSet/kore-demo/web-"))
        .expect("web ReplicaSet node");
    k.click(&rs.id);
    k.wait_until("deployment details", |k| k.state()["view"] == "details");
    assert_eq!(k.state()["selected"]["name"], "web");
    assert_eq!(k.state()["kind"], "deployments");
    k.wait_for_element("deployment.scale");
}
