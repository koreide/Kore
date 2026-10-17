//! YAML tab: read-only / editing, diff, reset, discard, apply, search.

use super::helpers::{DetailsExt, NS};
use crate::testing::Kore;
use gpui_kit::TestAppContext;
use kore_core::ResourceKind;
use serde_json::json;

fn yaml_text(k: &mut Kore) -> String {
    k.label("yaml.text").expect("yaml text")
}

/// Put the caret at the end of the first YAML line containing `needle`.
fn caret_to_end_of(k: &mut Kore, needle: &str) {
    let text = yaml_text(k);
    let line = text.lines().position(|l| l.contains(needle)).unwrap_or_else(|| panic!("no line with {needle:?} in\n{text}"));
    k.click("yaml.editor");
    k.wait_text_entry();
    k.keys("cmd-up");
    if line > 0 {
        k.keys(&vec!["down"; line].join(" "));
    }
    k.keys("end");
}

/// Scenario 20-details-yaml-apply: read-only by default (typing changes
/// nothing); Editing + an added line shows Diff / Reset / Apply and "Unsaved
/// changes"; Diff shows "1 additions, 0 deletions"; Reset restores; switching
/// back to Read-only discards edits; Apply asks "Apply YAML Changes", toasts,
/// returns to Read-only and the cluster has the change; ⌘F counts matches.
#[gpui_kit::test]
fn yaml_edit_diff_reset_discard_apply_and_search(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.cluster.apply(json!({"apiVersion": "v1", "kind": "ConfigMap",
        "metadata": {"name": "kore-yaml-edit", "namespace": NS, "managedFields": [{"manager": "kubectl", "operation": "Update"}]},
        "data": {"greeting": "hello"}}));
    k.open(ResourceKind::Configmaps, "kore-yaml-edit");
    k.keys("2");
    k.wait_label("yaml.toggle", "Read-only", |l| l == "Read-only");
    let original = k.wait_label("yaml.text", "loaded", |l| l.contains("greeting: hello"));
    for needle in ["apiVersion: v1", "kind: ConfigMap", "name: kore-yaml-edit"] {
        assert!(original.contains(needle), "yaml lacks {needle}");
    }
    assert!(!original.contains("managedFields"));
    assert!(k.element("yaml.copy").is_some());
    for id in ["yaml.diff", "yaml.reset", "yaml.apply", "yaml.unsaved"] {
        assert!(k.element(id).is_none(), "{id} shown while read-only");
    }

    // Read-only text cannot be edited.
    caret_to_end_of(&mut k, "greeting: hello");
    k.type_text("x");
    assert_eq!(yaml_text(&mut k), original);
    assert!(k.element("yaml.unsaved").is_none());

    // Edit -> diff -> close -> reset
    k.click("yaml.toggle");
    k.wait_label("yaml.toggle", "Editing", |l| l == "Editing");
    caret_to_end_of(&mut k, "greeting: hello");
    k.keys("enter");
    k.type_text("  edited: by-harness");
    k.wait_for_element("yaml.unsaved");
    assert!(yaml_text(&mut k).contains("greeting: hello\n  edited: by-harness\n"));
    for id in ["yaml.diff", "yaml.reset", "yaml.apply"] {
        assert!(k.element(id).is_some(), "{id} missing");
    }
    k.click("yaml.diff");
    k.wait_label("yaml.diff-summary", "1 addition", |l| l == "Diff: 1 additions, 0 deletions");
    k.click("yaml.close-diff");
    k.wait_gone("yaml.diff-summary");
    k.click("yaml.reset");
    k.wait_until("reset", |k| k.label("yaml.text").as_deref() == Some(original.as_str()));
    assert_eq!(k.label("yaml.toggle").as_deref(), Some("Editing"));
    for id in ["yaml.unsaved", "yaml.diff", "yaml.apply"] {
        assert!(k.element(id).is_none(), "{id} after reset");
    }

    // Back to Read-only discards edits.
    caret_to_end_of(&mut k, "greeting: hello");
    k.type_text("-discard-me");
    k.wait_label("yaml.text", "edited", |l| l.contains("greeting: hello-discard-me"));
    k.click("yaml.toggle");
    k.wait_label("yaml.toggle", "Read-only", |l| l == "Read-only");
    k.wait_label("yaml.text", "discarded", |l| l == original);

    // Apply
    k.click("yaml.toggle");
    k.wait_label("yaml.toggle", "Editing", |l| l == "Editing");
    caret_to_end_of(&mut k, "greeting: hello");
    k.keys("enter");
    k.type_text("  edited: by-harness");
    k.wait_for_element("yaml.unsaved");
    k.cluster.clear_requests();
    k.click("yaml.apply");
    assert_eq!(k.wait_for_element("confirm").label, "Apply YAML Changes");
    assert!(k.label("confirm.message").unwrap().contains("apply the modified YAML to configmap \"kore-yaml-edit\""));
    assert!(k.element("confirm.cancel").is_some());
    k.click("confirm.ok");
    k.wait_toast("applied", |l| l.contains("pplied successfully"));
    k.wait_label("yaml.toggle", "Read-only", |l| l == "Read-only");
    let cm = k.cluster.get("ConfigMap", Some(NS), "kore-yaml-edit").unwrap();
    assert_eq!(cm["data"]["edited"], "by-harness");
    assert_eq!(cm["data"]["greeting"], "hello");
    assert!(k.cluster.requests().iter().any(|r| r.method == "PATCH" && r.path.ends_with("/configmaps/kore-yaml-edit")));

    // Search
    k.open_search();
    k.type_text("kore");
    k.advance(250); // the YAML search is debounced by 200ms
    let n = yaml_text(&mut k).to_lowercase().matches("kore").count();
    k.wait_label("searchbar.count", "1/N", |l| l == format!("1/{n}"));
    k.cx.update(|window, cx| window.blur(cx));
    k.keys("escape");
    k.wait_until("table", |k| k.state()["view"] == "table");
}
