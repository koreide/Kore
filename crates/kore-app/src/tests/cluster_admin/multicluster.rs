//! Multi-cluster mode (spec 03 §8): the Dashboard per context and the
//! cross-cluster table, over both harness contexts.

use super::support::KoreExt;
use crate::testing::Kore;
use gpui_kit::TestAppContext;
use kore_testkit::{fixtures, FakeCluster};
use std::time::{Duration, Instant};

fn is_score(l: &str) -> bool {
    l.split_once(" - ")
        .is_some_and(|(n, s)| n.parse::<u32>().is_ok() && ["Healthy", "Degraded", "Critical"].contains(&s))
}

/// Scenario 30-cluster-multicluster: with the toggle on, the Dashboard shows one
/// section per context with a score pill, and Deployments across all namespaces
/// include kind-kore-harness-2's remote-web (CLUSTER column); toggling off drops it.
#[gpui_kit::test]
fn multicluster_dashboard_and_table_span_both_contexts(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    assert_eq!(k.state()["multiCluster"], false);
    k.click("sidebar.multi-cluster");
    k.wait_until("multi-cluster on", |k| k.state()["multiCluster"] == true);
    k.open_view("dashboard");
    for ctx in [fixtures::CONTEXT, fixtures::CONTEXT_2] {
        k.wait_for_element(&format!("dashboard.cluster.{ctx}"));
        let score = k.wait_label(&format!("dashboard.cluster.{ctx}.score"), "score pill", is_score);
        assert!(is_score(&score), "{ctx}: {score}");
    }
    // Each context's health came from its own cluster.
    assert!(k.cluster_2.requests().iter().any(|r| r.path == "/api/v1/pods"), "kind-kore-harness-2 was queried");

    k.click("sidebar.namespace.trigger");
    k.click("sidebar.namespace.item.*");
    k.wait_until("all namespaces", |k| k.state()["namespace"] == "*");
    k.wait_gone("sidebar.namespace.panel");
    k.click("sidebar.kind.deployments");
    k.advance(Duration::from_millis(600)); // the table's refresh delay
    k.wait_for_element("table.row.kore-remote/remote-web");
    k.wait_for_element("table.header._context");
    k.wait_for_element("table.row.kore-demo/logger");
    let ctx_of = |k: &mut Kore, name: &str| {
        let name = name.to_string();
        k.app_state(move |s, cx| s.resources.read(cx).rows.iter().find(|r| r.name == name).and_then(|r| r.context.clone()))
    };
    assert_eq!(ctx_of(&mut k, "remote-web").as_deref(), Some(fixtures::CONTEXT_2));
    assert_eq!(ctx_of(&mut k, "logger").as_deref(), Some(fixtures::CONTEXT));

    k.click("sidebar.multi-cluster");
    k.wait_until("multi-cluster off", |k| k.state()["multiCluster"] == false);
    k.advance(Duration::from_millis(600));
    k.wait_gone("table.row.kore-remote/remote-web");
    assert!(k.element("table.header._context").is_none());
}

/// Scenario 30-cluster-multicluster-timeout-fix (Fix 9): an extra context whose
/// API server never answers does not stall the multi-cluster Dashboard: after
/// the per-context timeout (`MULTI_CLUSTER_CONTEXT_TIMEOUT`, a real 5 s on
/// Core's runtime, so this test takes ~5 s) both reachable clusters are shown
/// and the unreachable one is skipped.
#[gpui_kit::test]
fn multicluster_dashboard_skips_unreachable_context_after_timeout(cx: &mut TestAppContext) {
    let (env, c1, c2) = fixtures::harness_env();
    let hung = FakeCluster::new();
    hung.set_hang(true);
    let env = env.context("kore-unreachable", &hung);
    let mut k = Kore::boot_env(cx, env, c1, c2);

    // Precondition: the kubeconfig really has the unreachable context.
    k.click("sidebar.context.trigger");
    k.wait_for_element("sidebar.context.item.kore-unreachable");
    k.keys("escape");
    k.wait_gone("sidebar.context.panel");

    k.click("sidebar.multi-cluster");
    k.wait_until("multi-cluster on", |k| k.state()["multiCluster"] == true);
    let start = Instant::now();
    k.open_view("dashboard");
    k.wait_until_for("both reachable clusters", Duration::from_secs(12), |k| {
        k.element(&format!("dashboard.cluster.{}", fixtures::CONTEXT)).is_some()
            && k.element(&format!("dashboard.cluster.{}", fixtures::CONTEXT_2)).is_some()
    });
    assert!(start.elapsed() < Duration::from_secs(12), "{:?}", start.elapsed());
    assert!(k.element("dashboard.cluster.kore-unreachable").is_none());
    assert!(!hung.requests().is_empty(), "the unreachable context was tried");
}
