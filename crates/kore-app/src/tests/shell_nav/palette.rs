//! Command palette (spec 01 §14, §21.28-34).

use super::helpers::ShellExt;
use crate::testing::Kore;
use gpui_kit::TestAppContext;
use kore_testkit::fixtures::{CONTEXT, CONTEXT_2};

/// Replace the palette query (select all + type), like the scenarios' cmd+a / type.
fn requery(k: &mut Kore, q: &str) {
    k.keys("cmd-a backspace");
    k.type_text(q);
}

fn selected(k: &mut Kore) -> String {
    k.element("palette.selected").map(|e| e.label).unwrap_or_default()
}

/// Scenario 10-shell-palette-basic: Cmd+K opens the palette (Views / Resources
/// groups), Cmd+K toggles it closed, Esc closes it, typing filters to the Actions /
/// Contexts groups, and a nonsense query shows "No results found".
#[gpui_kit::test]
fn palette_opens_toggles_filters_and_shows_empty_state(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.pods_kore_demo();
    k.open_palette();
    assert_eq!(k.element("palette.input").unwrap().label, "Search resources, navigate, switch context...");
    assert_eq!(k.state()["paletteOpen"], true);
    k.wait_for_element("palette.group.Views");
    k.wait_for_element("palette.group.Resources");
    for v in ["AI Chat", "Resource Graph", "Network Policies", "Custom Resources", "Helm Releases", "RBAC Simulator", "Settings"] {
        k.wait_for_element(&format!("palette.item.view-{v}"));
    }
    k.wait_for_element("palette.item.nav-Pods");
    assert!(k.element("palette.empty").is_none());

    // Cmd+K again toggles it closed; Esc closes it too.
    k.keys("cmd-k");
    k.wait_gone("palette");
    assert_eq!(k.state()["paletteOpen"], false);
    k.open_palette();
    k.keys("escape");
    k.wait_gone("palette");
    assert_eq!(k.view(), "table");
    k.wait_el_ff("table.header.restarts");

    k.open_palette();
    k.type_text("sponsor");
    k.wait_for_element("palette.item.action-sponsor");
    k.wait_for_element("palette.group.Actions");
    assert!(k.element("palette.group.Views").is_none());

    requery(&mut k, "kind-kore");
    k.wait_for_element("palette.group.Contexts");
    k.wait_for_element(&format!("palette.item.ctx-{CONTEXT}"));
    k.wait_for_element(&format!("palette.item.ctx-{CONTEXT_2}"));

    requery(&mut k, "zzzzqqq");
    k.wait_label("palette.empty", "No results found", |l| l == "No results found");
    assert!(k.element("palette.item.*").is_none());
    k.keys("escape");
    k.wait_gone("palette");
}

/// Scenario 10-shell-palette-context-ai: palette "Contexts" items switch the context
/// (header pill, table view) and "Ask AI" opens the AI side panel.
#[gpui_kit::test]
fn palette_switches_context_and_opens_ai_panel(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.pods_kore_demo();
    k.open_palette();
    k.type_text(CONTEXT_2);
    k.wait_for_element("palette.group.Contexts");
    k.click(&format!("palette.item.ctx-{CONTEXT_2}"));
    k.wait_gone("palette");
    k.wait_until("context 2", |k| k.state()["context"] == CONTEXT_2);
    k.wait_label("header.context", "kind-kore-harness-2", |l| l == CONTEXT_2);
    k.wait_label("sidebar.context.trigger", "kind-kore-harness-2", |l| l == CONTEXT_2);
    assert_eq!(k.view(), "table");

    k.open_palette();
    k.type_text("kind-kore");
    k.wait_for_element("palette.group.Contexts");
    k.click(&format!("palette.item.ctx-{CONTEXT}"));
    k.wait_until("context 1", |k| k.state()["context"] == CONTEXT);
    k.wait_label("header.context", "kind-kore-harness", |l| l == CONTEXT);
    k.select_namespace("kore-demo");

    k.open_palette();
    k.type_text("Ask AI");
    k.wait_until("Ask AI selected", |k| selected(k) == "action-ask-ai");
    k.keys("enter");
    k.wait_gone("palette");
    k.wait_for_element("ai-panel");
    assert!(k.state()["aiPanel"].is_object());
    k.keys("escape");
    k.wait_gone("ai-panel");
    assert!(k.state()["aiPanel"].is_null());
}

/// Scenario 10-shell-palette-nav: "Dashboard" + Enter opens the Dashboard, "Go to
/// Secrets" opens the Secrets table, ":" lists the Quick Filters, ":deployments"
/// opens Deployments, and ":failed" filters the table with a "Status: Failed" chip
/// that a sidebar kind click clears.
#[gpui_kit::test]
fn palette_navigates_views_kinds_and_quick_filters(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.pods_kore_demo();
    k.open_palette();
    k.type_text("Dashboard");
    k.keys("enter");
    k.wait_until("dashboard", |k| k.state()["view"] == "dashboard");
    k.wait_gone("palette");
    k.wait_for_element("view.dashboard");

    k.open_palette();
    k.type_text("Secrets");
    k.click("palette.item.nav-Secrets");
    k.wait_until("secrets table", |k| k.state()["view"] == "table" && k.state()["kind"] == "secrets");
    k.wait_label("header.breadcrumb", "Secrets", |l| l == "Secrets");
    k.wait_el_ff("table.row.kore-demo/web-secret");

    k.open_palette();
    k.type_text(":");
    k.wait_for_element("palette.group.Quick Filters");
    let filters: Vec<String> = k.ids_in_order("palette.item.filter-");
    assert_eq!(
        filters,
        [":pods", ":deployments", ":services", ":running", ":failed", ":pending"].map(|f| format!("palette.item.filter-{f}"))
    );
    k.type_text("deployments");
    k.wait_until("only :deployments", |k| selected(k) == "filter-:deployments");
    k.keys("enter");
    k.wait_until("deployments", |k| k.state()["kind"] == "deployments" && k.state()["view"] == "table");
    k.wait_label("header.breadcrumb", "Deployments", |l| l == "Deployments");
    k.wait_el_ff("table.header.upToDate");

    // Status quick filter on Deployments: closes the palette, filters (nothing matches).
    k.open_palette();
    k.type_text(":failed");
    k.wait_until(":failed selected", |k| selected(k) == "filter-:failed");
    k.keys("enter");
    k.wait_gone("palette");
    k.wait_label("header.status-filter", "Status: Failed", |l| l == "Status: Failed");
    assert_eq!(k.state()["statusFilter"], "Failed");
    // The sidebar kind click clears it.
    k.click("sidebar.kind.pods");
    k.wait_until("cleared", |k| k.state()["statusFilter"].is_null() && k.state()["kind"] == "pods");
    k.wait_gone("header.status-filter");
}

/// Scenario 10-shell-palette-search-recent: a name fragment shows backend "Search
/// Results"; selecting one opens its details and records it; after a restart the
/// palette lists it under "Recent".
#[gpui_kit::test]
fn palette_search_opens_details_and_records_recent(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.pods_kore_demo();
    k.open_palette();
    k.type_text("multi-co");
    k.wait_el_ff("palette.group.Search Results");
    k.click("palette.item.search-pods-multi-container");
    k.wait_gone("palette");
    k.wait_until("details", |k| k.state()["view"] == "details" && k.state()["selected"]["name"] == "multi-container");
    assert_eq!(k.state()["kind"], "pods");
    k.keys("h");
    k.wait_until("table", |k| k.state()["view"] == "table");

    k.restart();
    k.open_palette();
    k.type_text("recent");
    k.wait_for_element("palette.group.Recent");
    k.wait_for_element("palette.item.recent-pods-multi-container");
    k.keys("escape");
    k.wait_gone("palette");
}

/// Scenario 10-shell-bug-palette: the keyboard selection starts on the first item and
/// follows Up/Down; "Network Policies" is a view entry; ":failed" / ":pending" filter
/// the pods table with a "Status: ..." chip whose x clears it; Recent updates live.
/// (Heading color/size is Layer 2.)
#[gpui_kit::test]
fn palette_selection_status_filters_and_live_recent(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.pods_kore_demo();
    k.open_palette();
    k.wait_for_element("palette.item.view-Network Policies");
    k.wait_for_element("palette.item.view-Resource Graph");
    k.wait_label("palette.selected", "first item", |l| l == "view-AI Chat");
    assert!(k.element("palette.item.view-AI Chat").unwrap().selected);
    k.keys("down down");
    k.wait_label("palette.selected", "Resource Graph", |l| l == "view-Resource Graph");
    assert!(k.element("palette.item.view-Resource Graph").unwrap().selected);
    assert!(!k.element("palette.item.view-AI Chat").unwrap().selected);
    k.keys("up");
    k.wait_label("palette.selected", "Dashboard", |l| l == "view-Dashboard");

    k.type_text("Network Policies");
    k.wait_label("palette.selected", "Network Policies", |l| l == "view-Network Policies");
    k.keys("enter");
    k.wait_gone("palette");
    k.wait_until("network policies", |k| k.state()["view"] == "network-policies");

    k.click("sidebar.kind.pods");
    k.wait_el_ff("table.row.kore-demo/multi-container");

    // ":failed" keeps crashloop, hides running / pending / completed pods.
    k.open_palette();
    k.type_text(":failed");
    k.wait_until(":failed selected", |k| selected(k) == "filter-:failed");
    k.keys("enter");
    k.wait_gone("palette");
    k.wait_label("header.status-filter", "Status: Failed", |l| l == "Status: Failed");
    assert_eq!(k.state()["statusFilter"], "Failed");
    k.wait_ff("multi-container hidden", |k| k.element("table.row.kore-demo/multi-container").is_none());
    k.wait_el_ff("table.row.kore-demo/crashloop");
    assert!(k.element("table.row.kore-demo/unschedulable").is_none());
    assert!(k.element("table.row.kore-demo/one-shot-*").is_none());

    // ":pending" swaps the filter.
    k.open_palette();
    k.type_text(":pending");
    k.wait_until(":pending selected", |k| selected(k) == "filter-:pending");
    k.keys("enter");
    k.wait_label("header.status-filter", "Status: Pending", |l| l == "Status: Pending");
    k.wait_el_ff("table.row.kore-demo/unschedulable");
    assert!(k.element("table.row.kore-demo/crashloop").is_none());
    assert!(k.element("table.row.kore-demo/multi-container").is_none());

    // The chip's x clears it.
    k.click("header.status-filter.clear");
    k.wait_until("cleared", |k| k.state()["statusFilter"].is_null());
    k.wait_el_ff("table.row.kore-demo/multi-container");

    // Recent is live: a search-result selection shows on the next open, no restart.
    k.open_palette();
    k.type_text("unschedul");
    k.wait_el_ff("palette.group.Search Results");
    k.click("palette.item.search-pods-unschedulable");
    k.wait_until("details", |k| k.state()["view"] == "details" && k.state()["selected"]["name"] == "unschedulable");
    k.keys("h");
    k.wait_until("table", |k| k.state()["view"] == "table");
    k.open_palette();
    k.type_text("recent-pods-unsch");
    k.wait_for_element("palette.group.Recent");
    k.wait_for_element("palette.item.recent-pods-unschedulable");
    k.keys("escape");
    k.wait_gone("palette");
}
