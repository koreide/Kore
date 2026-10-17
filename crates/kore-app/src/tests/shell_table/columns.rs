//! Kind-specific columns and cell values (spec 01 §7.5, §7.8).

use super::helpers::*;
use gpui_kit::TestAppContext;
use serde_json::json;

/// Scenario 10-shell-table-cluster-kinds: Nodes, Namespaces and Events show
/// their own columns, breadcrumb and seeded rows.
#[gpui_kit::test]
fn cluster_kinds_show_their_columns_and_rows(cx: &mut TestAppContext) {
    let mut k = pods_kore_demo(cx);

    k.click_sidebar("sidebar.kind.nodes");
    k.wait_label("header.breadcrumb", "Nodes", |l| l == "Nodes");
    k.wait_headers(&["NAME", "STATUS", "ROLES", "AGE", "VERSION"]);
    let node = "/kore-harness-control-plane";
    assert_eq!(k.cell(node, "status"), "Ready");
    assert_eq!(k.cell(node, "roles"), "control-plane");
    assert!(k.cell(node, "version").starts_with("v1."), "version {}", k.cell(node, "version"));

    k.click_sidebar("sidebar.kind.namespaces");
    k.wait_label("header.breadcrumb", "Namespaces", |l| l == "Namespaces");
    k.wait_for_element("table.row./kore-rbac");
    k.wait_headers(&["NAME", "STATUS", "AGE"]);
    for ns in ["default", "kore-demo", "kube-system"] {
        assert_eq!(k.cell(&format!("/{ns}"), "status"), "Active", "namespace {ns}");
    }

    k.click_sidebar("sidebar.kind.events");
    k.wait_label("header.breadcrumb", "Events", |l| l == "Events");
    k.wait_headers(&["LAST SEEN", "TYPE", "REASON", "OBJECT", "MESSAGE", "COUNT"]);
    k.wait_until("crashloop event", |k| k.column("involvedObject").iter().any(|o| o == "Pod/crashloop"));
    let warnings: Vec<(String, String)> = k.column("eventType").into_iter().zip(k.column("reason")).collect();
    assert!(warnings.contains(&("Warning".into(), "BackOff".into())), "events {warnings:?}");
}

/// Scenario 10-shell-table-network-config: Services, Ingresses, ConfigMaps and
/// Secrets columns and seeded rows.
#[gpui_kit::test]
fn network_and_config_kinds_show_their_columns_and_rows(cx: &mut TestAppContext) {
    let mut k = pods_kore_demo(cx);

    k.click_sidebar("sidebar.kind.services");
    k.wait_label("header.breadcrumb", "Services", |l| l == "Services");
    k.wait_headers(&["NAMESPACE", "NAME", "TYPE", "CLUSTER-IP", "EXTERNAL-IP", "PORT(S)", "AGE"]);
    let web = "kore-demo/web";
    assert_eq!(k.cell(web, "type"), "ClusterIP");
    assert_eq!(k.cell(web, "externalIp"), "<none>");
    assert_eq!(k.cell(web, "ports"), "80/TCP");
    assert!(k.cell(web, "clusterIp").starts_with("10.96."));

    k.click_sidebar("sidebar.kind.ingresses");
    k.wait_label("header.breadcrumb", "Ingresses", |l| l == "Ingresses");
    k.wait_headers(&["NAMESPACE", "NAME", "CLASS", "HOSTS", "PORTS", "AGE"]);
    assert_eq!(k.cell(web, "hosts"), "web.kore.local");
    assert_eq!(k.cell(web, "ingressPorts"), "80");

    k.click_sidebar("sidebar.kind.configmaps");
    k.wait_label("header.breadcrumb", "Configmaps", |l| l == "Configmaps");
    k.wait_headers(&["NAMESPACE", "NAME", "DATA KEYS", "AGE"]);
    k.wait_for_element("table.row.kore-demo/web-config");
    k.wait_for_element("table.row.kore-demo/kube-root-ca.crt");
    assert_eq!(k.cell("kore-demo/web-config", "dataKeys"), "2");

    k.click_sidebar("sidebar.kind.secrets");
    k.wait_label("header.breadcrumb", "Secrets", |l| l == "Secrets");
    k.wait_headers(&["NAMESPACE", "NAME", "TYPE", "DATA KEYS", "AGE"]);
    assert_eq!(k.cell("kore-demo/web-secret", "secretType"), "Opaque");
}

/// Scenario 10-shell-table-workloads: Deployments, Jobs and CronJobs columns,
/// breadcrumb labels and seeded rows.
#[gpui_kit::test]
fn workload_kinds_show_their_columns_and_rows(cx: &mut TestAppContext) {
    let mut k = pods_kore_demo(cx);

    k.click_sidebar("sidebar.kind.deployments");
    k.wait_label("header.breadcrumb", "Deployments", |l| l == "Deployments");
    k.wait_headers(&["NAMESPACE", "NAME", "READY", "UP-TO-DATE", "AVAILABLE", "AGE"]);
    k.wait_for_element("table.row.kore-demo/logger");
    assert_eq!(k.cell("kore-demo/logger", "ready"), "2/2");
    assert_eq!(k.cell("kore-demo/web", "namespace"), "kore-demo");

    k.click_sidebar("sidebar.kind.jobs");
    k.wait_label("header.breadcrumb", "Jobs", |l| l == "Jobs");
    k.wait_headers(&["NAMESPACE", "NAME", "STATUS", "COMPLETIONS", "DURATION", "AGE"]);
    let job = "kore-demo/one-shot";
    assert_eq!(k.cell(job, "status"), "Complete");
    assert_eq!(k.cell(job, "completions"), "1/1");
    let d = k.cell(job, "duration");
    assert!(d.ends_with('s') && d[..d.len() - 1].parse::<u32>().is_ok(), "duration {d}");

    k.click_sidebar("sidebar.kind.cronjobs");
    k.wait_label("header.breadcrumb", "Cronjobs", |l| l == "Cronjobs");
    k.wait_headers(&["NAMESPACE", "NAME", "SCHEDULE", "LAST SCHEDULE", "ACTIVE", "AGE"]);
    assert_eq!(k.cell("kore-demo/ticker", "schedule"), "0 0 1 1 *");
    assert_eq!(k.cell("kore-demo/ticker", "active"), "0");
}

/// Scenario 10-shell-table-pods (behavior part): breadcrumb + count pill, the
/// sidebar count, Pod columns, every seeded pod and kubectl-style STATUS labels.
#[gpui_kit::test]
fn pods_table_shows_counts_columns_and_kubectl_status(cx: &mut TestAppContext) {
    let mut k = pods_kore_demo(cx);
    assert_eq!(k.element("header.breadcrumb").unwrap().label, "Pods");
    assert_eq!(k.element("header.search").unwrap().label, "Search...");
    k.wait_headers(&["NAME", "NAMESPACE", "STATUS", "READY", "RESTARTS", "AGE"]);

    let rows = k.row_names();
    assert_eq!(rows.len(), 10, "rows {rows:?}");
    let count = rows.len().to_string();
    assert_eq!(k.element("header.count").unwrap().label, count);
    assert_eq!(k.element("sidebar.count").unwrap().label, count);
    assert!(k.element("sidebar.kind.pods").unwrap().selected);
    for prefix in ["crashloop", "unschedulable", "multi-container", "one-shot-", "logger-", "web-", "kore-demo-"] {
        assert!(rows.iter().any(|r| r.starts_with(prefix)), "no {prefix}* row in {rows:?}");
    }

    assert_eq!(k.cell("kore-demo/crashloop", "status"), "CrashLoopBackOff");
    assert_eq!(k.cell("kore-demo/unschedulable", "status"), "Pending");
    assert_eq!(k.cell("kore-demo/multi-container", "status"), "Running");
    assert_eq!(k.cell("kore-demo/multi-container", "ready"), "2/2");
    assert_eq!(k.cell("kore-demo/crashloop", "ready"), "0/1");
    let one_shot = k.row_key_starting("one-shot-");
    assert_eq!(k.cell(&one_shot, "status"), "Completed");

    assert_eq!(k.element("header.context").unwrap().label, "kind-kore-harness");
    assert!(k.element("header.ai").is_some());
}

/// Scenario 10-shell-bug-table-columns: values as kubectl prints them. Pod
/// STATUS is phase/reason (never Ready/NotReady), RESTARTS sums all containers,
/// Secret TYPE is the secret type, Ingress PORTS is "80" or "80, 443" with TLS.
#[gpui_kit::test]
fn table_values_match_kubectl(cx: &mut TestAppContext) {
    let mut k = pods_kore_demo(cx);
    k.cluster.apply(json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {"name": "shell-restarts-probe", "namespace": "kore-demo"},
        "spec": {"containers": [{"name": "a", "image": "busybox:1.36"}, {"name": "b", "image": "busybox:1.36"}]},
        "status": {
            "phase": "Running",
            "conditions": [{"type": "Ready", "status": "True"}],
            "containerStatuses": [
                {"name": "a", "ready": true, "restartCount": 1, "state": {"running": {}}},
                {"name": "b", "ready": true, "restartCount": 1, "state": {"running": {}}}
            ]
        }
    }));
    k.cluster.apply(json!({
        "apiVersion": "networking.k8s.io/v1",
        "kind": "Ingress",
        "metadata": {"name": "shell-tls-probe", "namespace": "kore-demo"},
        "spec": {
            "tls": [{"hosts": ["tls.kore.local"], "secretName": "web-secret"}],
            "rules": [{"host": "tls.kore.local", "http": {"paths": [{"path": "/", "pathType": "Prefix",
                "backend": {"service": {"name": "web", "port": {"number": 80}}}}]}}]
        }
    }));

    // STATUS: kubectl's phase / reason column.
    let probe = "kore-demo/shell-restarts-probe";
    k.wait_for_element(&format!("table.row.{probe}"));
    let statuses = k.column("status");
    for want in ["CrashLoopBackOff", "Running", "Completed", "Pending"] {
        assert!(statuses.iter().any(|s| s == want), "no {want} in {statuses:?}");
    }
    assert!(!statuses.iter().any(|s| s == "Ready" || s == "NotReady"), "statuses {statuses:?}");

    // RESTARTS sums both containers.
    assert_eq!(k.cell(probe, "status"), "Running");
    assert_eq!(k.cell(probe, "ready"), "2/2");
    assert_eq!(k.cell(probe, "restarts"), "2");

    // Secret TYPE.
    k.click_sidebar("sidebar.kind.secrets");
    k.wait_for_element("table.row.kore-demo/web-secret");
    let types = k.column("secretType");
    assert!(types.iter().any(|t| t == "Opaque") && types.iter().any(|t| t == "helm.sh/release.v1"), "types {types:?}");

    // Ingress PORTS, rows sorted by name (the TLS probe above web).
    k.click_sidebar("sidebar.kind.ingresses");
    k.wait_for_element("table.row.kore-demo/shell-tls-probe");
    assert_eq!(k.row_names(), ["shell-tls-probe", "web"]);
    assert_eq!(k.cell("kore-demo/shell-tls-probe", "ingressPorts"), "80, 443");
    assert_eq!(k.cell("kore-demo/web", "ingressPorts"), "80");
}
