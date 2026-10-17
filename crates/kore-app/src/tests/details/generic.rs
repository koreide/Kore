//! Generic resource details (every kind but pods and deployments).

use super::helpers::{DetailsExt, NS};
use crate::testing::Kore;
use gpui_kit::TestAppContext;
use kore_core::ResourceKind;
use serde_json::json;

fn configmap(name: &str, data: serde_json::Value) -> serde_json::Value {
    json!({"apiVersion": "v1", "kind": "ConfigMap", "metadata": {"name": name, "namespace": NS}, "data": data})
}

/// Scenario 20-details-generic-configmap: Back / Delete; NAME / NAMESPACE /
/// KIND (Configmaps); tabs Describe YAML Events; Describe shows the data;
/// keys 1/2/3 switch tabs and 4 does nothing; Esc returns to the table.
#[gpui_kit::test]
fn configmap_details_header_tabs_and_keys(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.open(ResourceKind::Configmaps, "web-config");
    for id in ["details.back", "details.delete"] {
        assert!(k.element(id).is_some(), "{id} missing");
    }
    assert_eq!(k.label("info.name").as_deref(), Some("web-config"));
    assert_eq!(k.label("info.namespace").as_deref(), Some(NS));
    assert_eq!(k.label("info.kind").as_deref(), Some("Configmaps"));
    assert_eq!(k.labels("tab."), ["Describe", "YAML", "Events"]);
    let text = k.wait_describe("\"kind\": \"ConfigMap\"");
    assert!(text.contains("\"LOG_LEVEL\": \"debug\"") && text.contains("greeting=hello-from-kore"));

    k.keys("2");
    k.wait_tab("yaml");
    k.wait_label("yaml.toggle", "Read-only", |l| l == "Read-only");
    let yaml = k.wait_label("yaml.text", "configmap yaml", |l| l.contains("kind: ConfigMap"));
    assert!(yaml.contains("LOG_LEVEL: debug"));
    k.keys("3");
    k.wait_tab("events");
    k.wait_for_element("events.live");
    k.keys("4");
    k.wait_tab("events");
    assert!(k.element("yaml.toggle").is_none() && k.element("describe.text").is_none());
    k.keys("1");
    k.wait_tab("describe");
    k.wait_describe("\"kind\": \"ConfigMap\"");
    k.keys("escape");
    k.wait_until("table", |k| k.state()["view"] == "table");
    k.wait_gone("info.kind");
    k.wait_for_element("table.row.kore-demo/web-config");
}

/// Scenario 20-details-generic-delete: `d` opens the danger "Delete ConfigMap"
/// dialog; Cancel closes it; `d` + Delete deletes the object, toasts
/// "Resource deleted" and returns to the table, where the row disappears.
#[gpui_kit::test]
fn generic_delete_confirms_deletes_and_returns(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.cluster.apply(configmap("kore-delete-me", json!({"a": "b"})));
    k.open(ResourceKind::Configmaps, "kore-delete-me");
    k.wait_describe("\"kind\": \"ConfigMap\"");
    k.keys("d");
    assert_eq!(k.wait_for_element("confirm").label, "Delete ConfigMap");
    let msg = k.label("confirm.message").unwrap();
    assert!(msg.starts_with("Are you sure you want to delete \"kore-delete-me\"") && msg.contains("cannot be undone"), "{msg}");
    k.click("confirm.cancel");
    k.wait_gone("confirm");
    assert_eq!(k.label("info.name").as_deref(), Some("kore-delete-me"));
    assert!(k.cluster.get("ConfigMap", Some(NS), "kore-delete-me").is_some());

    k.keys("d");
    k.click("confirm.ok");
    k.wait_toast("Resource deleted", |l| l == "Resource deleted");
    k.wait_until("table", |k| k.state()["view"] == "table");
    assert!(k.cluster.get("ConfigMap", Some(NS), "kore-delete-me").is_none());
    k.wait_gone("table.row.kore-demo/kore-delete-me");
}

/// ⌘F in the describe JSON, then `query` must match at least once.
fn describe_search_finds(k: &mut Kore, query: &str) {
    k.open_search();
    k.type_text(query);
    k.wait_label("searchbar.count", &format!("matches for {query:?}"), |l| l.starts_with("1/"));
}

/// Scenario 20-details-generic-kinds (one test per kind): NAME / NAMESPACE /
/// KIND and the describe JSON, where ⌘F finds a kind-specific value; Esc
/// (after leaving the search input) returns to the table.
fn generic_kind(cx: &mut TestAppContext, kind: ResourceKind, row: &str, kind_label: &str, json_kind: &str, needle: &str, query: &str) -> Kore {
    let mut k = Kore::boot(cx);
    let cluster_scoped = kind == ResourceKind::Nodes;
    k.open_row(kind, if cluster_scoped { "*" } else { NS }, row);
    let name = row.rsplit('/').next().unwrap();
    assert_eq!(k.wait_for_element("info.name").label, name);
    assert_eq!(k.label("info.kind").as_deref(), Some(kind_label));
    // Cluster-scoped kinds show the namespace as "default".
    assert_eq!(k.label("info.namespace").as_deref(), Some(if cluster_scoped { "default" } else { NS }));
    let text = k.wait_describe(&format!("\"kind\": \"{json_kind}\""));
    assert!(text.contains(needle), "{kind_label} describe lacks {needle}");
    describe_search_finds(&mut k, query);
    k.cx.update(|window, cx| window.blur(cx));
    k
}

fn esc_to_table(k: &mut Kore) {
    k.cx.update(|window, cx| window.blur(cx));
    k.keys("escape");
    k.wait_until("table", |k| k.state()["view"] == "table");
    k.wait_gone("info.kind");
}

#[gpui_kit::test]
fn service_details(cx: &mut TestAppContext) {
    let mut k = generic_kind(cx, ResourceKind::Services, "kore-demo/web", "Services", "Service", "\"app\": \"web\"", "ClusterIP");
    esc_to_table(&mut k);
}

#[gpui_kit::test]
fn ingress_details(cx: &mut TestAppContext) {
    let mut k =
        generic_kind(cx, ResourceKind::Ingresses, "kore-demo/web", "Ingresses", "Ingress", "\"host\": \"web.kore.local\"", "web.kore.local");
    esc_to_table(&mut k);
}

#[gpui_kit::test]
fn job_details(cx: &mut TestAppContext) {
    let mut k = generic_kind(cx, ResourceKind::Jobs, "kore-demo/one-shot", "Jobs", "Job", "\"name\": \"one-shot\"", "KORE-JOB-DONE");
    esc_to_table(&mut k);
}

#[gpui_kit::test]
fn cronjob_details(cx: &mut TestAppContext) {
    let mut k =
        generic_kind(cx, ResourceKind::Cronjobs, "kore-demo/ticker", "Cronjobs", "CronJob", "\"schedule\": \"0 0 1 1 *\"", "0 0 1 1 *");
    esc_to_table(&mut k);
}

#[gpui_kit::test]
fn node_details(cx: &mut TestAppContext) {
    let mut k = generic_kind(
        cx,
        ResourceKind::Nodes,
        "/kore-harness-control-plane",
        "Nodes",
        "Node",
        "\"name\": \"kore-harness-control-plane\"",
        "kubelet",
    );
    k.keys("3");
    k.wait_tab("events");
    k.wait_for_element("events.live");
    esc_to_table(&mut k);
}

/// Scenario 20-details-generic-secret: a REVEAL bar with one chip per data key;
/// values show as "***" until the chip is toggled, then decoded; toggling again
/// hides it. The YAML tab keeps the raw base64.
#[gpui_kit::test]
fn secret_values_are_hidden_until_revealed(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.open(ResourceKind::Secrets, "web-secret");
    assert_eq!(k.label("info.kind").as_deref(), Some("Secrets"));
    k.wait_describe("\"API_TOKEN\": \"***\"");
    assert!(!k.wait_for_element("reveal.API_TOKEN").selected);
    assert_eq!(k.labels("reveal."), ["API_TOKEN"]);
    k.click("reveal.API_TOKEN");
    k.wait_describe("\"API_TOKEN\": \"kore-harness-token\"");
    assert!(k.element("reveal.API_TOKEN").unwrap().selected);
    k.click("reveal.API_TOKEN");
    let text = k.wait_describe("\"API_TOKEN\": \"***\"");
    assert!(!text.contains("kore-harness-token"));
    k.keys("2");
    k.wait_label("yaml.toggle", "Read-only", |l| l == "Read-only");
    k.wait_label("yaml.text", "raw base64", |l| l.contains("API_TOKEN: a29yZS1oYXJuZXNzLXRva2Vu"));
}
