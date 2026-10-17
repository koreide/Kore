//! Deployment details: header, tabs, scale / restart / delete, logs tab.

use super::helpers::{deployment, labelled_pod, DetailsExt, NS};
use crate::testing::Kore;
use gpui_kit::TestAppContext;
use kore_core::ResourceKind;
use serde_json::json;

/// Scenario 20-details-deployment: Back, replica stepper, Scale / Restart /
/// Delete; NAME / NAMESPACE / READY / UP-TO-DATE / AVAILABLE; tabs Describe
/// YAML Rollback Logs Events with keys 1-5; Esc returns to the table.
#[gpui_kit::test]
fn deployment_details_header_tabs_and_keys(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.open(ResourceKind::Deployments, "web");
    for id in ["details.back", "deployment.replicas.minus", "deployment.replicas.plus", "deployment.scale", "deployment.restart", "deployment.delete"] {
        assert!(k.element(id).is_some(), "{id} missing");
    }
    assert_eq!(k.label("info.name").as_deref(), Some("web"));
    assert_eq!(k.label("info.namespace").as_deref(), Some(NS));
    assert_eq!(k.label("info.ready").as_deref(), Some("3/3"));
    assert_eq!(k.label("info.up-to-date").as_deref(), Some("3"));
    assert_eq!(k.label("info.available").as_deref(), Some("3"));
    assert_eq!(k.labels("tab."), ["Describe", "YAML", "Rollback", "Logs", "Events"]);
    k.wait_tab("describe");
    let text = k.wait_describe("\"kind\": \"Deployment\"");
    assert!(text.contains("\"name\": \"web\"") && text.contains("\"replicas\": 3"));
    k.wait_label("deployment.replicas", "3", |l| l == "3");

    k.keys("2");
    k.wait_tab("yaml");
    k.wait_label("yaml.toggle", "Read-only", |l| l == "Read-only");
    k.wait_label("yaml.text", "deployment yaml", |l| l.contains("kind: Deployment"));
    k.keys("3");
    k.wait_tab("rollback");
    k.wait_for_element("view.rollback");
    k.keys("4");
    k.wait_tab("logs");
    k.wait_label("multilogs.selector", "kore-demo/app=web", |l| l == "kore-demo/app=web");
    for id in ["multilogs.filter", "multilogs.copy", "multilogs.autoscroll"] {
        assert!(k.element(id).is_some(), "{id} missing");
    }
    k.keys("5");
    k.wait_tab("events");
    k.wait_for_element("events.live");
    k.keys("1");
    k.wait_tab("describe");
    k.wait_describe("\"kind\": \"Deployment\"");
    k.keys("escape");
    k.wait_until("table", |k| k.state()["view"] == "table");
    k.wait_for_element("table.row.kore-demo/logger");
    assert!(k.element("deployment.scale").is_none() && k.element("view.rollback").is_none());
}

/// Scenario 20-details-deployment-actions: on a Deployment selected by
/// run=kore-scratch (no app=<name> label), Logs uses that selector ("No logs
/// available" for its silent pod); + then Scale patches 2 replicas and toasts;
/// Restart asks and can be cancelled; Delete asks, deletes, toasts and returns.
#[gpui_kit::test]
fn deployment_scale_restart_cancel_and_delete(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.cluster.apply(deployment("kore-scratch", json!({"run": "kore-scratch"}), 1, &["c"]));
    k.cluster.apply(labelled_pod("kore-scratch-abc12-x1y2z", json!({"run": "kore-scratch"}), &["c"]));
    k.open(ResourceKind::Deployments, "kore-scratch");
    assert_eq!(k.wait_for_element("info.ready").label, "1/1");

    k.keys("4");
    k.wait_label("multilogs.selector", "real selector", |l| l == "kore-demo/run=kore-scratch");
    k.wait_label("multilogs.empty", "No logs available", |l| l == "No logs available");
    assert!(k.element("multilogs.error").is_none());

    // Scale 1 -> 2
    k.keys("1");
    k.wait_describe("\"kind\": \"Deployment\"");
    k.wait_label("deployment.replicas", "1", |l| l == "1");
    k.click("deployment.replicas.plus");
    k.wait_label("deployment.replicas", "2", |l| l == "2");
    k.cluster.clear_requests();
    k.click("deployment.scale");
    k.wait_toast("Scaled to 2 replicas", |l| l == "Scaled to 2 replicas");
    assert_eq!(k.cluster.get("Deployment", Some(NS), "kore-scratch").unwrap()["spec"]["replicas"], 2);
    assert!(k.cluster.requests().iter().any(|r| r.method == "PATCH" && r.path.contains("/namespaces/kore-demo/deployments/kore-scratch")));

    // Restart: confirm, then cancel (nothing patched).
    k.cluster.clear_requests();
    k.click("deployment.restart");
    assert_eq!(k.wait_for_element("confirm").label, "Restart Deployment");
    assert!(k.label("confirm.message").unwrap().contains("rolling restart of deployment \"kore-scratch\""));
    k.click("confirm.cancel");
    k.wait_gone("confirm");
    assert!(!k.cluster.requests().iter().any(|r| r.method == "PATCH"));

    // Delete
    k.click("deployment.delete");
    assert_eq!(k.wait_for_element("confirm").label, "Delete Deployment");
    assert!(k.label("confirm.message").unwrap().contains("want to delete deployment \"kore-scratch\""));
    k.click("confirm.ok");
    k.wait_toast("Deployment deleted", |l| l == "Deployment deleted");
    k.wait_until("table", |k| k.state()["view"] == "table");
    k.wait_gone("confirm");
    assert!(k.cluster.get("Deployment", Some(NS), "kore-scratch").is_none());
    k.wait_gone("table.row.kore-demo/kore-scratch");
}

/// Scenario 20-details-fix-deployment-restart: confirming Restart succeeds
/// (merge patch without `force`, which kube-rs rejects client-side): toast
/// "Rolling restart initiated", the dialog closes and the pod template gets
/// kubectl.kubernetes.io/restartedAt.
#[gpui_kit::test]
fn deployment_restart_patches_restarted_at(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.cluster.apply(deployment("kore-restart-me", json!({"app": "kore-restart-me"}), 1, &["busybox"]));
    k.open(ResourceKind::Deployments, "kore-restart-me");
    k.cluster.clear_requests();
    k.click("deployment.restart");
    k.wait_for_element("confirm.ok");
    k.click("confirm.ok");
    k.wait_toast("Rolling restart initiated", |l| l == "Rolling restart initiated");
    k.wait_gone("confirm");
    let d = k.cluster.get("Deployment", Some(NS), "kore-restart-me").unwrap();
    let at = &d["spec"]["template"]["metadata"]["annotations"]["kubectl.kubernetes.io/restartedAt"];
    assert!(at.as_str().is_some_and(|s| !s.is_empty()), "restartedAt not set: {d}");
    let patch = k.cluster.requests().into_iter().find(|r| r.method == "PATCH").expect("restart PATCH");
    assert!(patch.headers.get("content-type").is_some_and(|c| c.contains("merge-patch")), "{:?}", patch.headers);
    assert!(!patch.query.contains_key("force"));
}

/// Scenario 20-details-bug-deployment-selector: the Logs tab builds its
/// selector from the Deployment's real spec.selector (no app label here) and
/// streams every container of each pod with a `[container]` prefix.
#[gpui_kit::test]
fn deployment_logs_use_the_real_selector_and_all_containers(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    let labels = json!({"tier": "bugsel", "team": "kore"});
    k.cluster.apply(deployment("kore-bug-selector", labels.clone(), 1, &["c-alpha", "c-beta"]));
    let pod = "kore-bug-selector-5d9f8-abcde";
    k.cluster.apply(labelled_pod(pod, labels, &["c-alpha", "c-beta"]));
    k.cluster.set_logs(NS, pod, "c-alpha", &["ALPHA-LINE 1", "ALPHA-LINE 2"]);
    k.cluster.set_logs(NS, pod, "c-beta", &["BETA-LINE 1", "BETA-LINE 2"]);
    k.open(ResourceKind::Deployments, "kore-bug-selector");
    k.keys("4");
    k.wait_label("multilogs.selector", "real selector", |l| l == "kore-demo/team=kore,tier=bugsel");
    k.wait_row("multilogs.row.", "[c-alpha] ALPHA-LINE");
    k.wait_row("multilogs.row.", "[c-beta] BETA-LINE");
    k.wait_label("multilogs.count", "1 pod", |l| l == "1 pod");
    assert!(k.element("multilogs.error").is_none());
    // Followed: a new line shows up.
    k.cluster.append_log(NS, pod, "c-beta", "BETA-LINE 3");
    k.wait_row("multilogs.row.", "[c-beta] BETA-LINE 3");
    k.keys("escape");
    k.wait_until("table", |k| k.state()["view"] == "table");
}

/// Scenarios 20-details-fix-deployment-logs and 20-details-multi-pod-logs: the
/// logger Deployment's Logs tab ("kore-demo/app=logger", Filter / Copy /
/// Auto-scroll) streams lines from both logger pods with pod-name prefixes and
/// counts "2 pods"; Filter lists both pods, None hides everything "(2 hidden)",
/// All restores.
#[gpui_kit::test]
fn deployment_logs_stream_both_pods_and_filter(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.open(ResourceKind::Deployments, "logger");
    k.click("tab.logs");
    k.wait_label("multilogs.selector", "app=logger", |l| l == "kore-demo/app=logger");
    for id in ["multilogs.filter", "multilogs.copy", "multilogs.autoscroll"] {
        assert!(k.element(id).is_some(), "{id} missing");
    }
    let re = regex::Regex::new(r"^logger-[a-z0-9]+-[a-z0-9]+ KORE-LOG logger-.* line=\d+ level=").unwrap();
    k.wait_until("a logger line", |k| k.labels("multilogs.row.").iter().any(|l| re.is_match(l)));
    k.wait_label("multilogs.count", "2 pods", |l| l == "2 pods");

    k.click("multilogs.filter");
    for pod in ["logger-684d79cb86-26m9x", "logger-684d79cb86-7r5gn"] {
        assert!(k.wait_for_element(&format!("multilogs.pod.{pod}")).selected, "{pod} visible");
    }
    k.wait_for_element("multilogs.all");
    k.click("multilogs.none");
    k.wait_label("multilogs.empty", "all hidden", |l| l.starts_with("All pods are hidden"));
    assert_eq!(k.label("multilogs.count").as_deref(), Some("2 pods (2 hidden)"));
    assert!(k.labels("multilogs.row.").is_empty());
    k.click("multilogs.all");
    k.wait_row("multilogs.row.", "KORE-LOG logger-");
    assert_eq!(k.label("multilogs.count").as_deref(), Some("2 pods"));
}
