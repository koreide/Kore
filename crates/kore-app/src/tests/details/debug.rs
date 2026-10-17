//! Debug container dialog and the ephemeral-container request.

use super::helpers::{running_pod, DetailsExt, NS};
use crate::testing::Kore;
use gpui_kit::TestAppContext;
use serde_json::json;

/// Scenario 20-details-debug: `b` opens "Debug Container" (netshoot / busybox /
/// ubuntu / curl / Custom, Share process namespace, Cancel / Create & Connect);
/// Custom with an empty image disables Create; Share reveals the target select;
/// Cancel closes it on the pod. Creating a busybox debugger PATCHes the pod's
/// ephemeralcontainers subresource, shows "Creating debug container...", and
/// once the container runs switches to Shell with a ready toast, the "Debug:"
/// banner and "Stop Debug"; re-opening the pod restores that state. (Attaching
/// to the shell and Stop Debug, which attaches to the container, are Layer 2.)
#[gpui_kit::test]
fn debug_dialog_options_and_ephemeral_container_request(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.cluster.apply(running_pod("kore-debug-target", &["kore-debug-target"]));
    k.open_pod("kore-debug-target");
    assert_eq!(k.label("pod.debug").as_deref(), Some("Debug container"));
    k.keys("b");
    assert_eq!(k.wait_for_element("debug").label, "Debug Container");
    k.wait_until("modal registered", |k| k.state()["modals"].as_array().is_some_and(|m| m.iter().any(|v| v == "debug-container")));
    let images: Vec<String> = k.labels("debug.image.");
    assert_eq!(images, ["netshoot", "busybox", "ubuntu", "curl", "Custom"]);
    assert!(k.element("debug.image.netshoot").unwrap().selected);
    for id in ["debug.share", "debug.cancel", "debug.create"] {
        assert!(k.element(id).is_some(), "{id} missing");
    }
    assert!(!k.element("debug.create").unwrap().disabled);

    // Custom with an empty image: input appears, Create & Connect is disabled.
    k.click("debug.image.custom");
    assert_eq!(k.wait_for_element("debug.custom").label, "e.g. alpine:latest");
    assert!(k.element("debug.create").unwrap().disabled);

    // Share process namespace reveals the target select.
    k.click("debug.share");
    assert_eq!(k.wait_for_element("debug.target").label, "Select a container...");
    k.click("debug.share");
    k.wait_gone("debug.target");

    // Cancel closes it and stays on the pod.
    k.click("debug.cancel");
    k.wait_gone("debug");
    assert_eq!(k.state()["view"], "details");
    assert!(k.element("logs.autoscroll").is_some());

    // Create a busybox debugger targeting the pod's container.
    k.keys("b");
    k.wait_for_element("debug");
    k.click("debug.image.busybox");
    k.wait_until("busybox selected", |k| k.element("debug.image.busybox").is_some_and(|e| e.selected));
    k.click("debug.share");
    k.click("debug.target");
    k.click("debug.target.option.kore-debug-target");
    k.wait_label("debug.target", "target chosen", |l| l == "kore-debug-target");
    k.cluster.clear_requests();
    k.click("debug.create");
    k.wait_for_element("debug.loading");
    let path = "/api/v1/namespaces/kore-demo/pods/kore-debug-target/ephemeralcontainers";
    let mut body = serde_json::Value::Null;
    k.wait_until("ephemeralcontainers PATCH", |k| match k.cluster.requests().into_iter().find(|r| r.method == "PATCH" && r.path == path) {
        Some(r) => {
            body = r.body.unwrap_or_default();
            true
        }
        None => false,
    });
    let c = &body["spec"]["ephemeralContainers"][0];
    let name = c["name"].as_str().unwrap().to_string();
    assert!(regex::Regex::new(r"^debugger-[0-9a-f]{8}$").unwrap().is_match(&name), "{name}");
    assert_eq!(c["image"], "busybox:latest");
    assert_eq!(c["targetContainerName"], "kore-debug-target");
    assert_eq!(c["command"], json!(["/bin/sh"]));
    assert_eq!((c["stdin"].clone(), c["tty"].clone()), (json!(true), json!(true)));

    // The kubelet (not faked) starts it.
    k.cluster.modify("Pod", Some(NS), "kore-debug-target", |p| {
        p["status"]["ephemeralContainerStatuses"] = json!([{"name": name, "image": "busybox:latest", "ready": false,
            "restartCount": 0, "state": {"running": {"startedAt": "2026-10-15T10:00:00Z"}}}]);
    });
    k.wait_toast("ready", |l| l == format!("Debug container \"{name}\" is ready"));
    k.wait_gone("debug");
    k.wait_tab("shell");
    assert_eq!(k.wait_for_element("pod.debug-banner").label, format!("Debug: {name}"));
    assert_eq!(k.label("pod.debug"), Some(format!("Stop debug container \"{name}\"")));
    assert!(k.state()["modals"].as_array().is_some_and(|m| m.is_empty()));

    // Back and re-open: the debug state is restored from the pod.
    k.click("details.back");
    k.wait_until("table", |k| k.state()["view"] == "table");
    k.open_pod("kore-debug-target");
    k.wait_label("pod.debug", "Stop Debug", |l| l == format!("Stop debug container \"{name}\""));
    assert_eq!(k.wait_for_element("pod.debug-banner").label, format!("Debug: {name}"));
}
