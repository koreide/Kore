//! Cluster Dashboard (spec 03 §1): the six cards, the score breakdown, card
//! navigation and node usage (metrics-server fixtures).

use super::support::KoreExt;
use crate::testing::Kore;
use gpui_kit::TestAppContext;
use regex::Regex;

const NODE: &str = "kore-harness-control-plane";
const NODE_2: &str = "kore-harness-2-control-plane";

fn open_dashboard(cx: &mut TestAppContext) -> Kore {
    let mut k = Kore::boot(cx);
    k.click("sidebar.view.dashboard");
    k.wait_until("dashboard view", |k| k.state()["view"] == "dashboard");
    k.wait_for_element("dashboard.card.cluster-health");
    k
}

fn re(s: &str) -> Regex {
    Regex::new(s).unwrap()
}

/// Scenario 30-cluster-dashboard: six cards from live data - score + label,
/// pod legend counts, node status with CPU/Mem, restart hotlist, pending pods
/// and recent warnings.
#[gpui_kit::test]
fn dashboard_cards_show_cluster_data(cx: &mut TestAppContext) {
    let mut k = open_dashboard(cx);
    for (id, title) in [
        ("cluster-health", "Cluster Health"),
        ("pod-health", "Pod Health"),
        ("node-status", "Node Status"),
        ("restart-hotlist", "Restart Hotlist"),
        ("pending-pods", "Pending Pods"),
        ("recent-warnings", "Recent Warnings"),
    ] {
        assert_eq!(k.label(&format!("dashboard.card.{id}")).as_deref(), Some(title));
    }
    // Cluster Health.
    let score: u32 = k.label("dashboard.score.single").unwrap().parse().unwrap();
    assert!(score <= 100);
    let label = k.label("dashboard.score-label.single").unwrap();
    assert!(re("^(Healthy|Degraded|Critical)$").is_match(&label), "{label:?}");
    let summary = k.label("dashboard.summary.single").unwrap();
    assert!(re(r"^\d+ pods across 1 nodes$").is_match(&summary), "{summary:?}");
    assert_eq!(k.label("dashboard.breakdown-toggle.single").as_deref(), Some("How is this calculated?"));
    // Pod Health legend.
    let total: u32 = k.label("dashboard.pod-legend.single.Total").unwrap().parse().unwrap();
    assert_eq!(summary, format!("{total} pods across 1 nodes"));
    let mut sum = 0;
    for name in ["Running", "Pending", "Succeeded"] {
        let v: u32 = k.label(&format!("dashboard.pod-legend.single.{name}")).unwrap_or_else(|| panic!("{name} legend")).parse().unwrap();
        assert!(v > 0, "{name}");
        sum += v;
    }
    assert!(sum <= total);
    // Node Status.
    assert_eq!(k.label(&format!("dashboard.node.{NODE}")).as_deref(), Some("Ready"));
    k.wait_for_element(&format!("dashboard.node.{NODE}.cpu"));
    k.wait_for_element(&format!("dashboard.node.{NODE}.mem"));
    // Restart Hotlist / Pending Pods.
    assert_eq!(k.label("dashboard.hotlist.kore-demo/crashloop").as_deref(), Some("crashloop"));
    assert_eq!(k.label("dashboard.pending.kore-demo/unschedulable").as_deref(), Some("unschedulable"));
    assert_eq!(k.label("dashboard.pending.kore-demo/unschedulable.reason").as_deref(), Some("Unschedulable"));
    // Recent Warnings.
    let reasons: Vec<String> = k
        .labels_with_prefix("dashboard.warning.")
        .into_iter()
        .filter(|(id, _)| id.matches('.').count() == 2)
        .map(|(_, l)| l)
        .collect();
    let objects: Vec<String> = k.labels_with_prefix("dashboard.warning.").into_iter().filter(|(id, _)| id.ends_with(".object")).map(|(_, l)| l).collect();
    for r in ["BackOff", "FailedScheduling"] {
        assert!(reasons.iter().any(|x| x == r), "{r} in {reasons:?}");
    }
    for o in ["Pod/crashloop", "Pod/unschedulable"] {
        assert!(objects.iter().any(|x| x == o), "{o} in {objects:?}");
    }
}

/// Scenario 30-cluster-dashboard-breakdown: "How is this calculated?" expands
/// the breakdown (node/pod ratios, base score, pending penalty, total,
/// footnote); "Hide breakdown" collapses it.
#[gpui_kit::test]
fn dashboard_breakdown_expands_and_collapses(cx: &mut TestAppContext) {
    let mut k = open_dashboard(cx);
    assert!(k.element("dashboard.breakdown.single.total").is_none());
    k.click("dashboard.breakdown-toggle.single");
    k.wait_label("dashboard.breakdown-toggle.single", "Hide breakdown", |l| l == "Hide breakdown");
    let rows: Vec<String> = k
        .labels_with_prefix("dashboard.breakdown.single.")
        .into_iter()
        .filter(|(id, _)| id.rsplit('.').next().is_some_and(|s| s.parse::<usize>().is_ok()))
        .map(|(_, l)| l)
        .collect();
    assert_eq!(rows.first().map(String::as_str), Some("Node health 1/1 ready +30.0"), "{rows:?}");
    assert!(rows.iter().any(|r| re(r"^Pod health \d+/\d+ running \+\d+\.\d$").is_match(r)), "{rows:?}");
    assert!(rows.iter().any(|r| r.starts_with("Base score")), "{rows:?}");
    assert!(rows.iter().any(|r| re(r"^Pending penalty 1 pod -\d+\.\d$").is_match(r)), "{rows:?}");
    let score = k.label("dashboard.score.single").unwrap();
    assert_eq!(k.label("dashboard.breakdown.single.total"), Some(format!("Total {score}")));
    assert!(k.label("dashboard.breakdown.single.note").unwrap().starts_with("Score is clamped to 0-100"));

    k.click("dashboard.breakdown-toggle.single");
    k.wait_gone("dashboard.breakdown.single.total");
    assert!(k.labels_with_prefix("dashboard.breakdown.single.").is_empty());
    assert_eq!(k.label("dashboard.breakdown-toggle.single").as_deref(), Some("How is this calculated?"));
}

/// Scenario 30-cluster-dashboard-navigate: a Restart Hotlist pod and a Pending
/// Pods entry open that pod's details.
#[gpui_kit::test]
fn dashboard_lists_open_pod_details(cx: &mut TestAppContext) {
    let mut k = open_dashboard(cx);
    k.click("dashboard.hotlist.kore-demo/crashloop");
    k.wait_until("crashloop details", |k| k.state()["view"] == "details" && k.state()["selected"]["name"] == "crashloop");
    assert_eq!(k.state()["kind"], "pods");
    assert_eq!(k.state()["selected"]["namespace"], "kore-demo");
    k.wait_for_element("tab.logs");

    k.click("sidebar.view.dashboard");
    k.wait_until("dashboard again", |k| k.state()["view"] == "dashboard");
    k.click("dashboard.pending.kore-demo/unschedulable");
    k.wait_until("unschedulable details", |k| k.state()["view"] == "details" && k.state()["selected"]["name"] == "unschedulable");
    assert_eq!(k.state()["kind"], "pods");
}

/// Scenario 30-cluster-bug-dashboard-usage (quirk 10.12, fixed): Node Status
/// shows live usage from metrics-server over allocatable (not allocatable over
/// capacity); pending ages and warning "last seen" are relative. With
/// Multi-Cluster on, kind-kore-harness-2 (no metrics-server) shows "? / <allocatable>".
#[gpui_kit::test]
fn dashboard_node_usage_is_live_metrics(cx: &mut TestAppContext) {
    let mut k = open_dashboard(cx);
    // NodeMetrics fixture: cpu 136828306n, memory 1772652Ki; allocatable 12 / 14294948Ki.
    let cpu = k.wait_label(&format!("dashboard.node.{NODE}.cpu"), "cpu usage", |l| !l.starts_with('?'));
    let mem = k.label(&format!("dashboard.node.{NODE}.mem")).unwrap();
    assert!(re(r"^(\d+m|\d+\.\d) / \d+(\.\d)?$").is_match(&cpu), "{cpu:?}");
    assert_eq!(cpu, "137m / 12");
    assert!(re(r"^\d+(\.\d)?(Ki|Mi|Gi) / \d+(\.\d)?Gi$").is_match(&mem), "{mem:?}");
    assert_eq!(mem, "1.7Gi / 13.6Gi");
    // Relative ages, no ISO timestamps.
    let age = k.label("dashboard.pending.kore-demo/unschedulable.age").unwrap();
    assert!(re(r"^\d+[smhd]$").is_match(&age), "{age:?}");
    let seen: Vec<String> = k.labels_with_prefix("dashboard.warning.").into_iter().filter(|(id, _)| id.ends_with(".seen")).map(|(_, l)| l).collect();
    assert!(!seen.is_empty());
    for s in &seen {
        assert!(re(r"^\d+[smhd] ago$").is_match(s), "{s:?}");
    }
    let iso = re(r"\d{4}-\d\d-\d\dT\d\d");
    assert!(!k.elements().iter().any(|e| e.id.starts_with("dashboard.") && iso.is_match(&e.label)), "no ISO timestamps");

    // Multi-cluster: the second cluster has no node metrics.
    k.click("sidebar.multi-cluster");
    k.wait_for_element("dashboard.cluster.kind-kore-harness-2");
    let cpu2 = k.wait_label(&format!("dashboard.node.{NODE_2}.cpu"), "fallback cpu", |_| true);
    assert_eq!(cpu2, "? / 12");
    assert_eq!(k.label(&format!("dashboard.node.{NODE_2}.mem")).as_deref(), Some("? / 13.6Gi"));
    k.click("sidebar.multi-cluster");
    k.wait_gone("dashboard.cluster.kind-kore-harness-2");
    assert_eq!(k.state()["multiCluster"], false);
}
