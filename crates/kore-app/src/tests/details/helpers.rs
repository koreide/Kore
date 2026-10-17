//! Helpers shared by the details tests (an extension trait on the driver).

use crate::testing::Kore;
use kore_core::ResourceKind;

pub const NS: &str = "kore-demo";

pub trait DetailsExt {
    /// Open `name`'s details from the `kind` table in kore-demo (row click).
    fn open(&mut self, kind: ResourceKind, name: &str);
    /// Open the details of table row `table.row.<row_key>` of `kind` in `namespace`.
    fn open_row(&mut self, kind: ResourceKind, namespace: &str, row_key: &str);
    fn open_pod(&mut self, name: &str);
    /// Label of element `id`, if present in the last frame.
    fn label(&mut self, id: &str) -> Option<String>;
    /// Labels of every element whose id starts with `prefix`, in frame order.
    fn labels(&mut self, prefix: &str) -> Vec<String>;
    /// Wait for a toast whose message satisfies `pred`; returns the message.
    fn wait_toast(&mut self, what: &str, pred: impl FnMut(&str) -> bool) -> String;
    /// Wait until the selected tab is `tab` (`tab.<tab>`).
    fn wait_tab(&mut self, tab: &str);
    /// Wait for the Describe text to contain `needle`; returns the text.
    fn wait_describe(&mut self, needle: &str) -> String;
    /// Fully visible label of an element whose label contains `needle` (prefix-filtered).
    fn wait_row(&mut self, prefix: &str, needle: &str) -> String;
    /// Jump GPUI's fake clock by `ms` (e.g. past the table's 500ms refresh
    /// delay; the driver only advances 5ms per frame, slow in debug builds).
    fn advance(&mut self, ms: u64);
    /// Wait until a text entry (input, search bar, terminal) has keyboard focus.
    fn wait_text_entry(&mut self);
    /// ⌘F, then wait for the search input to take focus (it focuses after 50ms).
    fn open_search(&mut self);
}

impl DetailsExt for Kore {
    fn wait_text_entry(&mut self) {
        self.wait_until("text entry focused", |k| k.cx.update(|window, _| crate::keys::text_entry_focused(window)));
    }

    fn open_search(&mut self) {
        self.keys("cmd-f");
        self.wait_for_element("searchbar.input");
        self.advance(60);
        self.wait_text_entry();
    }

    fn advance(&mut self, ms: u64) {
        self.cx.executor().advance_clock(std::time::Duration::from_millis(ms));
        self.frame();
    }

    fn open(&mut self, kind: ResourceKind, name: &str) {
        self.open_row(kind, NS, &format!("{NS}/{name}"));
    }

    fn open_row(&mut self, kind: ResourceKind, namespace: &str, row_key: &str) {
        self.show(kind, namespace);
        // ResourceStore::refresh waits 500ms (+100ms before the watch).
        self.advance(650);
        self.click(&format!("table.row.{row_key}"));
        let name = row_key.rsplit('/').next().unwrap_or_default().to_string();
        self.wait_until("details view", |k| k.state()["view"] == "details" && k.state()["selected"]["name"] == name.as_str());
        self.wait_for_element("view.details");
    }

    fn open_pod(&mut self, name: &str) {
        self.open(ResourceKind::Pods, name);
        self.wait_for_element("pod.status");
    }

    fn label(&mut self, id: &str) -> Option<String> {
        self.element(id).map(|e| e.label)
    }

    fn labels(&mut self, prefix: &str) -> Vec<String> {
        self.elements().into_iter().filter(|e| e.id.starts_with(prefix)).map(|e| e.label).collect()
    }

    fn wait_toast(&mut self, what: &str, mut pred: impl FnMut(&str) -> bool) -> String {
        let mut found = String::new();
        self.wait_until(&format!("toast {what}"), |k| {
            match k.elements().into_iter().find(|e| e.id.starts_with("toast.") && pred(&e.label)) {
                Some(e) => {
                    found = e.label;
                    true
                }
                None => false,
            }
        });
        found
    }

    fn wait_tab(&mut self, tab: &str) {
        let id = format!("tab.{tab}");
        self.wait_until(&format!("{id} selected"), |k| k.element(&id).is_some_and(|e| e.selected));
    }

    fn wait_describe(&mut self, needle: &str) -> String {
        self.wait_label("describe.text", &format!("contains {needle:?}"), |l| l.contains(needle))
    }

    fn wait_row(&mut self, prefix: &str, needle: &str) -> String {
        let mut found = String::new();
        self.wait_until(&format!("{prefix}* containing {needle:?}"), |k| {
            match k.labels(prefix).into_iter().find(|l| l.contains(needle)) {
                Some(l) => {
                    found = l;
                    true
                }
                None => false,
            }
        });
        found
    }
}

/// A minimal running pod in kore-demo (scheduled on the fixture node).
pub fn running_pod(name: &str, containers: &[&str]) -> serde_json::Value {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let specs: Vec<serde_json::Value> =
        containers.iter().map(|c| serde_json::json!({"name": c, "image": "busybox:1.36", "command": ["sleep", "3600"]})).collect();
    let statuses: Vec<serde_json::Value> = containers
        .iter()
        .map(|c| {
            serde_json::json!({"name": c, "image": "busybox:1.36", "ready": true, "restartCount": 0, "started": true,
                "state": {"running": {"startedAt": now}}})
        })
        .collect();
    serde_json::json!({
        "apiVersion": "v1", "kind": "Pod",
        "metadata": {"name": name, "namespace": NS, "labels": {"run": name}},
        "spec": {"nodeName": "kore-harness-control-plane", "containers": specs},
        "status": {"phase": "Running", "podIP": "10.244.0.99", "hostIP": "172.18.0.2", "startTime": now,
            "conditions": [{"type": "Ready", "status": "True"}], "containerStatuses": statuses}
    })
}

/// A ready Deployment in kore-demo whose `spec.selector` is `selector`
/// (matchLabels; the pod template carries the same labels), with `replicas`
/// ready replicas.
pub fn deployment(name: &str, selector: serde_json::Value, replicas: i64, containers: &[&str]) -> serde_json::Value {
    let specs: Vec<serde_json::Value> =
        containers.iter().map(|c| serde_json::json!({"name": c, "image": "busybox:1.36", "command": ["sleep", "3600"]})).collect();
    serde_json::json!({
        "apiVersion": "apps/v1", "kind": "Deployment",
        "metadata": {"name": name, "namespace": NS, "generation": 1},
        "spec": {
            "replicas": replicas,
            "selector": {"matchLabels": selector},
            "template": {"metadata": {"labels": selector}, "spec": {"containers": specs}}
        },
        "status": {"replicas": replicas, "readyReplicas": replicas, "updatedReplicas": replicas, "availableReplicas": replicas,
            "observedGeneration": 1}
    })
}

/// `running_pod` with `labels` (a Deployment's pod).
pub fn labelled_pod(name: &str, labels: serde_json::Value, containers: &[&str]) -> serde_json::Value {
    let mut p = running_pod(name, containers);
    p["metadata"]["labels"] = labels;
    p
}
