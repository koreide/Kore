//! Row-level features: live watch, actions menu, delete + confirm dialog, pins,
//! multi-cluster rows and restart sparklines (spec 01 §4, §7, §9, §10).

use super::helpers::*;
use crate::testing::Kore;
use gpui_kit::{point, px, Modifiers, TestAppContext};
use kore_core::ResourceKind;

fn menu_labels(k: &mut Kore) -> Vec<String> {
    let mut items: Vec<_> = k.elements().into_iter().filter(|e| e.id.starts_with("table.menu.")).collect();
    items.sort_by(|a, b| a.bounds.y.total_cmp(&b.bounds.y));
    items.into_iter().map(|e| e.label).collect()
}

fn deletes(k: &Kore) -> Vec<String> {
    let mut all: Vec<String> = Vec::new();
    for c in [&k.cluster, &k.cluster_2] {
        all.extend(c.requests().into_iter().filter(|r| r.method == "DELETE").map(|r| r.path));
    }
    all
}

/// Scenario 10-shell-table-watch (behavior part): a pod created in the cluster
/// appears without a refresh and disappears again when deleted.
#[gpui_kit::test]
fn watch_adds_and_removes_rows_live(cx: &mut TestAppContext) {
    let mut k = pods_kore_demo(cx);
    let n = k.row_keys().len();
    assert!(k.element("table.row.kore-demo/shell-watch-probe").is_none());
    k.cluster.clear_requests();

    k.cluster.apply(probe_pod("shell-watch-probe"));
    k.wait_for_element("table.row.kore-demo/shell-watch-probe");
    k.wait_label("header.count", "n+1", |l| l == (n + 1).to_string());
    assert_eq!(k.cell("kore-demo/shell-watch-probe", "status"), "Running");

    k.cluster.delete("Pod", Some("kore-demo"), "shell-watch-probe");
    k.wait_gone("table.row.kore-demo/shell-watch-probe");
    k.wait_label("header.count", "n", |l| l == n.to_string());
    // Through the watch: no list request was needed.
    assert!(
        !k.cluster.requests().iter().any(|r| r.method == "GET" && r.path.ends_with("/pods") && !r.query.contains_key("watch")),
        "unexpected re-list: {:?}",
        k.cluster.requests()
    );
}

/// Scenario 10-shell-row-actions: the "..." button opens a kind-specific menu
/// without opening the row; clicking elsewhere closes it; a menu action opens
/// the row's details.
#[gpui_kit::test]
fn row_actions_menu_is_kind_specific(cx: &mut TestAppContext) {
    let mut k = pods_kore_demo(cx);

    k.click("table.actions.kore-demo/crashloop");
    k.wait_for_element("table.menu.logs");
    assert_eq!(menu_labels(&mut k), ["View Logs", "Exec Shell", "Port Forward", "Delete"]);
    assert_eq!(k.state()["view"], "table");
    assert_eq!(k.state()["selected"], serde_json::Value::Null);

    k.click("header.breadcrumb");
    k.wait_gone("table.menu.logs");

    k.click("table.actions.kore-demo/crashloop");
    k.click("table.menu.logs");
    k.wait_until("crashloop details", |k| k.state()["view"] == "details" && k.state()["selected"]["name"] == "crashloop");
    k.keys("h");
    k.wait_until("back", |k| k.state()["view"] == "table");

    k.click_sidebar("sidebar.kind.deployments");
    k.click("table.actions.kore-demo/logger");
    k.wait_for_element("table.menu.scale");
    assert_eq!(menu_labels(&mut k), ["Scale", "Restart", "Delete"]);
    k.click("header.breadcrumb");
    k.wait_gone("table.menu.scale");

    k.click_sidebar("sidebar.kind.services");
    k.click("table.actions.kore-demo/web");
    k.wait_for_element("table.menu.describe");
    assert_eq!(menu_labels(&mut k), ["Describe", "Delete"]);
    k.click("header.breadcrumb");
    k.wait_gone("table.menu.describe");
    assert_eq!(k.state()["view"], "table");
}

/// Scenario 10-shell-confirm-delete: row action Delete opens the danger
/// ConfirmDialog; Cancel closes it; Delete removes the pod, shows the success
/// toast (auto-dismissed) and the row disappears via the watch.
#[gpui_kit::test]
fn row_menu_delete_confirms_then_deletes(cx: &mut TestAppContext) {
    let mut k = pods_kore_demo(cx);
    k.cluster.apply(probe_pod("shell-delete-probe"));
    let row = "table.row.kore-demo/shell-delete-probe";
    k.click("header.search");
    k.type_text("shell-delete-probe");
    k.blur();
    k.wait_until("only the probe", |k| k.row_names() == ["shell-delete-probe"]);

    k.click("table.actions.kore-demo/shell-delete-probe");
    k.click("table.menu.delete");
    k.wait_label("confirm", "Delete Pod", |l| l == "Delete Pod");
    assert_eq!(
        k.element("confirm.message").unwrap().label,
        "Are you sure you want to delete \"shell-delete-probe\" in namespace \"kore-demo\"?"
    );
    assert_eq!(k.element("confirm.cancel").unwrap().label, "Cancel");
    assert_eq!(k.element("confirm.ok").unwrap().label, "Delete");
    assert!(k.element("table.menu.delete").is_none(), "menu closes on the action");

    k.click("confirm.cancel");
    k.wait_gone("confirm");
    assert_eq!(k.state()["confirmOpen"], false);
    assert!(k.element(row).is_some());
    assert!(deletes(&k).is_empty());

    k.click("table.actions.kore-demo/shell-delete-probe");
    k.click("table.menu.delete");
    k.wait_for_element("confirm");
    k.click("confirm.ok");
    k.wait_until("toast", |k| k.elements().iter().any(|e| e.id.starts_with("toast.") && e.label == "shell-delete-probe deleted"));
    k.wait_gone("confirm");
    assert_eq!(deletes(&k), ["/api/v1/namespaces/kore-demo/pods/shell-delete-probe"]);
    assert!(k.cluster.get("Pod", Some("kore-demo"), "shell-delete-probe").is_none());
    k.wait_gone(row);
    k.wait_for_element("table.empty");

    // The toast auto-dismisses after ~3 s.
    k.advance(3500);
    k.wait_until("toast gone", |k| !k.elements().iter().any(|e| e.id.starts_with("toast.")));
}

/// Scenario 10-shell-bug-delete-shortcut: "d" deletes the highlighted row
/// through the same confirm; "d" typed into the search stays text.
#[gpui_kit::test]
fn d_key_deletes_the_highlighted_row(cx: &mut TestAppContext) {
    let mut k = pods_kore_demo(cx);
    k.cluster.apply(probe_pod("shell-dkey-probe"));
    k.wait_for_element("table.row.kore-demo/shell-dkey-probe");

    // "d" with the search focused is typed (real keystrokes), not a shortcut.
    k.keys("/");
    k.keys("s h e l l - d k e y - p r o b e");
    k.wait_until("only the probe", |k| k.row_names() == ["shell-dkey-probe"]);
    assert_eq!(k.state()["search"], "shell-dkey-probe");
    assert_eq!(k.state()["confirmOpen"], false);
    k.blur();

    k.keys("j");
    k.keys("d");
    k.wait_label("confirm", "Delete Pod", |l| l == "Delete Pod");
    assert!(k.element("confirm.message").unwrap().label.starts_with("Are you sure you want to delete \"shell-dkey-"));

    // Esc cancels; the pod is still there and we stay on the table.
    k.keys("escape");
    k.wait_gone("confirm");
    assert_eq!(k.state()["view"], "table");
    assert!(k.element("table.row.kore-demo/shell-dkey-probe").is_some());
    assert!(deletes(&k).is_empty());

    k.keys("d");
    k.wait_for_element("confirm");
    k.click("confirm.ok");
    k.wait_until("toast", |k| k.elements().iter().any(|e| e.id.starts_with("toast.") && e.label == "shell-dkey-probe deleted"));
    k.wait_gone("confirm");
    assert!(k.cluster.get("Pod", Some("kore-demo"), "shell-dkey-probe").is_none());
    assert_eq!(deletes(&k), ["/api/v1/namespaces/kore-demo/pods/shell-dkey-probe"]);
}

/// Scenario 10-shell-bug-confirm-dialog: titles use the proper singular kind;
/// Esc and a backdrop click cancel, a click inside the panel does not; Cancel
/// closes. Nothing is deleted.
#[gpui_kit::test]
fn confirm_dialog_titles_and_dismissal(cx: &mut TestAppContext) {
    let mut k = pods_kore_demo(cx);

    // Ingresses: row menu Delete; a click inside the panel keeps it; Esc cancels.
    k.click_sidebar("sidebar.kind.ingresses");
    k.click("table.actions.kore-demo/web");
    k.wait_for_element("table.menu.describe");
    k.click("table.menu.delete");
    k.wait_label("confirm", "Delete Ingress", |l| l == "Delete Ingress");
    k.click("confirm.message");
    k.advance(300);
    assert_eq!(k.state()["confirmOpen"], true);
    k.keys("escape");
    k.wait_gone("confirm");
    assert_eq!(k.state()["confirmOpen"], false);
    assert_eq!(k.state()["view"], "table");
    assert!(k.element("table.row.kore-demo/web").is_some());

    // ConfigMaps via search + j + d; a backdrop click cancels.
    k.click_sidebar("sidebar.kind.configmaps");
    k.click("header.search");
    k.type_text("web-config");
    k.blur();
    k.wait_until("only web-config", |k| k.row_names() == ["web-config"]);
    k.keys("j d");
    k.wait_label("confirm", "Delete ConfigMap", |l| l == "Delete ConfigMap");
    k.cx.simulate_click(point(px(0.6 * 1400.), px(0.9 * 900.)), Modifiers::default());
    k.frame();
    k.wait_gone("confirm");
    assert_eq!(k.state()["confirmOpen"], false);
    assert!(k.element("table.row.kore-demo/web-config").is_some());

    // CronJobs: Cancel closes.
    k.clear_search();
    k.click_sidebar("sidebar.kind.cronjobs");
    k.wait_for_element("table.row.kore-demo/ticker");
    k.keys("j d");
    k.wait_label("confirm", "Delete CronJob", |l| l == "Delete CronJob");
    k.click("confirm.cancel");
    k.wait_gone("confirm");

    assert!(deletes(&k).is_empty(), "deleted: {:?}", deletes(&k));
    assert!(k.cluster.get("Ingress", Some("kore-demo"), "web").is_some());
    assert!(k.cluster.get("ConfigMap", Some("kore-demo"), "web-config").is_some());
    assert!(k.cluster.get("CronJob", Some("kore-demo"), "ticker").is_some());
}

/// Scenario 10-shell-pin: the row star pins (sidebar PINNED section with name +
/// namespace) without opening the row; pins survive a restart; the pinned item
/// opens its details; the star again unpins and removes the section.
#[gpui_kit::test]
fn pin_from_table_survives_restart_and_opens(cx: &mut TestAppContext) {
    let mut k = pods_kore_demo(cx);
    assert!(k.element("sidebar.section.pinned").is_none());
    assert_eq!(k.element("table.pin.kore-demo/multi-container").unwrap().label, "Pin");

    k.click("table.pin.kore-demo/multi-container");
    k.wait_for_element("sidebar.section.pinned");
    assert_eq!(k.wait_for_element("sidebar.pin.kore-demo/multi-container").label, "multi-container");
    assert_eq!(k.state()["pins"], serde_json::json!([{"kind": "pods", "name": "multi-container", "namespace": "kore-demo"}]));
    let star = k.element("table.pin.kore-demo/multi-container").unwrap();
    assert!(star.selected && star.label == "Unpin");
    // Pinning does not open the row.
    assert_eq!(k.state()["view"], "table");
    assert_eq!(k.state()["selected"], serde_json::Value::Null);

    k.restart();
    k.wait_for_element("sidebar.section.pinned");
    k.click("sidebar.pin.kore-demo/multi-container");
    k.wait_until("pinned details", |k| k.state()["view"] == "details" && k.state()["selected"]["name"] == "multi-container");
    assert_eq!(k.state()["kind"], "pods");
    k.keys("h");
    k.wait_until("back", |k| k.state()["view"] == "table");

    k.show(ResourceKind::Pods, "kore-demo");
    k.settle();
    k.click("table.pin.kore-demo/multi-container");
    k.wait_gone("sidebar.section.pinned");
    assert!(k.element("sidebar.pin.kore-demo/multi-container").is_none());
    assert_eq!(k.state()["pins"], serde_json::json!([]));
}

/// Scenario 10-shell-bug-pinned-heading (structure part): the PINNED section has
/// a single heading directly above the pinned row, and pinned rows line up with
/// the other nav items (no extra indent).
#[gpui_kit::test]
fn pinned_section_has_one_heading_and_aligned_rows(cx: &mut TestAppContext) {
    let mut k = pods_kore_demo(cx);
    k.click("table.pin.kore-demo/multi-container");
    let pin = k.wait_for_element("sidebar.pin.kore-demo/multi-container");
    let headings: Vec<_> = k.elements().into_iter().filter(|e| e.id == "sidebar.section.pinned").collect();
    assert_eq!(headings.len(), 1);
    let heading = &headings[0];
    assert_eq!(heading.label, "PINNED");

    // Nothing (e.g. a second heading) between the heading and the pinned row:
    // the row starts right under the heading's 6px bottom margin.
    let gap = pin.bounds.y - (heading.bounds.y + heading.bounds.h);
    assert!((0. ..=8.).contains(&gap), "gap between heading and pinned row: {gap}");
    let workloads = k.wait_for_element("sidebar.section.workloads");
    assert!(workloads.bounds.y > pin.bounds.y + pin.bounds.h, "WORKLOADS below the pinned row");

    // Aligned with the other nav items.
    let pods = k.element("sidebar.kind.pods").unwrap();
    assert_eq!(pin.bounds.x, pods.bounds.x, "pinned row x vs nav item x");
    assert_eq!(heading.bounds.x, workloads.bounds.x, "heading x vs section heading x");

    k.click("table.pin.kore-demo/multi-container");
    k.wait_gone("sidebar.section.pinned");
}

/// Scenario 10-shell-multi-cluster (behavior part): the sidebar toggle prepends
/// a CLUSTER column and lists rows from every context (remote-web from
/// kind-kore-harness-2); toggling off removes them.
#[gpui_kit::test]
fn multi_cluster_toggle_lists_every_context(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.show(ResourceKind::Deployments, "*");
    k.settle();
    k.wait_for_element("table.row.kube-system/coredns");
    assert!(k.element("table.header._context").is_none());
    assert!(k.element("table.row.kore-remote/remote-web").is_none());

    k.click("sidebar.multi-cluster");
    assert_eq!(k.state()["multiCluster"], true);
    k.settle();
    k.wait_for_element("table.row.kore-remote/remote-web");
    let headers = k.headers();
    assert_eq!(headers[..4], ["CLUSTER", "NAMESPACE", "NAME", "READY"]);
    assert_eq!(k.cell("kore-remote/remote-web", "_context"), "kind-kore-harness-2");
    assert_eq!(k.cell("kube-system/coredns", "_context"), "kind-kore-harness");
    assert_eq!(k.cell("kore-remote/remote-web", "namespace"), "kore-remote");

    k.click("sidebar.multi-cluster");
    assert_eq!(k.state()["multiCluster"], false);
    k.settle();
    k.wait_gone("table.row.kore-remote/remote-web");
    k.wait_for_element("table.row.kube-system/coredns");
    assert!(k.element("table.header._context").is_none());
    assert_eq!(k.headers()[0], "NAMESPACE");
}

/// Scenario 10-shell-fix-restart-sparkline (behavior part, Fix 14): restart
/// history is recorded, so a restarted pod's RESTARTS cell renders through the
/// sparkline, and an increase seen by the watch adds a point. (The amber/red
/// color stays in Layer 2.)
#[gpui_kit::test]
fn restarted_pods_render_restart_sparkline(cx: &mut TestAppContext) {
    let mut k = pods_kore_demo(cx);
    let crash = "kore-demo/crashloop";
    let cell = k.wait_for_element(&format!("table.cell.{crash}.restarts"));
    assert_eq!(cell.role, "sparkline");
    assert_eq!(cell.label, "498");
    assert_eq!(k.app_state(|s, cx| s.resources.read(cx).restart_history("kore-demo", "crashloop")), vec![498]);
    // A pod that never restarted keeps the plain number.
    let mc = k.wait_for_element("table.cell.kore-demo/multi-container.restarts");
    assert_eq!((mc.role.as_str(), mc.label.as_str()), ("cell", "0"));

    // A restart seen through the watch records a rising point.
    k.cluster.modify("Pod", Some("kore-demo"), "crashloop", |p| {
        p["status"]["containerStatuses"][0]["restartCount"] = serde_json::json!(499);
    });
    k.wait_label(&format!("table.cell.{crash}.restarts"), "499", |l| l == "499");
    assert_eq!(k.app_state(|s, cx| s.resources.read(cx).restart_history("kore-demo", "crashloop")), vec![498, 499]);
    assert_eq!(k.element(&format!("table.cell.{crash}.restarts")).unwrap().role, "sparkline");
}
