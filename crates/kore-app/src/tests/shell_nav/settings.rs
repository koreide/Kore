//! Settings page (spec 01 §17, §21.37-40).

use super::helpers::{release_json, JsonServer, ShellExt};
use crate::testing::Kore;
use gpui_kit::TestAppContext;
use kore_core::ResourceKind;
use kore_testkit::fixtures::{self, CONTEXT, CONTEXT_2};
use serde_json::json;

const AREA: &str = "view.settings";

fn settings(k: &mut Kore) -> crate::state::settings::Settings {
    k.app_state(|s, _| s.settings.clone())
}

/// Scroll to `id` on the Settings page and click it.
fn click_setting(k: &mut Kore, id: &str) {
    k.scroll_into_view(AREA, id);
    k.click(id);
}

/// Scenario 10-shell-settings-page: the sidebar gear opens Settings (Back + Esc
/// hint; General / AI Provider / Display / Keyboard Shortcuts / About); Esc returns
/// to the table, and so does Back.
#[gpui_kit::test]
fn settings_page_sections_and_back_navigation(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.pods_kore_demo();
    k.open_settings();
    assert!(k.element("sidebar.settings").unwrap().selected);
    k.wait_label("settings.back", "Back", |l| l == "Back");
    assert!(k.element("header.search").is_none());
    let cards = k.labels_in_order("settings.card.");
    assert_eq!(cards, ["General", "Display", "Keyboard Shortcuts", "About"]);
    // General.
    k.wait_label("settings.default-view", "AI Chat", |l| l == "AI Chat");
    k.wait_label("settings.retention", "7 days", |l| l == "7 days");
    // AI Provider.
    let providers = k.labels_in_order("settings.ai.provider.");
    for p in ["OpenAI", "Anthropic", "Ollama", "Claude Code", "Cursor Agent"] {
        assert!(providers.iter().any(|l| l == p), "{p} missing from {providers:?}");
    }
    // Display.
    k.wait_for_element("settings.default-ns.context");
    k.wait_for_element("settings.default-ns.namespace");
    k.wait_for_element("settings.default-ns.add");
    assert_eq!(k.labels_in_order("settings.accent."), ["Cyan", "Violet", "Emerald", "Orange", "Red"]);
    // Keyboard Shortcuts.
    for s in ["Navigate up / down", "Enter detail view", "Command palette", "Refresh resources", "Search logs", "Delete resource", "Show shortcut overlay"] {
        k.wait_for_element(&format!("settings.shortcut.{s}"));
    }
    // About.
    k.wait_for_element("settings.version");
    k.wait_for_element("settings.check-updates");

    k.keys("escape");
    k.wait_until("table", |k| k.state()["view"] == "table");
    k.wait_gone(AREA);
    k.wait_el_ff("table.header.restarts");

    k.open_settings();
    k.click("settings.back");
    k.wait_until("table again", |k| k.state()["view"] == "table");
    k.wait_gone(AREA);
}

/// Scenario 10-shell-settings-default-ns: "Add" is inert until both inputs are
/// filled; adding lists "ctx1 -> ns1" (and clears the inputs); "x" removes it.
#[gpui_kit::test]
fn default_namespace_entries_add_and_remove(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.pods_kore_demo();
    k.open_settings();
    assert!(k.element("settings.default-ns.add").unwrap().disabled);

    click_setting(&mut k, "settings.default-ns.context");
    k.type_text("ctx1");
    assert!(k.element("settings.default-ns.add").unwrap().disabled, "Add needs a namespace too");
    k.click("settings.default-ns.add");
    assert!(k.element("settings.default-ns.remove.ctx1").is_none());
    assert!(settings(&mut k).default_namespaces.is_empty());

    k.click("settings.default-ns.namespace");
    k.type_text("ns1");
    k.wait_until("Add enabled", |k| k.element("settings.default-ns.add").is_some_and(|e| !e.disabled));
    k.click("settings.default-ns.add");
    k.wait_for_element("settings.default-ns.remove.ctx1");
    assert_eq!(settings(&mut k).default_namespaces.get("ctx1").map(String::as_str), Some("ns1"));
    // Inputs cleared: Add is disabled again.
    k.wait_until("inputs cleared", |k| k.element("settings.default-ns.add").is_some_and(|e| e.disabled));

    k.click("settings.default-ns.remove.ctx1");
    k.wait_gone("settings.default-ns.remove.ctx1");
    assert!(settings(&mut k).default_namespaces.is_empty());

    k.keys("escape");
    k.wait_until("table", |k| k.state()["view"] == "table");
}

/// Scenario 10-shell-settings-default-view: Default View = "Resource Table" makes the
/// next launch open on the Pods table instead of AI Chat; back to AI Chat afterwards.
#[gpui_kit::test]
fn default_view_setting_applies_on_next_launch(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    assert_eq!(k.view(), "chat");
    k.pods_kore_demo();
    k.open_settings();
    k.click("settings.default-view");
    k.wait_for_element("settings.default-view.option.table");
    k.keys("down enter");
    k.wait_label("settings.default-view", "Resource Table", |l| l == "Resource Table");
    assert_eq!(settings(&mut k).default_view, "table");

    k.restart();
    k.wait_until("boots on table", |k| k.state()["view"] == "table");
    assert_eq!(k.state()["kind"], "pods");
    k.wait_for_element("header.search");
    k.wait_label("header.breadcrumb", "Pods", |l| l == "Pods");
    assert!(k.element("view.chat").is_none());

    k.open_settings();
    k.click("settings.default-view");
    k.keys("up enter");
    k.wait_label("settings.default-view", "AI Chat", |l| l == "AI Chat");
    k.keys("escape");
    k.restart();
    k.wait_until("boots on chat", |k| k.state()["view"] == "chat");
}

/// Scenario 10-shell-settings-accent: choosing Violet recolors the accent at once
/// and persists across restarts; Cyan restores the default. (Pixels are Layer 2.)
#[gpui_kit::test]
fn accent_color_applies_and_persists(cx: &mut TestAppContext) {
    let accent = |k: &mut Kore| k.cx.update(|_, cx| crate::theme::accent(cx));
    let mut k = Kore::boot(cx);
    k.pods_kore_demo();
    assert_eq!(k.state()["accent"], "#58d0ff");
    assert_eq!(accent(&mut k), crate::theme::hex(0x58d0ff));

    k.open_settings();
    assert!(k.element("settings.accent.Cyan").unwrap().selected);
    click_setting(&mut k, "settings.accent.Violet");
    k.wait_until("violet", |k| k.state()["accent"] == "#a78bfa");
    assert_eq!(accent(&mut k), crate::theme::hex(0xa78bfa));
    assert!(k.element("settings.accent.Violet").unwrap().selected);
    assert!(!k.element("settings.accent.Cyan").unwrap().selected);
    k.keys("escape");
    k.wait_until("table", |k| k.state()["view"] == "table");

    k.restart();
    assert_eq!(k.state()["accent"], "#a78bfa");
    assert_eq!(accent(&mut k), crate::theme::hex(0xa78bfa));

    k.open_settings();
    click_setting(&mut k, "settings.accent.Cyan");
    k.wait_until("cyan", |k| k.state()["accent"] == "#58d0ff");
    assert_eq!(accent(&mut k), crate::theme::hex(0x58d0ff));
}

/// Scenario 10-shell-settings-updates: "Check for Updates" queries the update endpoint
/// and shows "You're up to date!" (closed with OK), or the "Kore <ver> is available" banner.
#[gpui_kit::test]
fn check_for_updates_up_to_date_then_available(cx: &mut TestAppContext) {
    let server = JsonServer::start(&release_json("v0.0.1"));
    let (env, c1, c2) = fixtures::harness_env();
    let env = env.update_url(&server.url);
    let mut k = Kore::boot_env(cx, env, c1, c2);
    // The boot-time check already ran (and found nothing newer).
    k.wait_until("boot check", |k| k.state()["update"]["info"].is_object());
    assert_eq!(k.state()["update"]["info"]["has_update"], false);
    let hits = server.hits();
    k.open_settings();
    let version = env!("CARGO_PKG_VERSION");
    k.wait_label("settings.version", "current version", |l| l == version);

    click_setting(&mut k, "settings.check-updates");
    k.wait_label("settings.up-to-date", "up to date", |l| l == "You're up to date!");
    assert_eq!(server.hits(), hits + 1);
    assert!(k.element("settings.update-error").is_none());
    assert!(k.element("banner.update").is_none());
    k.click("settings.up-to-date.ok");
    k.wait_gone("settings.up-to-date");
    assert_eq!(k.view(), "settings");

    // A newer release: the banner, no modal.
    server.set_body(&release_json("v99.0.0"));
    click_setting(&mut k, "settings.check-updates");
    k.wait_label("banner.update", "is available", |l| l == "Kore v99.0.0 is available");
    assert!(k.element("settings.up-to-date").is_none());
    assert_eq!(k.state()["update"]["info"]["latest_version"], "v99.0.0");
    k.keys("escape");
    k.wait_until("table", |k| k.state()["view"] == "table");
}

/// Scenario 10-shell-bug-update-error: when the check fails (endpoint unreachable)
/// Settings shows "Couldn't check for updates" with the error, not "You're up to
/// date!"; OK closes it.
#[gpui_kit::test]
fn failed_update_check_shows_error_not_up_to_date(cx: &mut TestAppContext) {
    // FakeEnv's default update URL is a closed local port.
    let mut k = Kore::boot(cx);
    k.open_settings();
    click_setting(&mut k, "settings.check-updates");
    let label = k.wait_label("settings.update-error", "error", |l| l.starts_with("Couldn't check for updates: "));
    assert!(label.len() > "Couldn't check for updates: ".len(), "{label}");
    assert!(k.element("settings.up-to-date").is_none());
    k.click("settings.up-to-date.ok");
    k.wait_gone("settings.update-error");
    // Esc closed nothing else first: back to the table.
    k.keys("escape");
    k.wait_until("table", |k| k.state()["view"] == "table");
}

/// Scenario 10-shell-fix-event-retention: Settings > Event Retention Period is applied
/// to the event history store: "1 day" prunes events last seen more than a day ago
/// and keeps newer ones.
#[gpui_kit::test]
fn event_retention_setting_prunes_old_stored_events(cx: &mut TestAppContext) {
    let (env, c1, c2) = fixtures::harness_env();
    let now = chrono::Utc::now();
    let event = |name: &str, ago: chrono::Duration| {
        let ts = (now - ago).format("%Y-%m-%dT%H:%M:%SZ").to_string();
        json!({
            "apiVersion": "v1", "kind": "Event",
            "metadata": {"name": name, "namespace": "kore-demo", "uid": name, "creationTimestamp": ts},
            "involvedObject": {"kind": "Pod", "name": name, "namespace": "kore-demo"},
            "reason": "Test", "message": name, "type": "Normal", "count": 1,
            "firstTimestamp": ts, "lastTimestamp": ts,
        })
    };
    c1.apply(event("retention-old", chrono::Duration::days(3)));
    c1.apply(event("retention-new", chrono::Duration::hours(1)));
    let mut k = Kore::boot_env(cx, env, c1, c2);

    let stored = |k: &mut Kore| -> Vec<String> {
        let core = k.cx.update(|_, cx| crate::backend::core(cx));
        let since = (now - chrono::Duration::days(30)).to_rfc3339();
        let until = (now + chrono::Duration::hours(1)).to_rfc3339();
        let mut uids: Vec<String> = futures::executor::block_on(core.query_stored_events(since, until, Some("kore-demo".into())))
            .unwrap_or_default()
            .into_iter()
            .map(|e| e.uid)
            .filter(|u| u.starts_with("retention-"))
            .collect();
        uids.sort();
        uids
    };
    // Listing events records them in the history store (7-day retention keeps both).
    k.show(ResourceKind::Events, "kore-demo");
    k.wait_el_ff("table.row.kore-demo/retention-old");
    k.wait_until("both stored", |k| stored(k) == ["retention-new", "retention-old"]);

    k.open_settings();
    k.click("settings.retention");
    k.wait_for_element("settings.retention.option.1d");
    k.keys("up up enter");
    k.wait_label("settings.retention", "1 day", |l| l == "1 day");
    assert_eq!(settings(&mut k).event_retention, "1d");
    k.wait_until("old pruned", |k| stored(k) == ["retention-new"]);
}

/// Scenario 10-shell-bug-default-namespace: "Default Namespace per Context" is applied
/// when switching back to that context and at startup.
#[gpui_kit::test]
fn default_namespace_per_context_applies_on_switch_and_launch(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.pods_kore_demo();
    k.open_settings();
    click_setting(&mut k, "settings.default-ns.context");
    k.type_text(CONTEXT);
    k.click("settings.default-ns.namespace");
    k.type_text("kore-rbac");
    k.click("settings.default-ns.add");
    k.wait_for_element(&format!("settings.default-ns.remove.{CONTEXT}"));
    k.keys("escape");
    k.wait_until("table", |k| k.state()["view"] == "table");
    assert_eq!(k.state()["namespace"], "kore-demo");

    // Switch away and back: the configured namespace is selected.
    k.click("sidebar.context.trigger");
    k.click(&format!("sidebar.context.item.{CONTEXT_2}"));
    k.wait_until("context 2", |k| k.state()["context"] == CONTEXT_2);
    k.wait_until("cluster 2 namespaces", |k| k.state()["namespace"] == "default");
    k.click("sidebar.context.trigger");
    k.click(&format!("sidebar.context.item.{CONTEXT}"));
    k.wait_until("context 1", |k| k.state()["context"] == CONTEXT);
    k.wait_until("kore-rbac", |k| k.state()["namespace"] == "kore-rbac");
    k.wait_label("sidebar.namespace.trigger", "kore-rbac", |l| l == "kore-rbac");

    // And at startup.
    k.select_namespace("kore-demo");
    k.restart();
    k.wait_until("kore-rbac at launch", |k| k.state()["namespace"] == "kore-rbac");
}
