//! 403 handling (quirks 10.15 / 10.16, the RBAC "Permission Denied" banner of
//! spec 03 §7.8). The fake cluster answers `forbid`den calls like the API
//! server ("User \"fake\" cannot list resource ..."), so the banner names the
//! identity `fake` (the harness used a restricted ServiceAccount instead).

use super::support::KoreExt;
use crate::testing::Kore;
use gpui_kit::TestAppContext;
use kore_core::ResourceKind;
use serde_json::json;
use std::time::Duration;

/// Fire the Network Policies view's 30 s poll (GPUI's test clock).
fn poll_network_policies(k: &mut Kore) {
    k.cx.executor().advance_clock(Duration::from_secs(31));
    k.frame();
}

/// Scenario 30-cluster-bug-forbidden, Network Policies part: a refresh that
/// fails with a 403 keeps the last good graph with a "Refresh failed" indicator
/// and a "Permission Denied" banner (Dismiss hides it); once allowed again the
/// next refresh clears the error.
#[gpui_kit::test]
fn forbidden_netpol_refresh_keeps_data_and_shows_permission_denied(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.show(ResourceKind::Pods, "kore-demo");
    k.open_view("network-policies");
    k.wait_for_element("np.policy.default-deny-ingress");
    k.wait_for_element("np.policy.allow-logger-to-web");
    assert!(k.element("np.error").is_none());

    k.cluster.forbid("list", "networkpolicies", Some("kore-demo"));
    poll_network_policies(&mut k);
    let err = k.wait_label("np.error", "forbidden", |l| l.contains("forbidden"));
    assert!(err.contains("networkpolicies"), "{err}");
    // The last good data stays (not the full "Failed to load" state).
    assert_eq!(k.label("view.network-policies").as_deref(), Some("Network Policies"));
    k.wait_for_element("np.policy.default-deny-ingress");
    // Cards only (`np.policy.<name>`), not their `.ns`/`.types`/... parts.
    assert_eq!(k.ids_with_prefix("np.policy.").iter().filter(|id| id.split('.').count() == 3).count(), 2);
    k.wait_label("np.forbidden", "denied line", |l| l == "fake cannot list networkpolicies in namespace \"kore-demo\"");

    k.click("np.forbidden.dismiss");
    k.wait_gone("np.forbidden");
    assert!(k.element("np.error").is_some(), "Refresh failed stays after Dismiss");
    // The same error on the next poll keeps the dismissal.
    poll_network_policies(&mut k);
    k.frame();
    assert!(k.element("np.forbidden").is_none());

    k.cluster.allow_all();
    poll_network_policies(&mut k);
    k.wait_gone("np.error");
    k.wait_for_element("np.policy.default-deny-ingress");
}

/// Scenario 30-cluster-bug-forbidden, Custom Resources part: listing CRDs is
/// forbidden -> the banner names customresourcedefinitions and shows the
/// closest matching rules (the identity holds "view", which lists other resources).
#[gpui_kit::test]
fn forbidden_crd_list_shows_banner_with_closest_rules(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    // Bind the fake's user to the built-in "view" ClusterRole (like the harness's kore-bug-bot).
    k.cluster.apply(json!({
        "apiVersion": "rbac.authorization.k8s.io/v1",
        "kind": "ClusterRoleBinding",
        "metadata": {"name": "fake-view"},
        "roleRef": {"apiGroup": "rbac.authorization.k8s.io", "kind": "ClusterRole", "name": "view"},
        "subjects": [{"apiGroup": "rbac.authorization.k8s.io", "kind": "User", "name": "fake"}]
    }));
    k.cluster.forbid("list", "customresourcedefinitions", None);
    k.show(ResourceKind::Pods, "kore-demo");
    k.open_view("crds");
    k.wait_label("crd.forbidden", "denied line", |l| l == "fake cannot list customresourcedefinitions");
    let chain = k.wait_label("crd.forbidden.chain.0", "closest rule", |l| !l.is_empty());
    assert!(chain.starts_with("ClusterRoleBinding fake-view -> ClusterRole view -> "), "{chain}");
    assert!(k.element("crd.item.widgets.kore.dev").is_none());
}

/// Scenario 30-cluster-bug-forbidden, RBAC part (quirk 10.15): with RBAC reads
/// forbidden the simulator header shows the error, and a "Who Can?" query shows
/// "Query failed" with the error (no longer swallowed) plus the banner.
#[gpui_kit::test]
fn forbidden_rbac_reads_surface_query_errors(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.cluster.forbid("list", "clusterroles", None);
    k.show(ResourceKind::Pods, "kore-demo");
    k.open_view("rbac");
    k.wait_label("rbac.error", "forbidden", |l| l.contains("forbidden"));
    k.keys("3");
    k.click("rbac.query.mode.whocan");
    k.click("rbac.query.check");
    let err = k.wait_label("rbac.query.error", "forbidden", |l| l.contains("forbidden"));
    assert!(err.contains("clusterroles"), "{err}");
    assert!(k.element("rbac.query.result").is_none() && k.element("rbac.whocan.title").is_none());
    k.wait_label("rbac.forbidden", "denied line", |l| l.starts_with("fake cannot list "));
}
