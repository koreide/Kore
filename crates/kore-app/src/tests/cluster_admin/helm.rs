//! Helm releases and release detail (spec 03 §5) with the fake `helm` of
//! `kore_testkit::fixtures::with_helm` (release kore-demo, 8 revisions, no notes).

use super::support::KoreExt;
use crate::testing::Kore;
use gpui_kit::TestAppContext;
use kore_core::ResourceKind;
use kore_testkit::fixtures;

fn boot_helm(cx: &mut TestAppContext, customize: impl FnOnce(kore_testkit::FakeEnv) -> kore_testkit::FakeEnv) -> Kore {
    let (env, c1, c2) = fixtures::harness_env();
    let env = customize(fixtures::with_helm(env));
    let mut k = Kore::boot_env(cx, env, c1, c2);
    k.show(ResourceKind::Pods, "kore-demo");
    k.open_view("helm");
    k
}

fn helm_dir(k: &Kore) -> std::path::PathBuf {
    k.env.dir.path().join("helm")
}

/// `true` when `s` looks like "Oct 16, 07:02 PM" (the formatted Helm date).
fn is_short_date(s: &str) -> bool {
    let b = s.as_bytes();
    let parts: Vec<&str> = s.split(' ').collect();
    parts.len() == 4
        && parts[0].len() == 3
        && b[0].is_ascii_uppercase()
        && parts[1].ends_with(',')
        && parts[1].trim_end_matches(',').parse::<u32>().is_ok()
        && parts[2].len() == 5
        && parts[2].as_bytes()[2] == b':'
        && (parts[3] == "AM" || parts[3] == "PM")
}

/// Scenario 30-cluster-helm: the list shows release kore-demo (namespace,
/// revision, deployed, chart, app version) with sortable headers; Down + Enter
/// opens the detail, whose Values, Manifest and Notes tabs show `helm get`
/// output (Notes: "No notes available for this release.", not the values);
/// Esc returns to the list.
#[gpui_kit::test]
fn helm_list_keyboard_opens_detail_with_values_manifest_notes(cx: &mut TestAppContext) {
    let mut k = boot_helm(cx, |e| e);
    k.wait_for_element("helm.row.kore-demo/kore-demo");
    for h in ["name", "namespace", "revision", "status", "chart", "app_version", "updated"] {
        k.wait_for_element(&format!("helm.header.{h}"));
    }
    let cells = k.label("helm.cells.kore-demo/kore-demo").unwrap();
    let parts: Vec<&str> = cells.split(" | ").collect();
    assert_eq!(&parts[..6], &["kore-demo", "kore-demo", "8", "deployed", "kore-demo-0.1.0", "1.0"], "{cells}");
    // Listed in the selected namespace and context.
    let calls = k.env.tool_calls();
    assert!(
        calls.iter().any(|c| c.starts_with("helm list --output json --namespace kore-demo --kube-context kind-kore-harness")),
        "{calls:?}"
    );

    // Sortable headers: REVISION then NAME (ascending).
    k.click("helm.header.revision");
    k.wait_until("sorted by revision", |k| k.element("helm.header.revision").is_some_and(|e| e.selected));
    k.click("helm.header.name");
    k.wait_until("sorted by name", |k| k.element("helm.header.name").is_some_and(|e| e.selected));
    assert!(!k.element("helm.header.revision").unwrap().selected);

    k.keys("down");
    k.wait_until("row selected", |k| k.element("helm.row.kore-demo/kore-demo").is_some_and(|e| e.selected));
    k.keys("enter");
    k.wait_until("helm detail", |k| k.state()["view"] == "helm-detail");
    for t in ["tab.values", "tab.manifest", "tab.history", "tab.notes"] {
        k.wait_for_element(t);
    }
    k.wait_for_element("helm-detail.back");
    k.wait_for_element("helm-detail.copy");
    let values = k.wait_label("helm-detail.values", "values", |l| !l.is_empty());
    assert!(values.contains("message: hello-from-helm-v2"), "{values}");
    assert!(values.contains("replicaCount: 1"), "{values}");

    k.click("tab.manifest");
    let manifest = k.wait_label("helm-detail.manifest", "manifest", |l| l.contains("# Source: kore-demo/templates/"));
    assert!(manifest.contains("kind: ConfigMap") && manifest.contains("kind: Deployment"), "{manifest}");

    k.click("tab.notes");
    let notes = k.wait_label("helm-detail.notes", "notes", |l| !l.is_empty());
    assert_eq!(notes, "No notes available for this release.");
    assert!(k.env.tool_calls().iter().any(|c| c.starts_with("helm get notes kore-demo --namespace kore-demo")));

    k.keys("escape");
    k.wait_until("back to the list", |k| k.state()["view"] == "helm");
    k.wait_for_element("helm.row.kore-demo/kore-demo");
    k.wait_for_element("helm.header.app_version");
}

/// Scenario 30-cluster-helm-history-fix (Fix 8): History parses the integer
/// revisions (superseded + deployed, "latest" on the last), not "No revision
/// history found"; Rollback on revision 1 confirms "to revision 1" and runs
/// `helm rollback kore-demo 1`, then the history is refetched.
#[gpui_kit::test]
fn helm_history_lists_revisions_and_rolls_back(cx: &mut TestAppContext) {
    let mut k = boot_helm(cx, |e| e);
    k.click("helm.row.kore-demo/kore-demo");
    k.wait_until("helm detail", |k| k.state()["view"] == "helm-detail");
    k.click("tab.history");
    k.wait_for_element("helm-detail.history");
    assert!(k.element("helm-detail.history.empty").is_none(), "No revision history found");
    assert_eq!(k.ids_with_prefix("helm-detail.row.").len(), 8);
    assert_eq!(k.label("helm-detail.row.1").as_deref(), Some("superseded Install complete"));
    assert_eq!(k.label("helm-detail.row.2").as_deref(), Some("superseded Upgrade complete"));
    assert_eq!(k.label("helm-detail.row.3").as_deref(), Some("superseded Rollback to 1"));
    assert_eq!(k.label("helm-detail.row.8").as_deref(), Some("deployed Upgrade complete"));
    assert_eq!(k.label("helm-detail.latest").as_deref(), Some("latest 8"));
    // Rollback only on superseded revisions.
    k.wait_for_element("helm-detail.rollback.1");
    assert!(k.element("helm-detail.rollback.8").is_none());

    // The rolled-back history (helm would add revision 9 "Rollback to 1").
    let history = helm_dir(&k).join("history.json");
    let mut revs: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(&history).unwrap()).unwrap();
    revs[7]["status"] = "superseded".into();
    revs.push(serde_json::json!({"revision": 9, "updated": "2026-10-17T10:00:00.000000+03:00", "status": "deployed",
        "chart": "kore-demo-0.1.0", "app_version": "1.0", "description": "Rollback to 1"}));

    k.click("helm-detail.rollback.1");
    k.wait_label("confirm", "Rollback Helm Release", |l| l == "Rollback Helm Release");
    let msg = k.confirm_message().unwrap();
    assert!(msg.contains("to revision 1?"), "{msg}");
    assert!(msg.contains("\"kore-demo\" in namespace \"kore-demo\""), "{msg}");
    std::fs::write(&history, serde_json::to_string(&revs).unwrap()).unwrap();
    k.click("confirm.ok");
    k.wait_gone("confirm");
    k.wait_until("rollback toast", |k| k.toasts().iter().any(|t| t == "Rolled back to revision 1"));
    let calls = k.env.tool_calls();
    assert!(
        calls.iter().any(|c| c == "helm rollback kore-demo 1 --namespace kore-demo --kube-context kind-kore-harness"),
        "{calls:?}"
    );
    k.wait_label("helm-detail.row.9", "refetched history", |l| l == "deployed Rollback to 1");
    assert_eq!(k.label("helm-detail.latest").as_deref(), Some("latest 9"));
}

/// A fake `helm` like `with_helm`'s, plus a second release kore-bug-notes whose
/// `helm get notes` answers `notes-<release>.txt` when present.
fn two_release_helm(env: kore_testkit::FakeEnv) -> kore_testkit::FakeEnv {
    let d = env.dir.path().join("helm");
    let list = r#"[{"name":"kore-demo","namespace":"kore-demo","revision":"8","updated":"2026-10-16 23:32:14.185083 +0300 IDT","status":"deployed","chart":"kore-demo-0.1.0","app_version":"1.0"},{"name":"kore-bug-notes","namespace":"kore-demo","revision":"1","updated":"2026-10-16 19:02:43.123456 +0300 IDT","status":"deployed","chart":"kore-bug-notes-0.1.0","app_version":"9.9"}]"#;
    std::fs::write(d.join("list.json"), list).unwrap();
    // `helm get notes` prints a "NOTES:" header before the rendered NOTES.txt.
    std::fs::write(d.join("notes-kore-bug-notes.txt"), "NOTES:\nKORE-HELM-NOTES for kore-bug-notes\nSecond notes line\n").unwrap();
    let d = d.display();
    env.tool(
        "helm",
        &format!(
            r#"case "$1 $2" in
  "version "*) cat '{d}/version.txt' ;;
  "list "*) cat '{d}/list.json' ;;
  "history "*) cat '{d}/history.json' ;;
  "get values") cat '{d}/values.yaml' ;;
  "get manifest") cat '{d}/manifest.yaml' ;;
  "get notes") if [ -f '{d}/notes-'"$3"'.txt' ]; then cat '{d}/notes-'"$3"'.txt'; else cat '{d}/notes.txt'; fi ;;
  "rollback "*|"uninstall "*|"upgrade "*) exit 0 ;;
  *) echo "fake helm: unsupported $*" >&2; exit 1 ;;
esac"#
        ),
    )
}

/// Scenario 30-cluster-bug-helm-notes-date (quirks 10.8 / 10.9): the list
/// formats Helm's own "updated" timestamp (Go time string) like the history
/// ("Oct 16, 07:02 PM"); Notes shows `helm get notes` without its "NOTES:"
/// header (not the values), and "No notes available" for a release without notes.
#[gpui_kit::test]
fn helm_dates_are_formatted_and_notes_tab_shows_release_notes(cx: &mut TestAppContext) {
    let mut k = boot_helm(cx, two_release_helm);
    k.wait_for_element("helm.row.kore-demo/kore-bug-notes");
    for r in ["kore-demo", "kore-bug-notes"] {
        let d = k.label(&format!("helm.updated.kore-demo/{r}")).unwrap();
        assert!(is_short_date(&d), "{r}: {d:?}");
        assert!(!d.contains("IDT") && !d.contains("+0300"), "{r}: raw Go timestamp {d:?}");
    }

    k.click("helm.row.kore-demo/kore-bug-notes");
    k.wait_until("helm detail", |k| k.state()["view"] == "helm-detail");
    k.wait_for_element("helm-detail.values");
    k.click("tab.notes");
    let notes = k.wait_label("helm-detail.notes", "notes", |l| l.contains("KORE-HELM-NOTES"));
    assert_eq!(notes, "KORE-HELM-NOTES for kore-bug-notes\nSecond notes line");
    assert!(!notes.contains("COMPUTED VALUES") && !notes.contains("NOTES:"), "{notes}");

    k.keys("escape");
    k.wait_for_element("helm.row.kore-demo/kore-demo");
    k.click("helm.row.kore-demo/kore-demo");
    k.wait_until("helm detail", |k| k.state()["view"] == "helm-detail");
    k.wait_for_element("helm-detail.values");
    k.click("tab.notes");
    let notes = k.wait_label("helm-detail.notes", "notes", |l| !l.is_empty());
    assert_eq!(notes, "No notes available for this release.");

    k.click("tab.history");
    k.wait_label("helm-detail.row.1", "history", |l| l == "superseded Install complete");
    for rev in 1..=8 {
        let d = k.label(&format!("helm-detail.updated.{rev}")).unwrap();
        assert!(is_short_date(&d), "revision {rev}: {d:?}");
    }
    k.keys("escape");
    k.wait_until("back to the list", |k| k.state()["view"] == "helm");
}
