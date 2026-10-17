use crate::testing::Kore;
use gpui_kit::TestAppContext;
use kore_core::ResourceKind;

#[gpui_kit::test]
fn pods_table_lists_fixture_pods_and_follows_watch(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.show(ResourceKind::Pods, "kore-demo");
    k.wait_for_element("table.row.kore-demo/crashloop");
    k.cluster.delete("Pod", Some("kore-demo"), "crashloop");
    k.wait_gone("table.row.kore-demo/crashloop");
}

#[gpui_kit::test]
fn slash_then_typing_fills_search(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.show(ResourceKind::Pods, "kore-demo");
    k.wait_for_element("table.row.kore-demo/crashloop");
    k.keys("/");
    k.type_text("web");
    k.wait_until("search = web", |k| k.state()["search"] == "web" && k.state()["visibleCount"] == 3);
}

#[gpui_kit::test]
fn sidebar_click_and_row_click_open_details(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.show(ResourceKind::Pods, "kore-demo");
    k.click("sidebar.kind.deployments");
    k.wait_for_element("table.row.kore-demo/web");
    k.click("table.row.kore-demo/web");
    k.wait_until("deployment details", |k| k.state()["view"] == "details");
    k.wait_for_element("deployment.scale");
    k.keys("escape");
    k.wait_until("back to table", |k| k.state()["view"] == "table");
}

#[gpui_kit::test]
fn palette_navigates_to_dashboard(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.keys("cmd-k");
    k.wait_for_element("palette.input");
    k.type_text("Dashboard");
    k.keys("enter");
    k.wait_until("dashboard", |k| k.state()["view"] == "dashboard");
    k.wait_gone("palette");
}

#[gpui_kit::test]
fn pin_survives_restart(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.show(ResourceKind::Pods, "kore-demo");
    k.click("table.pin.kore-demo/crashloop");
    k.wait_for_element("sidebar.pin.kore-demo/crashloop");
    k.restart();
    k.wait_for_element("sidebar.pin.kore-demo/crashloop");
}
