//! Pod details: header, tab keys, live status, delete, describe.

use super::helpers::{running_pod, DetailsExt, NS};
use crate::testing::Kore;
use gpui_kit::TestAppContext;

/// Scenario 20-details-pod-header: Running pill, actions, six tabs with Logs
/// active, the details row (Namespace / Node from spec.nodeName / IP), and
/// Esc, h and the back button each return to the Pods table.
#[gpui_kit::test]
fn pod_header_shows_status_actions_tabs_and_details_row(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.open_pod("multi-container");
    assert_eq!(k.label("pod.status").as_deref(), Some("Running"));
    for id in ["details.back", "pod.details-toggle", "pod.debug", "pod.delete", "pod.ports"] {
        assert!(k.element(id).is_some(), "{id} missing");
    }
    assert_eq!(k.labels("tab."), ["Logs", "Describe", "YAML", "Metrics", "Events", "Shell"]);
    k.wait_tab("logs");
    assert!(k.element("pod.info.node").is_none());

    k.click("pod.details-toggle");
    assert_eq!(k.wait_for_element("pod.info.namespace").label, NS);
    assert_eq!(k.label("pod.info.node").as_deref(), Some("kore-harness-control-plane"));
    let ip = k.label("pod.info.ip").unwrap();
    assert_eq!(ip.split('.').count(), 4, "pod IP, got {ip}");
    k.click("pod.details-toggle");
    k.wait_gone("pod.info.node");

    k.keys("escape");
    k.wait_until("table after Esc", |k| k.state()["view"] == "table");
    k.wait_for_element("table.row.kore-demo/crashloop");

    k.open_pod("multi-container");
    k.keys("h");
    k.wait_until("table after h", |k| k.state()["view"] == "table");

    k.open_pod("multi-container");
    k.click("details.back");
    k.wait_until("table after back button", |k| k.state()["view"] == "table");
    assert!(k.element("view.details").is_none());
}

/// Scenario 20-details-pod-tab-keys: 2 Describe, 3 YAML, 4 Metrics, 5 Events,
/// 6 Shell, 1 Logs. A digit typed while the terminal has focus is terminal
/// input, not a tab switch. (Echoing it back from a live shell is Layer 2.)
#[gpui_kit::test]
fn number_keys_switch_pod_tabs_but_not_inside_the_terminal(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.open_pod("multi-container");
    k.keys("2");
    k.wait_tab("describe");
    k.wait_describe("\"apiVersion\": \"v1\"");
    k.keys("3");
    k.wait_tab("yaml");
    k.wait_label("yaml.toggle", "Read-only", |l| l == "Read-only");
    let yaml = k.wait_label("yaml.text", "pod yaml", |l| l.contains("kind: Pod"));
    assert!(yaml.contains("apiVersion: v1"));
    k.keys("4");
    k.wait_tab("metrics");
    k.wait_for_element("metrics.cpu");
    k.keys("5");
    k.wait_tab("events");
    k.wait_for_element("events.live");
    k.keys("6");
    k.wait_tab("shell");
    k.wait_for_element("exec.terminal");

    // Focus the terminal: digits go to it.
    k.click("exec.terminal");
    k.keys("1");
    k.wait_tab("shell");
    assert!(k.element("tab.logs").is_some_and(|e| !e.selected));

    // Leave the terminal focus; 1 returns to Logs.
    k.cx.update(|window, cx| window.blur(cx));
    k.keys("1");
    k.wait_tab("logs");
    for id in ["logs.previous", "logs.autoscroll", "logs.copy", "logs.download"] {
        k.wait_for_element(id);
    }
}

/// Scenario 20-details-bug-live-status: the header badge follows the pod
/// (watched) instead of the status captured on open; the details row shows the
/// node name (spec.nodeName), not the host IP.
#[gpui_kit::test]
fn pod_status_badge_follows_the_live_pod(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.cluster.apply(running_pod("kore-bug-status", &["kore-bug-status"]));
    k.open_pod("kore-bug-status");
    k.wait_label("pod.status", "Running", |l| l == "Running");
    k.click("pod.details-toggle");
    assert_eq!(k.wait_for_element("pod.info.node").label, "kore-harness-control-plane");

    // The container exits 1.
    k.cluster.modify("Pod", Some(NS), "kore-bug-status", |p| {
        p["status"]["phase"] = "Failed".into();
        p["status"]["conditions"][0]["status"] = "False".into();
        p["status"]["containerStatuses"][0]["ready"] = false.into();
        p["status"]["containerStatuses"][0]["state"] =
            serde_json::json!({"terminated": {"reason": "Error", "exitCode": 1, "finishedAt": "2026-10-15T10:00:00Z"}});
    });
    k.wait_label("pod.status", "Error", |l| l == "Error");
    assert_eq!(k.state()["view"], "details");
    assert_eq!(k.label("pod.info.node").as_deref(), Some("kore-harness-control-plane"));
    k.keys("escape");
    k.wait_for_element("table.row.kore-demo/crashloop");
}

/// Scenario 20-details-pod-delete: `d` opens "Delete Pod"; Cancel closes it;
/// Delete removes the pod (DELETE on the cluster) and, once the watch reports
/// it, the view shows "Pod No Longer Exists", a Deleted badge, no tabs or
/// actions; Go Back returns to the table.
#[gpui_kit::test]
fn deleting_a_pod_shows_the_deleted_state(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.cluster.apply(running_pod("kore-delete-probe", &["main"]));
    k.open_pod("kore-delete-probe");
    k.keys("d");
    assert_eq!(k.wait_for_element("confirm").label, "Delete Pod");
    let msg = k.label("confirm.message").unwrap();
    assert!(msg.contains("Are you sure you want to delete pod \"kore-delete-probe\"") && msg.contains("cannot be undone"), "{msg}");
    k.click("confirm.cancel");
    k.wait_gone("confirm");
    assert!(k.cluster.get("Pod", Some(NS), "kore-delete-probe").is_some());

    k.cluster.clear_requests();
    k.keys("d");
    k.click("confirm.ok");
    k.wait_for_element("pod.deleted");
    assert!(k.cluster.get("Pod", Some(NS), "kore-delete-probe").is_none());
    assert!(k
        .cluster
        .requests()
        .iter()
        .any(|r| r.method == "DELETE" && r.path == "/api/v1/namespaces/kore-demo/pods/kore-delete-probe"));
    assert_eq!(k.label("pod.status").as_deref(), Some("Deleted"));
    for id in ["tab.logs", "tab.describe", "pod.delete", "pod.ports", "pod.debug", "view.pod-logs"] {
        assert!(k.element(id).is_none(), "{id} still shown");
    }
    k.click("pod.go-back");
    k.wait_until("table", |k| k.state()["view"] == "table");
    k.wait_for_element("table.row.kore-demo/crashloop");
}

/// Scenario 20-details-describe: the Describe tab renders the pod JSON; ⌘F
/// search shows a 1/N counter that Enter advances, a miss shows "No results",
/// and the bar's X closes it, staying on the pod. (Token colors are Layer 2.)
#[gpui_kit::test]
fn describe_search_counts_steps_and_closes(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.open_pod("multi-container");
    k.click("tab.describe");
    let text = k.wait_describe("\"apiVersion\": \"v1\"");
    for needle in ["\"kind\": \"Pod\"", "\"name\": \"multi-container\"", "\"namespace\": \"kore-demo\""] {
        assert!(text.contains(needle), "describe lacks {needle}");
    }
    let n = text.to_lowercase().matches("busybox").count();
    assert!(n >= 2, "fixture has several busybox mentions");

    k.open_search();
    k.type_text("busybox");
    k.wait_label("searchbar.count", "1/N", |l| l == format!("1/{n}"));
    k.keys("enter");
    k.wait_label("searchbar.count", "2/N", |l| l == format!("2/{n}"));
    k.keys("cmd-a");
    k.type_text("zzzz");
    k.wait_label("searchbar.count", "No results", |l| l == "No results");
    k.click("searchbar.close");
    k.wait_gone("searchbar");
    assert_eq!(k.state()["view"], "details");
    k.wait_tab("describe");
    k.wait_describe("\"kind\": \"Pod\"");
    assert!(k.element("tab.shell").is_some());
}

/// Scenario 20-details-bug-describe-timestamps: describe JSON keeps string
/// contents verbatim, e.g. `"creationTimestamp": "2026-10-15T10:41:26Z"` (no
/// space inserted after the hour). (Its green / amber colors are Layer 2.)
#[gpui_kit::test]
fn describe_keeps_timestamps_verbatim(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    let pod = k.cluster.get("Pod", Some(NS), "multi-container").unwrap();
    let ts = pod["metadata"]["creationTimestamp"].as_str().unwrap().to_string();
    k.open_pod("multi-container");
    k.keys("2");
    let text = k.wait_describe(&format!("\"creationTimestamp\": \"{ts}\""));
    let re = regex::Regex::new(r"T\d\d: \d\d").unwrap();
    assert!(!re.is_match(&text), "a space was inserted into a timestamp");
    assert!(text.contains("\"generation\": 1"));
}
