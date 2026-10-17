//! CRD browser (spec 03 §4) against the fixtures' widgets.kore.dev
//! (blue-widget, red-widget in kore-demo).

use super::support::KoreExt;
use crate::testing::Kore;
use gpui_kit::TestAppContext;
use kore_core::ResourceKind;
use serde_json::json;

fn open_crds(cx: &mut TestAppContext) -> Kore {
    let mut k = Kore::boot(cx);
    k.show(ResourceKind::Pods, "kore-demo");
    k.open_view("dashboard");
    k.open_view("crds");
    k.wait_for_element("view.crds");
    k
}

/// Scenario 30-cluster-crd: widgets.kore.dev is discovered under group kore.dev,
/// the search filters the list, the Widget instances of kore-demo are listed,
/// an instance opens as JSON, and Esc walks back detail -> list -> no
/// selection -> resource table.
#[gpui_kit::test]
fn crd_browser_lists_instances_opens_json_and_esc_walks_back(cx: &mut TestAppContext) {
    let mut k = open_crds(cx);
    k.wait_for_element("crd.group.kore.dev");
    k.wait_for_element("crd.item.widgets.kore.dev");
    assert_eq!(k.label("crd.item.widgets.kore.dev").as_deref(), Some("Widget"));

    k.click("crd.search");
    k.type_text("wid");
    k.wait_for_element("crd.item.widgets.kore.dev");
    assert_eq!(k.ids_with_prefix("crd.item.").len(), 1, "only widgets match 'wid'");
    k.click("crd.item.widgets.kore.dev");
    k.wait_for_element("crd.row.kore-demo/blue-widget");
    k.wait_for_element("crd.row.kore-demo/red-widget");
    assert_eq!(k.label("crd.count").as_deref(), Some("2"));
    assert_eq!(k.label("crd.scope").as_deref(), Some("kore.dev/v1 - Namespaced"));
    // Namespaced: the instances were listed in the selected namespace.
    assert!(
        k.cluster.requests().iter().any(|r| r.method == "GET" && r.path == "/apis/kore.dev/v1/namespaces/kore-demo/widgets"),
        "namespaced list"
    );

    k.click("crd.row.kore-demo/blue-widget");
    k.wait_for_element("crd.back-to-list");
    let json = k.wait_label("crd.detail.json", "instance JSON", |l| l.contains("\"kind\": \"Widget\""));
    assert!(json.contains("\"color\": \"blue\""), "{json}");
    assert!(json.contains("\"size\": 3"), "{json}");

    k.keys("escape");
    k.wait_gone("crd.detail.json");
    k.wait_for_element("crd.row.kore-demo/red-widget");
    k.keys("escape");
    k.wait_gone("crd.table");
    assert_eq!(k.state()["view"], "crds");
    k.keys("escape");
    k.wait_until("back to the table", |k| k.state()["view"] == "table");
}

/// Scenario 30-cluster-crd-delete: the row's trash icon asks "Delete Widget" with
/// the name/namespace message, deletes on confirm, toasts "Resource deleted"
/// and removes the row.
#[gpui_kit::test]
fn crd_instance_delete_confirms_and_removes_row(cx: &mut TestAppContext) {
    let mut k = open_crds(cx);
    k.click("crd.item.widgets.kore.dev");
    k.wait_for_element("crd.row.kore-demo/red-widget");
    k.click("crd.delete.kore-demo/red-widget");
    k.wait_label("confirm", "Delete Widget", |l| l == "Delete Widget");
    assert_eq!(
        k.confirm_message().as_deref(),
        Some("Are you sure you want to delete \"red-widget\" in namespace \"kore-demo\"? This action cannot be undone.")
    );
    assert!(k.element("crd.detail.json").is_none(), "the trash icon does not open the row");
    k.wait_for_element("confirm.cancel");
    k.cluster.clear_requests();
    k.click("confirm.ok");
    k.wait_until("toast", |k| k.toasts().iter().any(|t| t == "Resource deleted"));
    k.wait_gone("crd.row.kore-demo/red-widget");
    k.wait_gone("confirm");
    k.wait_for_element("crd.row.kore-demo/blue-widget");
    assert!(k.cluster.get("Widget", Some("kore-demo"), "red-widget").is_none());
    assert!(k
        .cluster
        .requests()
        .iter()
        .any(|r| r.method == "DELETE" && r.path == "/apis/kore.dev/v1/namespaces/kore-demo/widgets/red-widget"));
}

/// The temporary cluster-scoped CRD of the bug scenario.
fn apply_gadgets(k: &Kore) {
    k.cluster.apply(json!({
        "apiVersion": "apiextensions.k8s.io/v1",
        "kind": "CustomResourceDefinition",
        "metadata": {"name": "gadgets.kore-bug.dev"},
        "spec": {
            "group": "kore-bug.dev",
            "scope": "Cluster",
            "names": {"plural": "gadgets", "singular": "gadget", "kind": "Gadget"},
            "versions": [{"name": "v1", "served": true, "storage": true,
                "schema": {"openAPIV3Schema": {"type": "object", "properties": {"spec": {"type": "object", "properties": {"color": {"type": "string"}}}}}}}]
        }
    }));
    k.cluster.apply(json!({
        "apiVersion": "kore-bug.dev/v1",
        "kind": "Gadget",
        "metadata": {"name": "bug-gadget"},
        "spec": {"color": "green"}
    }));
}

/// Scenario 30-cluster-bug-crd-cluster-scoped (quirk 10.10): instances of a
/// cluster-scoped CRD are listed, opened and deleted without a namespace
/// (namespace column "-", JSON instead of "Error fetching instance", a
/// namespace-free delete message).
#[gpui_kit::test]
fn cluster_scoped_crd_instance_opens_and_deletes_without_namespace(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    apply_gadgets(&k);
    k.show(ResourceKind::Pods, "kore-demo");
    k.open_view("dashboard");
    k.open_view("crds");
    k.click("crd.search");
    k.type_text("gadget");
    k.click("crd.item.gadgets.kore-bug.dev");
    // Namespace cell "-" (the row key is `<ns cell>/<name>`).
    k.wait_for_element("crd.row.-/bug-gadget");
    assert_eq!(k.label("crd.count").as_deref(), Some("1"));
    assert_eq!(k.label("crd.scope").as_deref(), Some("kore-bug.dev/v1 - Cluster"));
    let paths: Vec<String> = k.cluster.requests().into_iter().filter(|r| r.path.contains("gadgets")).map(|r| r.path).collect();
    assert!(paths.iter().all(|p| !p.contains("/namespaces/")), "cluster-scoped list: {paths:?}");

    k.cluster.clear_requests();
    k.click("crd.row.-/bug-gadget");
    let json = k.wait_label("crd.detail.json", "gadget JSON", |l| l.contains("\"color\": \"green\""));
    assert!(json.contains("\"kind\": \"Gadget\""), "{json}");
    assert!(json.contains("\"name\": \"bug-gadget\""), "{json}");
    assert!(!json.contains("Error fetching instance"), "{json}");
    assert!(k.cluster.requests().iter().any(|r| r.method == "GET" && r.path == "/apis/kore-bug.dev/v1/gadgets/bug-gadget"));

    k.keys("escape");
    k.wait_for_element("crd.row.-/bug-gadget");
    k.click("crd.delete.-/bug-gadget");
    k.wait_label("confirm", "Delete Gadget", |l| l == "Delete Gadget");
    let msg = k.confirm_message().unwrap();
    assert_eq!(msg, "Are you sure you want to delete \"bug-gadget\"? This action cannot be undone.");
    k.click("confirm.ok");
    k.wait_until("toast", |k| k.toasts().iter().any(|t| t == "Resource deleted"));
    k.wait_gone("crd.row.-/bug-gadget");
    assert!(k.cluster.get("Gadget", None, "bug-gadget").is_none());
    assert!(k.cluster.requests().iter().any(|r| r.method == "DELETE" && r.path == "/apis/kore-bug.dev/v1/gadgets/bug-gadget"));
}
