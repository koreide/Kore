//! Table keyboard navigation, sorting, search, refresh and the label filter
//! (spec 01 §3, §6, §7.3, §7.4, §8).

use super::helpers::*;
use gpui_kit::TestAppContext;

/// The highlighted (selected) row's key, if any.
fn highlighted(k: &mut crate::testing::Kore) -> Option<String> {
    k.elements().into_iter().find(|e| e.id.starts_with("table.row.") && e.selected).map(|e| e.id["table.row.".len()..].to_string())
}

fn selected_name(k: &mut crate::testing::Kore) -> String {
    k.state()["selected"]["name"].as_str().unwrap_or_default().to_string()
}

/// Scenario 10-shell-table-keyboard: j/j/j/k highlights rows, l opens the
/// highlighted pod, h goes back; Down + Enter opens the next row; clicking a
/// row opens it directly.
#[gpui_kit::test]
fn keyboard_and_mouse_row_navigation(cx: &mut TestAppContext) {
    let mut k = pods_kore_demo(cx);
    let rows = k.row_keys();
    assert_eq!(k.state()["selectedRowIndex"], -1);

    k.keys("j j j k");
    assert_eq!(k.state()["selectedRowIndex"], 1);
    assert_eq!(highlighted(&mut k).as_deref(), Some(rows[1].as_str()));
    assert!(rows[1].starts_with("kore-demo/kore-demo-"), "row 1 {}", rows[1]);

    k.keys("l");
    k.wait_until("details of row 1", |k| k.state()["view"] == "details");
    assert_eq!(format!("kore-demo/{}", selected_name(&mut k)), rows[1]);
    assert_eq!(k.element("header.breadcrumb").unwrap().label, "Pods");

    k.keys("h");
    k.wait_until("back to table", |k| k.state()["view"] == "table");
    k.wait_for_element("table.header.restarts");

    k.keys("down");
    assert_eq!(k.state()["selectedRowIndex"], 2);
    k.keys("enter");
    k.wait_until("details of row 2", |k| k.state()["view"] == "details");
    assert!(selected_name(&mut k).starts_with("logger-"), "opened {}", selected_name(&mut k));
    k.keys("h");
    k.wait_until("back to table", |k| k.state()["view"] == "table");

    k.click("table.row.kore-demo/multi-container");
    k.wait_until("multi-container details", |k| k.state()["view"] == "details");
    assert_eq!(selected_name(&mut k), "multi-container");
    k.keys("h");
    k.wait_until("back to table", |k| k.state()["view"] == "table");
    k.wait_for_element("table.header.restarts");
}

/// Seconds of a relative age label ("9h", "1d", "30s").
fn age_secs(a: &str) -> u64 {
    let (n, unit) = a.split_at(a.len() - 1);
    let n: u64 = n.parse().unwrap_or_else(|_| panic!("age {a}"));
    n * match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        "y" => 365 * 86400,
        _ => panic!("age {a}"),
    }
}

/// Scenario 10-shell-table-sort: NAME header sorts ascending then descending;
/// STATUS sorts as a string (Completed first, then Running first); AGE sorts too.
#[gpui_kit::test]
fn header_clicks_sort_name_status_and_age(cx: &mut TestAppContext) {
    let mut k = pods_kore_demo(cx);
    assert_eq!(k.state()["sorting"], serde_json::json!([]));

    k.click("table.header.name");
    k.wait_until("name asc", |k| k.state()["sorting"] == serde_json::json!([{"id": "name", "desc": false}]));
    assert!(k.element("table.header.name").unwrap().selected);
    let names = k.row_names();
    assert_eq!(names[0], "crashloop");
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);

    k.click("table.header.name");
    k.wait_until("name desc", |k| k.state()["sorting"] == serde_json::json!([{"id": "name", "desc": true}]));
    let names = k.row_names();
    assert!(names[0].starts_with("web-"), "first {names:?}");
    assert_ne!(names[0], "crashloop");
    assert_eq!(names.last().map(String::as_str), Some("crashloop"));

    k.click("table.header.status");
    k.wait_until("status sorted", |k| k.state()["sorting"][0]["id"] == "status");
    assert_eq!(k.column("status")[0], "Completed");
    k.click("table.header.status");
    k.wait_until("status desc", |k| k.column("status").first().map(String::as_str) == Some("Running"));
    assert!(!k.element("table.header.name").unwrap().selected);

    k.click("table.header.age");
    k.wait_until("age sorted", |k| k.state()["sorting"][0]["id"] == "age");
    let first: Vec<u64> = k.column("age").iter().map(|a| age_secs(a)).collect();
    assert!(first.windows(2).all(|w| w[0] <= w[1]) || first.windows(2).all(|w| w[0] >= w[1]), "ages {first:?}");
    k.click("table.header.age");
    let second: Vec<u64> = k.column("age").iter().map(|a| age_secs(a)).collect();
    let mut rev = first.clone();
    rev.reverse();
    assert_eq!(second, rev);
}

/// Scenario 10-shell-fix-jkl-sorted (Fix 2): with NAME sorted descending, j
/// highlights the first *visible* row (web-*) and l opens that same row.
#[gpui_kit::test]
fn jkl_follow_the_sorted_order(cx: &mut TestAppContext) {
    let mut k = pods_kore_demo(cx);
    k.click("table.header.name");
    k.click("table.header.name");
    k.wait_until("name desc", |k| k.state()["sorting"] == serde_json::json!([{"id": "name", "desc": true}]));
    let first = k.row_keys()[0].clone();
    assert!(first.starts_with("kore-demo/web-"), "first row {first}");

    k.keys("j");
    assert_eq!(highlighted(&mut k).as_deref(), Some(first.as_str()));
    k.keys("l");
    k.wait_until("details", |k| k.state()["view"] == "details");
    assert_eq!(format!("kore-demo/{}", selected_name(&mut k)), first);
    k.keys("h");
    k.wait_until("back", |k| k.state()["view"] == "table");
}

/// Scenario 10-shell-search: "/" focuses the header search; the filter matches
/// name or status, updates the count pill, shows the empty state for no
/// matches, and clearing restores all rows.
#[gpui_kit::test]
fn search_filters_by_name_and_status_with_empty_state(cx: &mut TestAppContext) {
    let mut k = pods_kore_demo(cx);
    let all = k.row_names().len();

    k.keys("/");
    k.type_text("crash");
    k.wait_until("only crashloop", |k| k.row_names() == ["crashloop"]);
    assert_eq!(k.state()["search"], "crash");
    k.wait_label("header.count", "1", |l| l == "1");

    // Status matches too.
    k.keys("cmd-a");
    k.type_text("Pending");
    k.wait_until("only unschedulable", |k| k.row_names() == ["unschedulable"]);

    // Namespace matches every row.
    k.keys("cmd-a");
    k.type_text("kore-demo");
    k.wait_until("all rows by namespace", |k| k.row_names().len() == all);

    // No match: empty state.
    k.keys("cmd-a");
    k.type_text("zzqqxx");
    k.wait_for_element("table.empty");
    assert_eq!(k.element("table.empty").unwrap().label, "No resources found");
    k.wait_label("header.count", "0", |l| l == "0");
    assert!(k.element("table.row.*").is_none());

    // Clear: every row back.
    k.keys("cmd-a backspace");
    k.wait_until("all rows back", |k| k.row_names().len() == all);
    assert_eq!(k.state()["search"], "");
    k.wait_label("header.count", "all", |l| l == all.to_string());
}

/// Scenario 10-shell-fix-slash-typeable (Fix 1): "/" typed into the label
/// filter input stays in that input; it does not jump to the header search.
#[gpui_kit::test]
fn slash_is_typeable_in_the_label_filter(cx: &mut TestAppContext) {
    let mut k = pods_kore_demo(cx);
    k.click("header.filter");
    k.click("labelbar.input");
    // Real keystrokes (not IME input), so "/" goes through the global key handler.
    k.keys("x . i o / a = w e b");
    assert_eq!(k.state()["search"], "", "the header search must stay empty");
    k.keys("enter");
    k.wait_until("chip", |k| k.state()["labelFilters"] == serde_json::json!(["x.io/a=web"]));
    k.wait_for_element("labelbar.chip.x.io/a=web");
    assert_eq!(k.state()["search"], "");
}

/// Scenario 10-shell-refresh: Cmd+R clears the rows behind the skeleton, lists
/// again and repopulates the table, without leaving the table.
#[gpui_kit::test]
fn cmd_r_refreshes_through_the_skeleton(cx: &mut TestAppContext) {
    let mut k = pods_kore_demo(cx);
    let before = k.row_keys();
    k.cluster.clear_requests();

    k.keys("cmd-r");
    assert_eq!(k.state()["loading"], true);
    k.wait_for_element("table.skeleton");
    assert!(k.element("table.row.*").is_none());
    assert_eq!(k.state()["view"], "table");

    k.settle();
    k.wait_for_element("table.row.kore-demo/multi-container");
    assert_eq!(k.row_keys(), before);
    assert_eq!(k.state()["view"], "table");
    assert_eq!(k.element("header.breadcrumb").unwrap().label, "Pods");
    k.wait_for_element("table.header.restarts");
    let lists = k
        .cluster
        .requests()
        .into_iter()
        .filter(|r| r.method == "GET" && r.path == "/api/v1/namespaces/kore-demo/pods" && !r.query.contains_key("watch"))
        .count();
    assert!(lists >= 1, "no pod list after refresh: {:?}", k.cluster.requests());
}

/// Scenario 10-shell-label-filter: the Filter button opens the label bar; Enter
/// adds a chip and the table refetches with the selector; Backspace in the empty
/// input removes the last chip; switching kind clears chips and hides the bar.
#[gpui_kit::test]
fn label_filter_chips_refetch_with_selector(cx: &mut TestAppContext) {
    let mut k = pods_kore_demo(cx);
    assert!(k.element("labelbar.input").is_none());

    k.click("header.filter");
    k.wait_for_element("labelbar.input");
    assert_eq!(k.element("labelbar.input").unwrap().label, "app=name");
    assert_eq!(k.state()["showLabelFilter"], true);

    k.cluster.clear_requests();
    k.click("labelbar.input");
    k.type_text("app=logger");
    k.keys("enter");
    k.wait_for_element("labelbar.chip.app=logger");
    k.settle();
    k.wait_until("only logger pods", |k| {
        let names = k.row_names();
        !names.is_empty() && names.iter().all(|n| n.starts_with("logger-"))
    });
    assert!(
        k.cluster.requests().iter().any(|r| r.path.ends_with("/pods") && r.query.get("labelSelector").map(String::as_str) == Some("app=logger")),
        "no list with labelSelector=app=logger"
    );

    // Backspace on the empty input removes the chip; the full list comes back.
    k.keys("backspace");
    k.wait_gone("labelbar.chip.app=logger");
    k.settle();
    k.wait_for_element("table.row.kore-demo/crashloop");
    assert_eq!(k.state()["labelFilters"], serde_json::json!([]));

    k.type_text("app=web");
    k.keys("enter");
    k.wait_for_element("labelbar.chip.app=web");
    k.settle();
    k.wait_gone("table.row.kore-demo/crashloop");

    // Switching kind clears chips and hides the bar.
    k.click_sidebar("sidebar.kind.deployments");
    k.wait_for_element("table.row.kore-demo/logger");
    assert!(k.element("labelbar.chip.app=web").is_none());
    assert!(k.element("labelbar.input").is_none());
    assert_eq!(k.state()["labelFilters"], serde_json::json!([]));
    assert_eq!(k.state()["showLabelFilter"], false);

    k.click_sidebar("sidebar.kind.pods");
    k.wait_for_element("table.row.kore-demo/crashloop");
}
