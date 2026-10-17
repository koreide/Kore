//! Sidebar: context / namespace dropdowns and the VIEWS nav (spec 01 §2, §4, §5).

use super::helpers::ShellExt;
use crate::testing::Kore;
use gpui_kit::{point, px, Modifiers, TestAppContext};
use kore_testkit::fixtures::{self, CONTEXT, CONTEXT_2};

/// Click an element whose id must match exactly (ids ending in `*` would be
/// treated as a prefix by `Kore::element`).
fn click_exact(k: &mut Kore, id: &str) {
    let want = id.to_string();
    k.wait_until(&format!("element {id}"), |k| k.elements().iter().any(|e| e.id == want));
    let e = k.elements().into_iter().find(|e| e.id == want).unwrap();
    let (x, y) = e.bounds.center();
    k.cx.simulate_click(point(px(x), px(y)), Modifiers::default());
    k.frame();
}

/// Mouse down somewhere neutral: the main area's top padding, right of the sidebar.
fn click_outside(k: &mut Kore) {
    k.cx.simulate_click(point(px(crate::views::sidebar::WIDTH + 30.), px(6.)), Modifiers::default());
    k.frame();
}

fn ns_items(k: &mut Kore) -> Vec<String> {
    k.labels_in_order("sidebar.namespace.item.")
}

/// Scenario 10-shell-context-switch: switching context from the sidebar dropdown
/// resets to the table, clears the search, updates the header pill and loads the
/// other cluster's namespaces; switching back restores kind-kore-harness.
#[gpui_kit::test]
fn sidebar_context_switch_resets_table_and_search(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.pods_kore_demo();
    k.click("header.search");
    k.type_text("crash");
    k.wait_until("search crash", |k| k.state()["search"] == "crash" && k.state()["visibleCount"] == 1);

    k.click("sidebar.context.trigger");
    k.wait_for_element("sidebar.context.panel");
    assert_eq!(k.labels_in_order("sidebar.context.item."), [CONTEXT, CONTEXT_2]);
    k.click(&format!("sidebar.context.item.{CONTEXT_2}"));
    k.wait_gone("sidebar.context.panel");
    k.wait_until("context 2", |k| k.state()["context"] == CONTEXT_2);
    k.wait_label("header.context", "kind-kore-harness-2", |l| l == CONTEXT_2);
    k.wait_label("sidebar.context.trigger", "kind-kore-harness-2", |l| l == CONTEXT_2);
    let s = k.state();
    assert_eq!(s["search"], "");
    assert_eq!(s["view"], "table");
    assert_eq!(s["kind"], "pods");
    k.wait_for_element("header.search");
    // kore-demo doesn't exist on cluster 2: falls back to default; cluster 2's namespaces load.
    k.wait_until("namespace default", |k| k.state()["namespace"] == "default");
    k.wait_until("cluster-2 namespaces", |k| {
        k.state()["namespaces"].as_array().is_some_and(|a| a.iter().any(|n| n == "kore-remote") && !a.iter().any(|n| n == "kore-demo"))
    });
    k.wait_label("sidebar.namespace.trigger", "default", |l| l == "default");

    // Back to kind-kore-harness.
    k.click("sidebar.context.trigger");
    k.click(&format!("sidebar.context.item.{CONTEXT}"));
    k.wait_until("context 1", |k| k.state()["context"] == CONTEXT);
    k.wait_label("header.context", "kind-kore-harness", |l| l == CONTEXT);
    k.select_namespace("kore-demo");
    k.wait_el_ff("table.row.kore-demo/multi-container");
}

/// Scenario 10-shell-ns-dropdown: the namespace dropdown lists "All Namespaces" then
/// namespaces A-Z, typing filters (substring), Tab accepts the ghost completion,
/// clicking outside closes without selecting, and "All Namespaces" lists pods across namespaces.
#[gpui_kit::test]
fn namespace_dropdown_filters_completes_and_selects_all(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.pods_kore_demo();
    k.open_ns_dropdown();
    let items = ns_items(&mut k);
    assert_eq!(items[0], "All Namespaces", "{items:?}");
    let mut sorted = items[1..].to_vec();
    sorted.sort();
    assert_eq!(items[1..], sorted[..], "namespaces are alphabetical");
    for ns in ["default", "kore-demo", "kore-rbac", "kube-node-lease", "kube-public", "kube-system"] {
        assert!(items.iter().any(|i| i == ns), "{ns} missing from {items:?}");
    }

    k.type_text("kube");
    k.wait_until("filtered to kube*", |k| ns_items(k) == ["kube-node-lease", "kube-public", "kube-system"]);

    // Tab accepts the ghost completion of the first match.
    k.keys("tab");
    k.wait_until("completed", |k| ns_items(k) == ["kube-node-lease"]);

    // Click outside closes the panel without selecting.
    click_outside(&mut k);
    k.wait_gone("sidebar.namespace.panel");
    assert_eq!(k.state()["namespace"], "kore-demo");
    k.wait_label("sidebar.namespace.trigger", "kore-demo", |l| l == "kore-demo");

    // All Namespaces lists pods from several namespaces.
    k.open_ns_dropdown();
    click_exact(&mut k, "sidebar.namespace.item.*");
    k.wait_until("all namespaces", |k| k.state()["namespace"] == "*");
    k.wait_el_ff("table.row.kore-demo/crashloop");
    k.wait_el_ff("table.row.kube-system/etcd-kore-harness-control-plane");
    k.wait_el_ff("table.row.kube-system/coredns-*");
}

/// Scenario 10-shell-ns-favorite: starring a namespace moves it to the top (above
/// "All Namespaces"); with exactly one favorite the next launch auto-selects it;
/// unstarring restores the default order and launch namespace.
#[gpui_kit::test]
fn favorite_namespace_sorts_first_and_is_selected_on_launch(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.pods_kore_demo();
    k.open_ns_dropdown();
    k.type_text("kube-sys");
    k.wait_until("filtered", |k| ns_items(k) == ["kube-system"]);
    k.wait_label("sidebar.namespace.star.kube-system", "Add", |l| l == "Add to favorites");
    k.click("sidebar.namespace.star.kube-system");
    k.wait_label("sidebar.namespace.star.kube-system", "Remove", |l| l == "Remove from favorites");
    // The panel stays open; clear its search box.
    let panel = k.wait_for_element("sidebar.namespace.panel");
    k.cx.simulate_click(point(px(panel.bounds.x + 60.), px(panel.bounds.y + 25.)), Modifiers::default());
    k.frame();
    k.keys("cmd-a backspace");
    k.wait_until("favorite first", |k| {
        let items = ns_items(k);
        items.len() > 2 && items[0] == "kube-system" && items[1] == "All Namespaces"
    });
    click_outside(&mut k);
    k.wait_gone("sidebar.namespace.panel");

    // Persisted (Core favorites.json in the sandbox), then auto-selected on launch.
    let fav = k.env.data_dir().join("favorites.json");
    k.wait_until("favorite saved", |_| std::fs::read_to_string(&fav).is_ok_and(|s| s.contains("kube-system")));
    k.restart();
    k.wait_until("kube-system auto-selected", |k| k.state()["namespace"] == "kube-system");
    k.wait_label("sidebar.namespace.trigger", "kube-system", |l| l == "kube-system");

    // Unstar: "All Namespaces" is back on top, and the next launch keeps the default.
    k.open_ns_dropdown();
    k.wait_label("sidebar.namespace.star.kube-system", "Remove", |l| l == "Remove from favorites");
    k.click("sidebar.namespace.star.kube-system");
    k.wait_until("All first", |k| ns_items(k).first().is_some_and(|i| i == "All Namespaces"));
    click_outside(&mut k);
    k.wait_gone("sidebar.namespace.panel");
    k.wait_until("favorite removed", |_| std::fs::read_to_string(&fav).is_ok_and(|s| !s.contains("kube-system")));
    k.restart();
    k.wait_until("connected", |k| k.state()["context"] == CONTEXT && k.element("sidebar.namespace.trigger").is_some());
    for _ in 0..20 {
        k.frame();
    }
    assert_eq!(k.state()["namespace"], "default");
}

/// Scenario 10-shell-sidebar-views: each VIEWS item opens its view; the table header
/// with the search box is hidden for these views and comes back for resource kinds.
#[gpui_kit::test]
fn sidebar_views_open_their_view_and_hide_table_header(cx: &mut TestAppContext) {
    let (env, c1, c2) = fixtures::harness_env();
    let env = fixtures::with_helm(env);
    let mut k = Kore::boot_env(cx, env, c1, c2);
    k.pods_kore_demo();
    k.wait_for_element("header.search");

    let views: [(&str, &str, &[&str]); 7] = [
        ("dashboard", "view.dashboard", &["dashboard.card.*"]),
        ("graph", "view.graph", &["graph.filter"]),
        ("network-policies", "view.network-policies", &["np.policy.allow-logger-to-web", "np.policy.default-deny-ingress"]),
        ("rbac", "view.rbac", &["tab.permission-matrix", "tab.role-browser"]),
        ("crds", "view.crds", &["crd.group.kore.dev"]),
        ("helm", "view.helm", &["helm.row.kore-demo/kore-demo"]),
        ("chat", "view.chat", &["chat.input"]),
    ];
    for (view, region, content) in views {
        k.click(&format!("sidebar.view.{view}"));
        k.wait_until(&format!("view {view}"), |k| k.state()["view"] == view);
        k.wait_for_element(region);
        for id in content {
            k.wait_for_element(id);
        }
        assert!(k.element(&format!("sidebar.view.{view}")).unwrap().selected);
        assert!(k.element("header.search").is_none(), "{view} shows no table header");
    }

    // Back to a resource kind: the header with search returns.
    k.click("sidebar.kind.pods");
    k.wait_until("table", |k| k.state()["view"] == "table");
    k.wait_for_element("header.search");
    k.wait_el_ff("table.header.restarts");
}

/// Scenario 10-shell-fix-dropdown-keyboard (Fix 3): while typing in the namespace
/// dropdown's search box, Down/Enter select the highlighted match and Esc closes the
/// panel without changing the selection.
#[gpui_kit::test]
fn namespace_dropdown_keyboard_works_from_search_input(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.click("sidebar.kind.pods");
    k.wait_until("pods default", |k| k.state()["view"] == "table" && k.state()["namespace"] == "default");

    k.open_ns_dropdown();
    k.type_text("kore-d");
    k.wait_until("kore-demo only", |k| ns_items(k) == ["kore-demo"]);
    k.keys("down enter");
    k.wait_gone("sidebar.namespace.panel");
    k.wait_until("kore-demo", |k| k.state()["namespace"] == "kore-demo");
    k.wait_label("sidebar.namespace.trigger", "kore-demo", |l| l == "kore-demo");
    k.wait_el_ff("table.row.kore-demo/multi-container");

    // Down moves the highlight: "kore" matches kore-demo, kore-rbac; Down + Enter picks the second.
    k.open_ns_dropdown();
    k.type_text("kore");
    k.wait_until("two matches", |k| ns_items(k) == ["kore-demo", "kore-rbac"]);
    k.keys("down enter");
    k.wait_until("kore-rbac", |k| k.state()["namespace"] == "kore-rbac");
    k.wait_gone("sidebar.namespace.panel");

    // Esc closes an open panel without changing the selection (or leaving the table).
    k.open_ns_dropdown();
    k.type_text("kube");
    k.keys("escape");
    k.wait_gone("sidebar.namespace.panel");
    assert_eq!(k.state()["namespace"], "kore-rbac");
    assert_eq!(k.state()["view"], "table");
    k.wait_label("sidebar.namespace.trigger", "kore-rbac", |l| l == "kore-rbac");
}

/// Scenario 10-shell-bug-ns-all-label: with all namespaces selected the trigger reads
/// "All Namespaces" (not "*").
#[gpui_kit::test]
fn all_namespaces_trigger_reads_all_namespaces(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.pods_kore_demo();
    k.open_ns_dropdown();
    click_exact(&mut k, "sidebar.namespace.item.*");
    k.wait_until("namespace *", |k| k.state()["namespace"] == "*");
    k.wait_label("sidebar.namespace.trigger", "All Namespaces", |l| l == "All Namespaces");
    k.select_namespace("kore-demo");
    k.wait_label("sidebar.namespace.trigger", "kore-demo", |l| l == "kore-demo");
}
