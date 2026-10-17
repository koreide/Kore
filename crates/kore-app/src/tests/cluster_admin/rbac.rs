//! RBAC Simulator (spec 03 §7) against the harness fixtures' RBAC objects:
//! Role kore-demo/pod-reader bound to kore-rbac/viewer-bot, ClusterRole
//! kore-deployer bound (ClusterRoleBinding deployer-bot-kore-deployer) to
//! kore-rbac/deployer-bot, alice@kore.dev and kore-devs.

use super::support::KoreExt;
use crate::testing::Kore;
use gpui_kit::TestAppContext;
use kore_core::ResourceKind;

/// Boot, select namespace kore-demo and open the RBAC Simulator from the sidebar.
fn open_rbac(cx: &mut TestAppContext) -> Kore {
    let mut k = Kore::boot(cx);
    k.show(ResourceKind::Pods, "kore-demo");
    k.open_view("rbac");
    k.wait_for_element("view.rbac");
    k
}

/// Open picker `which` (matrix / query / impersonation), type `query` and pick `label`.
fn pick(k: &mut Kore, which: &str, query: &str, label: &str) {
    k.click(&format!("rbac.picker.{which}"));
    k.wait_for_element(&format!("rbac.picker.{which}.panel"));
    k.type_text(query);
    let item = format!("rbac.picker.item.{label}");
    k.wait_for_element(&item);
    k.click(&item);
    k.wait_gone(&format!("rbac.picker.{which}.panel"));
    k.wait_label(&format!("rbac.picker.{which}"), &format!("is {label}"), |l| l == label);
}

/// Wait for the matrix, click cell `resource`/`verb` and return the popover's rule chain labels.
fn cell_chain(k: &mut Kore, resource: &str, verb: &str) -> Vec<String> {
    let cell = format!("rbac.cell.{resource}.{verb}");
    k.wait_for_element(&cell);
    k.click(&cell);
    k.wait_label("rbac.popover", &format!("{resource} / {verb}"), |l| l.starts_with(&format!("{resource} / {verb}:")));
    k.wait_for_element("rbac.popover.chain.0");
    let mut ids = k.ids_with_prefix("rbac.popover.chain.");
    ids.sort();
    ids.iter().map(|id| k.label(id).unwrap()).collect()
}

/// Scenario 30-cluster-rbac-matrix: the picker lists service accounts, users and
/// groups (filtered by the search); viewer-bot's matrix has the resource x verb
/// cells, and pods/get's popover shows RoleBinding viewer-bot-pod-reader ->
/// Role pod-reader -> pods, pods/log [get, list, watch].
#[gpui_kit::test]
fn matrix_picker_lists_identities_and_cell_popover_shows_rule_chain(cx: &mut TestAppContext) {
    let mut k = open_rbac(cx);
    for tab in ["tab.permission-matrix", "tab.role-browser", "tab.query", "tab.impersonation"] {
        k.wait_for_element(tab);
    }
    assert!(k.element("tab.permission-matrix").unwrap().selected);
    assert_eq!(k.label("rbac.picker.matrix").as_deref(), Some("Select identity..."));
    assert!(k.element("rbac.matrix").is_none(), "no matrix before an identity is selected");

    k.click("rbac.picker.matrix");
    k.wait_for_element("rbac.picker.matrix.panel");
    k.type_text("kore");
    // kube-system service accounts are filtered out; the kore-* identities stay.
    k.wait_gone("rbac.picker.item.kube-system/*");
    for item in ["kore-rbac/viewer-bot", "kore-rbac/deployer-bot", "alice@kore.dev", "kore-devs"] {
        k.wait_for_element(&format!("rbac.picker.item.{item}"));
    }
    k.keys("cmd-a");
    k.type_text("viewer");
    k.wait_gone("rbac.picker.item.alice@kore.dev");
    k.click("rbac.picker.item.kore-rbac/viewer-bot");
    k.wait_gone("rbac.picker.matrix.panel");

    k.wait_for_element("rbac.matrix");
    k.wait_for_element("rbac.copy-csv");
    for row in ["pods", "deployments", "networkpolicies"] {
        k.wait_for_element(&format!("rbac.row.{row}"));
    }
    for verb in ["get", "list", "watch", "create", "update", "patch", "delete"] {
        k.wait_for_element(&format!("rbac.cell.pods.{verb}"));
    }
    assert_eq!(k.label("rbac.cell.pods.get").as_deref(), Some("pods get: Allowed"));
    assert_eq!(k.label("rbac.cell.pods.delete").as_deref(), Some("pods delete: Denied"));
    assert_eq!(k.label("rbac.cell.deployments.get").as_deref(), Some("deployments get: Denied"));

    let chain = cell_chain(&mut k, "pods", "get");
    assert_eq!(
        chain,
        vec!["RoleBinding viewer-bot-pod-reader (ns: kore-demo) -> Role pod-reader (ns: kore-demo) -> pods, pods/log [get, list, watch]"]
    );
    assert_eq!(k.label("rbac.popover").as_deref(), Some("pods / get: allowed"));
    k.keys("escape");
    k.wait_gone("rbac.popover");
    assert_eq!(k.state()["view"], "rbac", "Esc closes the popover, not the view");
}

/// Scenario 30-cluster-rbac-identities: a User and a Group bound cluster-wide get
/// their rule chain through ClusterRoleBinding deployer-bot-kore-deployer ->
/// ClusterRole kore-deployer -> deployments [get, list, watch, create, update, patch].
#[gpui_kit::test]
fn user_and_group_identities_show_cluster_role_binding_chain(cx: &mut TestAppContext) {
    let mut k = open_rbac(cx);
    let expected = "ClusterRoleBinding deployer-bot-kore-deployer -> ClusterRole kore-deployer -> deployments [get, list, watch, create, update, patch]";

    pick(&mut k, "matrix", "alice", "alice@kore.dev");
    k.wait_for_element("rbac.copy-csv");
    assert_eq!(cell_chain(&mut k, "deployments", "create"), vec![expected]);
    k.keys("escape");
    k.wait_gone("rbac.popover");

    pick(&mut k, "matrix", "devs", "kore-devs");
    // The matrix is rebuilt for the group (cell labels stay; wait for the new popover chain).
    assert_eq!(cell_chain(&mut k, "deployments", "create"), vec![expected]);
    k.keys("escape");
    k.wait_gone("rbac.popover");

    pick(&mut k, "matrix", "deployer", "kore-rbac/deployer-bot");
    k.wait_for_element("rbac.cell.deployments.create");
}

/// Scenario 30-cluster-rbac-roles: the Role Browser filters roles; ClusterRole
/// kore-deployer has 2 rules and 3 subjects bound cluster-wide; Role pod-reader
/// (ns kore-demo) has 1 rule and viewer-bot; clicking a subject opens its matrix.
#[gpui_kit::test]
fn role_browser_reverse_lookup_lists_rules_and_subjects(cx: &mut TestAppContext) {
    let mut k = open_rbac(cx);
    k.keys("2");
    k.wait_until("role browser tab", |k| k.element("tab.role-browser").is_some_and(|e| e.selected));
    k.wait_for_element("rbac.roles.filter");
    k.wait_for_element("rbac.role.ClusterRole//kore-deployer");

    k.click("rbac.roles.filter");
    k.type_text("deployer");
    k.wait_gone("rbac.role.Role/kore-demo/pod-reader");
    k.click("rbac.role.ClusterRole//kore-deployer");
    k.wait_label("rbac.roles.detail", "ClusterRole/kore-deployer", |l| l == "ClusterRole/kore-deployer");
    let mut rules = k.ids_with_prefix("rbac.roles.rule.");
    rules.sort();
    let rules: Vec<String> = rules.iter().map(|id| k.label(id).unwrap()).collect();
    assert_eq!(rules.len(), 2, "RULES (2): {rules:?}");
    assert!(rules[0].starts_with("deployments ["), "{rules:?}");
    assert!(rules[1].starts_with("services, configmaps"), "{rules:?}");
    let subjects = ["kore-rbac/deployer-bot", "alice@kore.dev", "kore-devs"];
    let found: Vec<String> = k.ids_with_prefix("rbac.subject.").into_iter().filter(|id| !id.ends_with(".via")).collect();
    assert_eq!(found.len(), 3, "SUBJECTS (3): {found:?}");
    for s in subjects {
        assert_eq!(
            k.label(&format!("rbac.subject.{s}.via")).as_deref(),
            Some("via ClusterRoleBinding \"deployer-bot-kore-deployer\" (cluster-wide)"),
            "{s}"
        );
    }

    k.click("rbac.roles.filter");
    k.keys("cmd-a");
    k.type_text("reader");
    k.click("rbac.role.Role/kore-demo/pod-reader");
    k.wait_label("rbac.roles.detail", "Role/pod-reader", |l| l == "Role/pod-reader (ns: kore-demo)");
    assert_eq!(k.ids_with_prefix("rbac.roles.rule.").len(), 1, "RULES (1)");
    assert_eq!(k.label("rbac.roles.rule.0").as_deref(), Some("pods, pods/log [get, list, watch]"));
    assert_eq!(k.ids_with_prefix("rbac.subject.").into_iter().filter(|id| !id.ends_with(".via")).count(), 1, "SUBJECTS (1)");
    assert_eq!(
        k.label("rbac.subject.kore-rbac/viewer-bot.via").as_deref(),
        Some("via RoleBinding \"viewer-bot-pod-reader\" (namespace: kore-demo)")
    );

    // A subject jumps to its matrix.
    k.click("rbac.subject.kore-rbac/viewer-bot");
    k.wait_until("matrix tab", |k| k.element("tab.permission-matrix").is_some_and(|e| e.selected));
    k.wait_label("rbac.picker.matrix", "viewer-bot", |l| l == "kore-rbac/viewer-bot");
    k.wait_for_element("rbac.copy-csv");
}

/// Scenario 30-cluster-rbac-query: Check Permission for viewer-bot get pods in
/// kore-demo is Allowed with reason and rule chain; Who Can? lists viewer-bot via pod-reader.
#[gpui_kit::test]
fn query_tab_checks_permission_and_who_can(cx: &mut TestAppContext) {
    let mut k = open_rbac(cx);
    k.keys("3");
    k.wait_for_element("rbac.query.mode.check");
    k.wait_for_element("rbac.query.mode.whocan");
    assert_eq!(k.label("rbac.query.namespace").as_deref(), Some("ns: kore-demo"));
    assert_eq!(k.label("rbac.select.verb").as_deref(), Some("get"));
    assert_eq!(k.label("rbac.select.resource").as_deref(), Some("pods"));
    assert!(k.element("rbac.query.check").unwrap().disabled, "Check needs an identity");

    pick(&mut k, "query", "viewer", "kore-rbac/viewer-bot");
    k.wait_until("check enabled", |k| k.element("rbac.query.check").is_some_and(|e| !e.disabled));
    k.click("rbac.query.check");
    k.wait_label("rbac.query.result", "Allowed", |l| l == "Allowed");
    assert_eq!(
        k.label("rbac.query.summary").as_deref(),
        Some("Allowed because of Role 'pod-reader' via RoleBinding 'viewer-bot-pod-reader'")
    );
    assert_eq!(
        k.label("rbac.query.chain.0").as_deref(),
        Some("RoleBinding viewer-bot-pod-reader (ns: kore-demo) -> Role pod-reader (ns: kore-demo) -> pods, pods/log [get, list, watch]")
    );

    k.click("rbac.query.mode.whocan");
    k.wait_gone("rbac.picker.query");
    k.click("rbac.query.check");
    k.wait_label("rbac.whocan.title", "identities can get pods in kore-demo", |l| l.ends_with("identities can get pods in kore-demo:"));
    k.wait_for_element("rbac.whocan.kore-rbac/viewer-bot");
    assert_eq!(k.label("rbac.whocan.kore-rbac/viewer-bot.via").as_deref(), Some("via pod-reader"));
}

/// Scenario 30-cluster-rbac-impersonation: the Impersonation tab picks an identity;
/// "Enter Impersonation Mode" shows the global banner on every view (also the
/// Dashboard) and its Exit removes it. Impersonation is a simulation (spec 03
/// §7.6, banner text): API calls keep the real credentials, so no request
/// carries Impersonate-* headers.
#[gpui_kit::test]
fn impersonation_banner_follows_views_until_exit(cx: &mut TestAppContext) {
    let mut k = open_rbac(cx);
    k.keys("4");
    k.wait_until("impersonation tab", |k| k.element("tab.impersonation").is_some_and(|e| e.selected));
    k.wait_for_element("rbac.picker.impersonation");
    assert!(k.element("rbac.impersonation.enter").is_none(), "no Enter button before an identity is picked");

    pick(&mut k, "impersonation", "alice", "alice@kore.dev");
    k.cluster.clear_requests();
    k.click("rbac.impersonation.enter");
    k.wait_for_element("banner.impersonation");
    k.wait_for_element("rbac.impersonation.exit");
    let imp = k.state()["impersonation"].clone();
    assert_eq!(imp["label"], "alice@kore.dev", "{imp}");

    k.open_view("dashboard");
    k.wait_for_element("view.dashboard");
    k.wait_for_element("dashboard.card.*");
    k.wait_for_element("banner.impersonation");
    assert_eq!(k.state()["impersonation"]["label"], "alice@kore.dev");

    // Simulated: the dashboard's API calls use the real identity.
    let reqs = k.cluster.requests();
    assert!(!reqs.is_empty(), "the dashboard loaded from the cluster");
    for r in &reqs {
        assert!(!r.headers.keys().any(|h| h.to_ascii_lowercase().starts_with("impersonate-")), "{} {} {:?}", r.method, r.path, r.headers);
    }

    k.click("banner.impersonation.exit");
    k.wait_gone("banner.impersonation");
    assert!(k.state()["impersonation"].is_null());
}

/// Scenario 30-cluster-bug-rbac-picker-esc (quirk 10.22, Fix 4): Esc closes the
/// open identity picker, also while its search field has focus, without leaving
/// the RBAC view; reopening starts with an empty search.
#[gpui_kit::test]
fn esc_closes_identity_picker_without_leaving_rbac(cx: &mut TestAppContext) {
    let mut k = open_rbac(cx);
    k.click("rbac.picker.matrix");
    k.wait_for_element("rbac.picker.matrix.panel");
    k.type_text("viewer");
    k.wait_gone("rbac.picker.item.kore-rbac/deployer-bot");
    k.wait_until("picker modal registered", |k| k.state()["modals"].as_array().is_some_and(|m| !m.is_empty()));
    k.keys("escape");
    k.wait_gone("rbac.picker.matrix.panel");
    k.wait_until("modal cleared", |k| k.state()["modals"] == serde_json::json!([]));
    assert_eq!(k.state()["view"], "rbac");
    assert_eq!(k.label("rbac.picker.matrix").as_deref(), Some("Select identity..."));

    // Reopen: the query was cleared, so every identity is listed again.
    k.click("rbac.picker.matrix");
    k.wait_for_element("rbac.picker.matrix.panel");
    k.wait_for_element("rbac.picker.item.kore-rbac/deployer-bot");
    k.wait_for_element("rbac.picker.item.kore-rbac/viewer-bot");
    k.keys("escape");
    k.wait_gone("rbac.picker.matrix.panel");
    assert_eq!(k.state()["view"], "rbac");
}
