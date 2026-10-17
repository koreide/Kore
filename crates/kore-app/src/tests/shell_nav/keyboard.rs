//! Global keyboard shortcuts and overlays (spec 01 §3, §15, §21.35-36, Fix 4).

use super::helpers::ShellExt;
use crate::testing::Kore;
use gpui_kit::{point, px, Modifiers, TestAppContext};

fn settle(k: &mut Kore) {
    for _ in 0..5 {
        k.frame();
    }
}

/// Scenario 10-shell-shortcut-overlay: "?" (outside inputs) toggles the Keyboard
/// Shortcuts overlay with its four groups; it closes with "?", Esc, or a backdrop click.
#[gpui_kit::test]
fn shortcut_overlay_toggles_and_closes(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.pods_kore_demo();
    k.keys("?");
    k.wait_for_element("shortcuts");
    assert_eq!(k.state()["shortcutsOpen"], true);
    assert_eq!(k.labels_in_order("shortcuts.group."), ["Navigation", "Actions", "RBAC", "General"]);
    for item in [
        "Move down",
        "Move up",
        "Enter detail view",
        "Go back",
        "Switch tabs",
        "Delete resource",
        "Command palette",
        "Refresh resources",
        "Search / filter",
        "Search logs",
        "Open RBAC view",
        "Toggle impersonation",
        "Switch RBAC tabs",
        "Close / go back",
        "Select / confirm",
        "Toggle this overlay",
    ] {
        k.wait_for_element(&format!("shortcuts.item.{item}"));
    }
    assert_eq!(k.element("shortcuts.item.Toggle this overlay").unwrap().label, "?");

    k.keys("?");
    k.wait_gone("shortcuts");
    k.keys("?");
    k.wait_for_element("shortcuts");
    k.keys("escape");
    k.wait_gone("shortcuts");
    assert_eq!(k.view(), "table", "Esc only closed the overlay");

    k.keys("?");
    let dialog = k.wait_for_element("shortcuts.group.general");
    // Backdrop click (well below the dialog).
    let y = (dialog.bounds.y + dialog.bounds.h + 60.).min(890.);
    k.cx.simulate_click(point(px(700.), px(y)), Modifiers::default());
    k.frame();
    k.wait_gone("shortcuts");
    assert_eq!(k.state()["shortcutsOpen"], false);
    assert_eq!(k.view(), "table");
    k.wait_for_element("table.header.restarts");

    // Inside a text input "?" types instead.
    k.click("header.search");
    k.keys("?");
    settle(&mut k);
    assert!(k.element("shortcuts").is_none());
    k.wait_until("typed ?", |k| k.state()["search"] == "?");
}

/// Scenario 10-shell-rbac-shortcut: Cmd+Shift+R outside inputs opens the RBAC view
/// rather than refreshing, whether the key arrives as "r" or as "R" (macOS reports
/// the shifted letter); inside an input it does nothing.
#[gpui_kit::test]
fn cmd_shift_r_opens_rbac_outside_inputs(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    let cmd_shift = Modifiers { platform: true, shift: true, ..Default::default() };

    // Parsed "cmd-shift-r" (key "r").
    k.pods_kore_demo();
    k.keys("cmd-shift-r");
    k.wait_until("rbac", |k| k.state()["view"] == "rbac");
    k.wait_for_element("view.rbac");
    k.wait_for_element("tab.permission-matrix");
    k.click("sidebar.kind.pods");
    k.wait_until("table", |k| k.state()["view"] == "table");

    // macOS shape: key "R" with shift + cmd.
    k.raw_key("R", cmd_shift);
    k.wait_until("rbac via R", |k| k.state()["view"] == "rbac");
    k.click("sidebar.kind.pods");
    k.wait_until("table", |k| k.state()["view"] == "table");

    // Inside an input: neither shape opens RBAC.
    k.click("header.search");
    k.keys("cmd-shift-r");
    settle(&mut k);
    k.raw_key("R", cmd_shift);
    settle(&mut k);
    assert_eq!(k.view(), "table");
    assert!(k.element("view.rbac").is_none());
}

/// Scenario 10-shell-fix-esc-overlays (Fix 4): on Settings (where Esc means Back),
/// Esc with the palette or the shortcut overlay open only closes that overlay.
#[gpui_kit::test]
fn esc_closes_overlay_without_leaving_settings(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.pods_kore_demo();
    k.open_settings();

    k.open_palette();
    k.keys("escape");
    k.wait_gone("palette");
    settle(&mut k);
    assert_eq!(k.view(), "settings");
    k.wait_for_element("settings.back");

    k.keys("?");
    k.wait_for_element("shortcuts");
    k.keys("escape");
    k.wait_gone("shortcuts");
    settle(&mut k);
    assert_eq!(k.view(), "settings");
    k.wait_for_element("settings.default-view");

    k.keys("escape");
    k.wait_until("table", |k| k.state()["view"] == "table");
    k.wait_el_ff("table.header.restarts");
}
