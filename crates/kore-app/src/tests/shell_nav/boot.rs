//! Boot: connection setup, home view, initial context (spec 01 §1, §12, §21.1-4).

use super::helpers::{boot_with_config, ShellExt};
use crate::testing::Kore;
use gpui_kit::TestAppContext;
use kore_testkit::fixtures::{self, CONTEXT, CONTEXT_2};
use kore_testkit::{FakeCluster, FakeEnv};

/// The broken-startup world: `kore-broken` (current) and `kore-unreachable` fail to
/// connect, `kore-hang` never answers, and the two harness contexts work.
fn broken_env() -> (FakeEnv, FakeCluster, FakeCluster) {
    let (c1, c2) = (fixtures::harness_cluster(), fixtures::harness_cluster_2());
    let hang = FakeCluster::new();
    hang.set_hang(true);
    let env = FakeEnv::new()
        .broken_context("kore-broken", "client certificate kore-broken.crt not found")
        .broken_context("kore-unreachable", "tcp connect error: Connection refused")
        .context("kore-hang", &hang)
        .context(CONTEXT, &c1)
        .context(CONTEXT_2, &c2)
        .current("kore-broken");
    (env, c1, c2)
}

/// Scenario 10-shell-boot-connection-error: a broken startup context shows
/// ConnectionSetup with the error, the "Try a different context" picker (the
/// startup context tagged "current"), technical details, and retrying another
/// context; picking kind-kore-harness connects and the main UI renders.
#[gpui_kit::test]
fn broken_startup_context_shows_setup_and_picker_connects(cx: &mut TestAppContext) {
    let (env, c1, c2) = broken_env();
    let mut k = Kore::boot_env(cx, env, c1, c2);
    assert_eq!(k.state()["connected"], false);
    // A connector error (not a kube API error) is the generic "Connection Error".
    k.wait_label("connection", "is Connection Error", |l| l == "Connection Error");
    k.wait_label("connection.description", "names the error", |l| l.contains("kore-broken.crt not found"));
    k.wait_for_element("connection.picker");
    for ctx in ["kore-broken", "kore-unreachable", "kore-hang", CONTEXT, CONTEXT_2] {
        k.wait_for_element(&format!("connection.context.{ctx}"));
    }
    assert_eq!(k.ids_in_order("connection.context.").iter().filter(|id| id.ends_with(".current")).count(), 1);
    k.wait_for_element("connection.context.kore-broken.current");
    k.wait_label("connection.retry", "Retry Connection", |l| l == "Retry Connection");
    // The main UI is not shown.
    assert!(k.element("sidebar.context.trigger").is_none());

    // Technical details reveal the raw error.
    assert!(k.element("connection.error").is_none());
    k.click("connection.details");
    k.wait_label("connection.error", "raw error", |l| l.contains("kore-broken.crt not found"));

    // Pick another (failing) context and retry: its error, and it is now "current".
    k.click("connection.context.kore-unreachable");
    assert!(k.element("connection.context.kore-unreachable").unwrap().selected);
    k.wait_label("connection.retry", "Connect to kore-unreachable", |l| l == "Connect to kore-unreachable");
    k.click("connection.retry");
    k.wait_label("connection.description", "new error", |l| l.contains("Connection refused"));
    k.wait_for_element("connection.context.kore-unreachable.current");
    assert_eq!(k.state()["connected"], false);

    // Pick the harness cluster: connects and the main UI renders.
    k.click(&format!("connection.context.{CONTEXT}"));
    k.wait_label("connection.retry", "Connect to kind-kore-harness", |l| l == format!("Connect to {CONTEXT}"));
    k.click("connection.retry");
    k.wait_until("connected", |k| k.state()["connected"] == true);
    k.wait_gone("connection");
    k.wait_until("context", |k| k.state()["context"] == CONTEXT);
    k.wait_label("sidebar.context.trigger", "kind-kore-harness", |l| l == CONTEXT);
    k.wait_for_element("sidebar.multi-cluster");
    k.wait_for_element("sidebar.view.chat");
}

/// Scenario 10-shell-boot-connection-error: while a retry is in flight the button
/// reads "Connecting..." and is disabled (a hanging cluster keeps it there).
#[gpui_kit::test]
fn retry_against_hanging_cluster_shows_connecting(cx: &mut TestAppContext) {
    let (env, c1, c2) = broken_env();
    let mut k = Kore::boot_env(cx, env, c1, c2);
    k.wait_for_element("connection.context.kore-hang");
    k.click("connection.context.kore-hang");
    k.click("connection.retry");
    let retry = k.wait_label("connection.retry", "Connecting...", |l| l == "Connecting...");
    assert_eq!(retry, "Connecting...");
    assert!(k.element("connection.retry").unwrap().disabled);
    // Clicking another context while retrying is ignored.
    k.click(&format!("connection.context.{CONTEXT}"));
    assert!(!k.element(&format!("connection.context.{CONTEXT}")).unwrap().selected);
}

/// Scenario 10-shell-boot-home: a fresh launch lands on AI Chat (default view)
/// with the full sidebar and no resource header.
#[gpui_kit::test]
fn fresh_launch_lands_on_ai_chat_with_full_sidebar(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.wait_until("context", |k| k.state()["context"] == CONTEXT);
    assert_eq!(k.view(), "chat");
    k.wait_label("sidebar.context.trigger", "kind-kore-harness", |l| l == CONTEXT);
    k.wait_label("sidebar.namespace.trigger", "default", |l| l == "default");
    k.wait_for_element("sidebar.multi-cluster");
    let views: Vec<String> = k.labels_in_order("sidebar.view.");
    assert_eq!(views, ["AI Chat", "Dashboard", "Graph", "Network Policies", "RBAC", "Custom Resources", "Helm"]);
    assert!(k.element("sidebar.view.chat").unwrap().selected);
    for kind in ["pods", "deployments", "cronjobs", "services", "configmaps", "nodes"] {
        k.wait_for_element(&format!("sidebar.kind.{kind}"));
    }
    k.wait_for_element("sidebar.settings");
    // AI Chat view, no table header / search box.
    k.wait_for_element("view.chat");
    k.wait_for_element("chat.input");
    assert!(k.element("header.search").is_none());
    assert!(k.element("table.header.restarts").is_none());
    assert!(k.element("table").is_none());
}

/// Scenario 10-shell-boot-no-kubeconfig: a kubeconfig path that doesn't exist shows
/// "No Kubeconfig Found" (steps, Retry, "Checking: <path>") instead of the app;
/// technical details show the raw error; Retry re-checks (and connects once the file exists).
#[gpui_kit::test]
fn missing_kubeconfig_shows_no_kubeconfig_setup(cx: &mut TestAppContext) {
    let (env, _, _) = fixtures::harness_env();
    let mut config = env.core_config();
    let valid = env.kubeconfig_path();
    let missing = env.dir.path().join("nonexistent").join("kubeconfig");
    config.kubeconfig_paths = vec![missing.clone()];
    let mut k = boot_with_config(cx, env, config);

    k.wait_label("connection", "No Kubeconfig Found", |l| l == "No Kubeconfig Found");
    assert_eq!(k.state()["connected"], false);
    k.wait_label("connection.description", "couldn't find", |l| l.starts_with("Kore couldn't find a kubeconfig file"));
    let steps = k.labels_in_order("connection.step.");
    assert_eq!(steps[0], "Ensure you have a Kubernetes cluster configured");
    assert_eq!(steps[1], "Check that ~/.kube/config exists and is readable");
    assert!(steps[2].starts_with("Or set the KUBECONFIG environment variable"), "{steps:?}");
    k.wait_label("connection.retry", "Retry Connection", |l| l == "Retry Connection");
    let checking = format!("Checking: {}", missing.display());
    k.wait_label("connection.kubeconfig", "Checking: <path>", |l| l == checking);
    // No sidebar, no context picker (there are no contexts).
    assert!(k.element("connection.picker").is_none());
    assert!(k.element("sidebar.multi-cluster").is_none());

    k.click("connection.details");
    k.wait_label("connection.error", "raw error", |l| l.contains("Kubeconfig not found or unreadable"));

    // Retry: still missing.
    k.click("connection.retry");
    k.wait_label("connection.retry", "retry finished", |l| l == "Retry Connection");
    k.wait_label("connection", "still No Kubeconfig Found", |l| l == "No Kubeconfig Found");

    // Once the file exists, Retry connects.
    std::fs::create_dir_all(missing.parent().unwrap()).unwrap();
    std::fs::copy(valid, &missing).unwrap();
    k.click("connection.retry");
    k.wait_until("connected", |k| k.state()["connected"] == true);
    k.wait_label("sidebar.context.trigger", "kind-kore-harness", |l| l == CONTEXT);
}

/// Scenario 10-shell-bug-initial-context: with no favorite contexts the app starts in
/// kubeconfig's current-context, not the first listed one.
#[gpui_kit::test]
fn starts_in_kubeconfig_current_context_not_first(cx: &mut TestAppContext) {
    let (c1, c2) = (fixtures::harness_cluster(), fixtures::harness_cluster_2());
    let env = FakeEnv::new().context(CONTEXT, &c1).context(CONTEXT_2, &c2).current(CONTEXT_2);
    let mut k = Kore::boot_env(cx, env, c1, c2);
    k.wait_until("context 2", |k| k.state()["context"] == CONTEXT_2);
    assert_eq!(k.state()["contexts"][0], CONTEXT);
    k.wait_label("sidebar.context.trigger", "kind-kore-harness-2", |l| l == CONTEXT_2);
    // And it stays there (no late switch to the first context).
    for _ in 0..20 {
        k.frame();
    }
    assert_eq!(k.state()["context"], CONTEXT_2);
}
