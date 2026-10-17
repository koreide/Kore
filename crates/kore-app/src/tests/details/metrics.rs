//! Pod Metrics tab (metrics.k8s.io objects from the fixtures / seeded here).

use super::helpers::{running_pod, DetailsExt, NS};
use crate::testing::Kore;
use gpui_kit::TestAppContext;
use serde_json::json;

/// Scenario 20-details-metrics: a logger pod without requests/limits shows
/// CPU and Memory cards with "used / limit" (limit = usage x 2, so 50.0%
/// gauges) and the "Usage Trend (Last 2 Minutes)" chart. (Chart drawing and
/// gauge colors are Layer 2.)
#[gpui_kit::test]
fn metrics_cards_show_usage_over_limit(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    // Fixture usage: cpu 2725382n (2.7m), memory 428Ki.
    k.open_pod("logger-684d79cb86-26m9x");
    k.keys("4");
    k.wait_for_element("metrics.cpu");
    assert_eq!(k.wait_for_element("metrics.cpu.usage").label, "3 m / 5 m");
    assert_eq!(k.label("metrics.memory.usage").as_deref(), Some("428.00 KB / 856.00 KB"));
    assert_eq!(k.label("metrics.cpu.percent").as_deref(), Some("50.0%"));
    assert_eq!(k.label("metrics.memory.percent").as_deref(), Some("50.0%"));
    assert_eq!(k.wait_for_element("metrics.trend").label, "Usage Trend (Last 2 Minutes)");
    assert!(k.cluster.requests().iter().any(|r| r.path == "/apis/metrics.k8s.io/v1beta1/namespaces/kore-demo/pods/logger-684d79cb86-26m9x"));
}

/// Scenario 20-details-bug-metrics-bytes: quantities are parsed per resource
/// type: a memory limit given as a plain byte count (200000001) is ~190.7 MiB
/// (not x1000) and the CPU limit "1" is one core.
#[gpui_kit::test]
fn metrics_parse_plain_byte_and_core_limits(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    let mut pod = running_pod("kore-bug-metrics", &["main"]);
    pod["spec"]["containers"][0]["resources"] =
        json!({"limits": {"cpu": "1", "memory": "200000001"}, "requests": {"cpu": "1", "memory": "200000001"}});
    k.cluster.apply(pod);
    k.cluster.apply(json!({
        "apiVersion": "metrics.k8s.io/v1beta1", "kind": "PodMetrics",
        "metadata": {"name": "kore-bug-metrics", "namespace": NS},
        "timestamp": chrono::Utc::now().to_rfc3339(), "window": "15s",
        "containers": [{"name": "main", "usage": {"cpu": "10m", "memory": "50Mi"}}]
    }));
    k.open_pod("kore-bug-metrics");
    k.keys("4");
    assert_eq!(k.wait_for_element("metrics.cpu.usage").label, "10 m / 1.00 cores");
    let mem = k.label("metrics.memory.usage").unwrap();
    assert_eq!(mem, "50.00 MB / 190.73 MB");
    assert!(!mem.contains("GB"));
    assert_eq!(k.label("metrics.cpu.percent").as_deref(), Some("1.0%"));
}
