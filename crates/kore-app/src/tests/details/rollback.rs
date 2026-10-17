//! Deployment Rollback tab: timeline, revision details and YAML, rollback.

use super::helpers::{DetailsExt, NS};
use crate::testing::Kore;
use gpui_kit::TestAppContext;
use kore_core::ResourceKind;

fn web_image(k: &Kore) -> String {
    let d = k.cluster.get("Deployment", Some(NS), "web").unwrap();
    d["spec"]["template"]["spec"]["containers"][0]["image"].as_str().unwrap().to_string()
}

fn open_rollback_tab(k: &mut Kore) {
    k.open(ResourceKind::Deployments, "web");
    k.keys("3");
    k.wait_for_element("view.rollback");
    k.wait_for_element("rollback.rev.53");
}

/// Scenario 20-details-rollback: the timeline lists web's revisions (current
/// first, tagged current) with their images; expanding one shows Image,
/// Created, Replicas, Change Cause ("No annotation"), View YAML (the
/// ReplicaSet YAML viewer, Hide YAML) and Rollback, which asks "Rollback
/// Deployment"; Cancel leaves the deployment unchanged.
#[gpui_kit::test]
fn rollback_timeline_details_yaml_and_cancel(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    open_rollback_tab(&mut k);
    let revs: Vec<(String, String, bool)> = k
        .elements()
        .into_iter()
        .filter(|e| e.id.starts_with("rollback.rev."))
        .map(|e| (e.id.clone(), e.label.clone(), e.selected))
        .collect();
    assert_eq!(
        revs,
        [
            ("rollback.rev.53".into(), "nginx:1.27-alpine".into(), true),
            ("rollback.rev.52".into(), "nginx:1.26-alpine".into(), false),
            ("rollback.rev.51".into(), "nginx:1.25-alpine".into(), false),
        ]
    );

    k.click("rollback.rev.52");
    assert_eq!(k.wait_for_element("rollback.cell.image").label, "nginx:1.26-alpine");
    assert_eq!(k.label("rollback.cell.replicas").as_deref(), Some("0"));
    assert_eq!(k.label("rollback.cell.change-cause").as_deref(), Some("No annotation"));
    assert!(k.label("rollback.cell.created").is_some_and(|c| c != "Invalid Date"));
    assert!(k.element("rollback.rollback.52").is_some());
    assert_eq!(k.label("rollback.yaml.52").as_deref(), Some("View YAML"));

    k.click("rollback.yaml.52");
    let yaml = k.wait_label("rollback.yaml-viewer", "ReplicaSet YAML", |l| l.contains("kind: ReplicaSet"));
    assert!(yaml.contains("apiVersion: apps/v1") && yaml.contains("web-5bfcff88dc"));
    assert_eq!(k.label("rollback.yaml.52").as_deref(), Some("Hide YAML"));
    k.click("rollback.yaml.52");
    k.wait_gone("rollback.yaml-viewer");

    k.click("rollback.rollback.52");
    assert_eq!(k.wait_for_element("confirm").label, "Rollback Deployment");
    let msg = k.label("confirm.message").unwrap();
    assert!(msg.starts_with("Roll back \"web\" to revision 52?") && msg.contains("This will replace"), "{msg}");
    k.click("confirm.cancel");
    k.wait_gone("confirm");
    assert_eq!(web_image(&k), "nginx:1.27-alpine");
}

/// Scenario 20-details-fix-rollback: confirming a rollback patches the
/// Deployment's template (merge patch, no `force`): toast "Rolled back to
/// revision 52", the live image becomes 1.26 and, once the controller
/// renumbers the ReplicaSet, the timeline's current revision shows it; rolling
/// forward to the 1.27 revision restores the fixture state.
#[gpui_kit::test]
fn rollback_and_roll_forward_patch_the_deployment(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    open_rollback_tab(&mut k);
    k.cluster.clear_requests();
    k.click("rollback.rev.52");
    k.click("rollback.rollback.52");
    k.click("confirm.ok");
    k.wait_toast("rolled back", |l| l == "Rolled back to revision 52");
    k.wait_gone("confirm");
    assert_eq!(web_image(&k), "nginx:1.26-alpine");
    let patch = k.cluster.requests().into_iter().find(|r| r.method == "PATCH").expect("rollback PATCH");
    assert_eq!(patch.path, "/apis/apps/v1/namespaces/kore-demo/deployments/web");
    assert!(!patch.query.contains_key("force"));

    // The deployment controller (not faked) renumbers the restored ReplicaSet.
    k.cluster.modify("ReplicaSet", Some(NS), "web-5bfcff88dc", |rs| {
        rs["metadata"]["annotations"]["deployment.kubernetes.io/revision"] = "54".into();
    });
    k.advance(500); // the timeline re-polls every 500ms until the restored ReplicaSet is on top
    k.wait_until("54 current", |k| k.element("rollback.rev.54").is_some_and(|e| e.selected && e.label == "nginx:1.26-alpine"));

    // Roll forward to 1.27 (revision 53) via the UI.
    k.click("rollback.rev.53");
    k.click("rollback.rollback.53");
    assert!(k.wait_for_element("confirm.message").label.contains("to revision 53?"));
    k.click("confirm.ok");
    k.wait_toast("rolled back", |l| l == "Rolled back to revision 53");
    assert_eq!(web_image(&k), "nginx:1.27-alpine");
}
