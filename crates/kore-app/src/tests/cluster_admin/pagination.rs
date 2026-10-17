//! Resource lists page through the API (spec 04 Appendix A4).

use super::support::KoreExt;
use crate::testing::Kore;
use gpui_kit::TestAppContext;
use kore_core::ResourceKind;
use kore_testkit::fixtures;
use serde_json::json;

/// Scenario 30-cluster-bug-list-pagination: 600 ConfigMaps plus kube-root-ca.crt
/// in a namespace; the table counts 601 (the list follows `continue` tokens
/// instead of stopping at the 500-object page) and the last one is found by search.
#[gpui_kit::test]
fn configmap_list_follows_continue_tokens_past_500(cx: &mut TestAppContext) {
    let (env, c1, c2) = fixtures::harness_env();
    c1.apply(json!({"apiVersion": "v1", "kind": "Namespace", "metadata": {"name": "kore-page-test"}}));
    c1.apply(json!({"apiVersion": "v1", "kind": "ConfigMap",
        "metadata": {"name": "kube-root-ca.crt", "namespace": "kore-page-test"}, "data": {"ca.crt": "x"}}));
    for i in 1..=600 {
        c1.apply(json!({"apiVersion": "v1", "kind": "ConfigMap",
            "metadata": {"name": format!("page-cm-{i:03}"), "namespace": "kore-page-test"}, "data": {"k": "v"}}));
    }
    let mut k = Kore::boot_env(cx, env, c1, c2);
    k.cluster.clear_requests();
    // The table's watch re-lists everything itself (kube-runtime's watcher pages
    // too), which would hide a list that stops at 500. With latency on every
    // request, the watch's objects arrive well after the list's result, so the
    // first non-empty row count is the list's own.
    k.cluster.set_latency(Some(std::time::Duration::from_millis(150)));
    k.show(ResourceKind::Configmaps, "kore-page-test");
    k.advance(std::time::Duration::from_millis(600)); // the table's refresh delay
    k.wait_until("list result", |k| k.state()["rowCount"].as_u64().unwrap_or(0) > 0);
    assert_eq!(k.state()["rowCount"], 601, "the list itself pages past 500");
    k.cluster.set_latency(None);
    k.wait_label("header.count", "601", |l| l == "601");

    let pages: Vec<_> = k
        .cluster
        .requests()
        .into_iter()
        .filter(|r| r.method == "GET" && r.path == "/api/v1/namespaces/kore-page-test/configmaps" && !r.query.contains_key("watch"))
        .collect();
    assert!(pages.iter().any(|r| r.query.get("limit").map(String::as_str) == Some("500") && !r.query.contains_key("continue")), "{pages:?}");
    assert!(pages.iter().any(|r| r.query.get("continue").map(String::as_str) == Some("500")), "second page: {pages:?}");

    k.keys("/");
    k.type_text("page-cm-600");
    k.wait_label("header.count", "1", |l| l == "1");
    k.wait_for_element("table.row.kore-page-test/page-cm-600");
}
