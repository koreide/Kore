//! `ResourceItem` and `toResourceItem` (spec 01 §7.5), age formatting (§7.6)
//! and status variants (§7.7).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Unified table row derived from a raw Kubernetes object.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceItem {
    pub name: String,
    pub namespace: String,
    pub status: Option<String>,
    /// metadata.creationTimestamp (ISO string).
    pub age: Option<String>,
    pub ready: Option<String>,
    pub restarts: Option<i64>,
    pub node: Option<String>,
    pub ip: Option<String>,
    pub up_to_date: Option<i64>,
    pub available: Option<i64>,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub cluster_ip: Option<String>,
    pub external_ip: Option<String>,
    pub ports: Option<String>,
    pub roles: Option<String>,
    pub version: Option<String>,
    pub reason: Option<String>,
    pub message: Option<String>,
    pub count: Option<i64>,
    pub last_seen: Option<String>,
    pub involved_object: Option<String>,
    pub event_type: Option<String>,
    pub completions: Option<String>,
    pub duration: Option<String>,
    pub schedule: Option<String>,
    pub last_schedule: Option<String>,
    pub active: Option<i64>,
    pub data_keys: Option<i64>,
    pub hosts: Option<String>,
    pub ingress_class: Option<String>,
    /// Secret `type` (kept at the object root, e.g. "Opaque").
    pub secret_type: Option<String>,
    /// Ingress PORTS as kubectl prints them: "80", or "80, 443" with TLS.
    pub ingress_ports: Option<String>,
    #[serde(rename = "_kind")]
    pub kind_marker: Option<String>,
    #[serde(rename = "_context")]
    pub context: Option<String>,
}

impl ResourceItem {
    /// Identity used for watch upserts: (name, namespace) (+ context in multi-cluster).
    pub fn key(&self) -> (String, String) {
        (self.name.clone(), self.namespace.clone())
    }
}

fn s(v: &Value) -> Option<String> {
    v.as_str().map(|x| x.to_string())
}

fn n(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_f64().map(|f| f as i64))
}

/// Port of `toResourceItem` (src/lib/transforms.ts). Returns None if the
/// object has no name.
pub fn to_resource_item(obj: &Value) -> Option<ResourceItem> {
    let metadata = &obj["metadata"];
    let status = &obj["status"];
    let spec = &obj["spec"];
    let name = s(&metadata["name"]).unwrap_or_default();
    if name.is_empty() {
        return None;
    }

    let mut ready = None;
    let container_statuses = status["containerStatuses"].as_array();
    if let Some(cs) = container_statuses {
        let r = cs.iter().filter(|c| c["ready"].as_bool() == Some(true)).count();
        ready = Some(format!("{}/{}", r, cs.len()));
    } else {
        let rr = n(&status["readyReplicas"]);
        let rep = n(&status["replicas"]);
        match (rr, rep) {
            (Some(a), Some(b)) => ready = Some(format!("{a}/{b}")),
            (Some(a), None) => ready = Some(a.to_string()),
            _ => {}
        }
    }

    let svc_type = s(&spec["type"]);
    let cluster_ip = s(&spec["clusterIP"]);

    let mut external_ip: Option<String> = None;
    if let Some(ing) = status["loadBalancer"]["ingress"].as_array() {
        if let Some(first) = ing.first() {
            external_ip = s(&first["ip"]).filter(|x| !x.is_empty()).or_else(|| s(&first["hostname"]).filter(|x| !x.is_empty()));
        }
    }
    if external_ip.is_none() {
        if let Some(e) = spec["externalIPs"].as_array().and_then(|a| a.first()).and_then(|v| v.as_str()) {
            external_ip = Some(e.to_string());
        }
    }
    if external_ip.is_none() && svc_type.as_deref() == Some("LoadBalancer") {
        external_ip = Some("<pending>".into());
    }
    let external_ip = external_ip.or_else(|| Some("<none>".into()));

    let ports = spec["ports"].as_array().map(|ps| {
        ps.iter()
            .map(|p| {
                let port = js_string(&p["port"]);
                let protocol = p["protocol"].as_str().filter(|x| !x.is_empty()).unwrap_or("TCP");
                let node_port = &p["nodePort"];
                if truthy(node_port) {
                    format!("{}:{}/{}", js_string(node_port), port, protocol)
                } else {
                    format!("{port}/{protocol}")
                }
            })
            .collect::<Vec<_>>()
            .join(",")
    });

    let conditions = status["conditions"].as_array();
    let mut node_status = None;
    if let Some(conds) = conditions {
        if let Some(rc) = conds.iter().find(|c| c["type"].as_str() == Some("Ready")) {
            node_status = Some(if rc["status"].as_str() == Some("True") { "Ready" } else { "NotReady" }.to_string());
        }
    }
    if node_status.is_none() {
        node_status = s(&status["phase"]);
    }

    let mut roles = None;
    if let Some(labels) = metadata["labels"].as_object() {
        let mut rl = Vec::new();
        for (k, v) in labels {
            if let Some(r) = k.strip_prefix("node-role.kubernetes.io/") {
                rl.push(r.to_string());
            } else if k == "kubernetes.io/role" {
                rl.push(v.as_str().unwrap_or_default().to_string());
            }
        }
        if !rl.is_empty() {
            roles = Some(rl.join(","));
        }
    }
    let roles = roles.or_else(|| Some("<none>".into()));

    let involved = obj["involvedObject"].as_object().map(|io| {
        format!(
            "{}/{}",
            io.get("kind").and_then(|v| v.as_str()).unwrap_or(""),
            io.get("name").and_then(|v| v.as_str()).unwrap_or("")
        )
    });

    let mut job_status = None;
    if let Some(conds) = conditions {
        let complete = conds.iter().any(|c| c["type"] == "Complete" && c["status"] == "True");
        let failed = conds.iter().any(|c| c["type"] == "Failed" && c["status"] == "True");
        if complete {
            job_status = Some("Complete".to_string());
        } else if failed {
            job_status = Some("Failed".to_string());
        }
    }

    let succeeded = n(&status["succeeded"]);
    let job_active = n(&status["active"]);
    let spec_completions = n(&spec["completions"]);
    let completions = if succeeded.is_some() || spec_completions.is_some() {
        Some(format!("{}/{}", succeeded.unwrap_or(0), spec_completions.unwrap_or(1)))
    } else {
        None
    };

    let duration = match (s(&status["startTime"]), s(&status["completionTime"])) {
        (Some(a), Some(b)) => match (parse_ts(&a), parse_ts(&b)) {
            (Some(a), Some(b)) => {
                let d = (b - a).num_milliseconds().div_euclid(1000);
                Some(if d < 60 {
                    format!("{d}s")
                } else if d < 3600 {
                    format!("{}m{}s", d / 60, d % 60)
                } else {
                    format!("{}h{}m", d / 3600, (d % 3600) / 60)
                })
            }
            _ => Some("NaNs".into()),
        },
        _ => None,
    };

    let active_count = status["active"].as_array().map(|a| a.len() as i64);
    let data_keys = obj["data"].as_object().map(|d| d.len() as i64);
    let hosts = spec["rules"].as_array().map(|rules| {
        rules
            .iter()
            .map(|r| r["host"].as_str().filter(|h| !h.is_empty()).unwrap_or("*").to_string())
            .collect::<Vec<_>>()
            .join(", ")
    });

    let creation = s(&metadata["creationTimestamp"]);
    let kind = obj["kind"].as_str().unwrap_or_default();
    let is_pod = kind == "Pod" || (kind.is_empty() && spec["containers"].is_array());
    let is_secret = kind == "Secret";
    let is_ingress = kind == "Ingress" || (kind.is_empty() && spec["rules"].is_array());
    let (status_str, restarts) = if is_pod {
        let (st, rc) = pod_status(obj);
        (Some(st), Some(rc))
    } else {
        let st = job_status
            .or(node_status)
            .or_else(|| s(&status["phase"]))
            .or_else(|| conditions.and_then(|c| c.first()).and_then(|c| s(&c["type"])));
        (st, container_statuses.map(|cs| cs.iter().filter_map(|c| n(&c["restartCount"])).sum()))
    };
    let secret_type = if is_secret { s(&obj["type"]) } else { None };
    let ingress_ports = is_ingress.then(|| {
        let tls = spec["tls"].as_array().map(|t| !t.is_empty()).unwrap_or(false);
        if tls { "80, 443" } else { "80" }.to_string()
    });

    Some(ResourceItem {
        name,
        namespace: s(&metadata["namespace"]).unwrap_or_default(),
        status: status_str,
        age: creation.clone(),
        ready,
        restarts,
        node: s(&status["hostIP"]),
        ip: s(&status["podIP"]),
        up_to_date: n(&status["updatedReplicas"]),
        available: n(&status["availableReplicas"]),
        type_: svc_type,
        cluster_ip,
        external_ip,
        ports,
        roles,
        version: s(&status["nodeInfo"]["kubeletVersion"]),
        reason: s(&obj["reason"]),
        message: s(&obj["message"]),
        count: n(&obj["count"]),
        last_seen: s(&obj["lastTimestamp"]).or(creation),
        involved_object: involved,
        event_type: s(&obj["type"]),
        completions,
        duration,
        schedule: s(&spec["schedule"]),
        last_schedule: s(&status["lastScheduleTime"]),
        active: active_count.or(job_active),
        data_keys,
        hosts,
        ingress_class: s(&spec["ingressClassName"]),
        secret_type,
        ingress_ports,
        kind_marker: s(&obj["_kind"]),
        context: s(&obj["_context"]),
    })
}

/// Pod STATUS and RESTARTS as `kubectl get pods` prints them (kubectl's
/// `printPod`): the phase, overridden by `status.reason`, the first unfinished
/// init container (`Init:N/M`, `Init:<reason>`), container waiting/terminated
/// reasons (`CrashLoopBackOff`, `ContainerCreating`, `Completed`, `Error`…), and
/// `Terminating` while a deletion timestamp is set. Restarts are summed over
/// the containers (plus init containers while initializing).
pub fn pod_status(pod: &Value) -> (String, i64) {
    let status = &pod["status"];
    let mut reason = s(&status["phase"]).unwrap_or_else(|| "Unknown".into());
    if let Some(r) = s(&status["reason"]).filter(|r| !r.is_empty()) {
        reason = r;
    }
    let empty = Vec::new();
    let init_total = pod["spec"]["initContainers"].as_array().map(|a| a.len()).unwrap_or(0);
    let mut restarts: i64 = 0;
    let mut initializing = false;
    for (i, c) in status["initContainerStatuses"].as_array().unwrap_or(&empty).iter().enumerate() {
        restarts += n(&c["restartCount"]).unwrap_or(0);
        let term = &c["state"]["terminated"];
        let wait_reason = c["state"]["waiting"]["reason"].as_str().unwrap_or_default();
        if term.is_object() && n(&term["exitCode"]) == Some(0) {
            continue;
        }
        initializing = true;
        reason = if term.is_object() {
            match term["reason"].as_str().filter(|r| !r.is_empty()) {
                Some(r) => format!("Init:{r}"),
                None => match n(&term["signal"]).filter(|s| *s != 0) {
                    Some(sig) => format!("Init:Signal:{sig}"),
                    None => format!("Init:ExitCode:{}", n(&term["exitCode"]).unwrap_or(0)),
                },
            }
        } else if !wait_reason.is_empty() && wait_reason != "PodInitializing" {
            format!("Init:{wait_reason}")
        } else {
            format!("Init:{i}/{init_total}")
        };
        break;
    }
    if !initializing {
        restarts = 0;
        let mut has_running = false;
        for c in status["containerStatuses"].as_array().unwrap_or(&empty).iter().rev() {
            restarts += n(&c["restartCount"]).unwrap_or(0);
            let st = &c["state"];
            let term = &st["terminated"];
            if let Some(w) = st["waiting"]["reason"].as_str().filter(|r| !r.is_empty()) {
                reason = w.to_string();
            } else if let Some(t) = term["reason"].as_str().filter(|r| !r.is_empty()) {
                reason = t.to_string();
            } else if term.is_object() {
                reason = match n(&term["signal"]).filter(|s| *s != 0) {
                    Some(sig) => format!("Signal:{sig}"),
                    None => format!("ExitCode:{}", n(&term["exitCode"]).unwrap_or(0)),
                };
            } else if c["ready"].as_bool() == Some(true) && st["running"].is_object() {
                has_running = true;
            }
        }
        if reason == "Completed" && has_running {
            let ready = status["conditions"]
                .as_array()
                .map(|cs| cs.iter().any(|c| c["type"] == "Ready" && c["status"] == "True"))
                .unwrap_or(false);
            reason = if ready { "Running" } else { "NotReady" }.into();
        }
    }
    if !pod["metadata"]["deletionTimestamp"].is_null() {
        reason = if status["reason"].as_str() == Some("NodeLost") { "Unknown" } else { "Terminating" }.into();
    }
    (reason, restarts)
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

/// JS template-literal stringification of a JSON value.
fn js_string(v: &Value) -> String {
    match v {
        Value::Null => "undefined".into(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

pub fn parse_ts(ts: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(ts).ok().map(|d| d.with_timezone(&chrono::Utc))
}

/// `formatRelativeAge` (spec 01 §7.6).
pub fn format_relative_age(ts: Option<&str>) -> String {
    let Some(ts) = ts else { return "-".into() };
    let Some(created) = parse_ts(ts) else { return "-".into() };
    let secs = (chrono::Utc::now() - created).num_milliseconds().div_euclid(1000);
    let mins = secs.div_euclid(60);
    let hours = mins.div_euclid(60);
    let days = hours.div_euclid(24);
    if days > 0 {
        format!("{days}d")
    } else if hours > 0 {
        format!("{hours}h")
    } else if mins > 0 {
        format!("{mins}m")
    } else {
        format!("{secs}s")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusVariant {
    Running,
    Pending,
    Failed,
    Terminating,
    Default,
}

pub fn status_variant(status: Option<&str>) -> StatusVariant {
    let Some(s) = status else { return StatusVariant::Default };
    match s.to_lowercase().as_str() {
        "running" | "ready" | "succeeded" | "completed" | "complete" => StatusVariant::Running,
        "pending" | "containercreating" | "podinitializing" | "init" | "waiting" => StatusVariant::Pending,
        "failed" | "crashloopbackoff" | "error" | "imagepullbackoff" | "errimagepull" | "evicted" | "oomkilled"
        | "createcontainerconfigerror" | "createcontainererror" | "invalidimagename" | "containerstatusunknown" => {
            StatusVariant::Failed
        }
        "terminating" => StatusVariant::Terminating,
        l if l.starts_with("exitcode:") || l.starts_with("signal:") => StatusVariant::Failed,
        l if l.starts_with("init:") => match status_variant(Some(&s[5..])) {
            StatusVariant::Failed => StatusVariant::Failed,
            _ => StatusVariant::Pending,
        },
        _ => StatusVariant::Default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pod_ready_status() {
        let pod = json!({
            "kind": "Pod",
            "metadata": {"name": "p", "namespace": "ns", "creationTimestamp": "2024-01-01T00:00:00Z"},
            "status": {"phase": "Running", "conditions": [{"type": "Ready", "status": "True"}],
                       "containerStatuses": [{"ready": true, "restartCount": 3}, {"ready": false, "restartCount": 1}]}
        });
        let it = to_resource_item(&pod).unwrap();
        // No container state → the phase, like kubectl.
        assert_eq!(it.status.as_deref(), Some("Running"));
        assert_eq!(it.ready.as_deref(), Some("1/2"));
        // Restarts are summed over all containers.
        assert_eq!(it.restarts, Some(4));
        assert_eq!(it.external_ip.as_deref(), Some("<none>"));
    }

    fn pod(status: serde_json::Value) -> serde_json::Value {
        json!({"kind": "Pod", "metadata": {"name": "p", "namespace": "ns"}, "spec": {"containers": [{"name": "c"}]}, "status": status})
    }

    fn pod_st(status: serde_json::Value) -> String {
        to_resource_item(&pod(status)).unwrap().status.unwrap()
    }

    #[test]
    fn pod_status_like_kubectl() {
        let running = json!({"phase": "Running", "conditions": [{"type": "Ready", "status": "True"}],
            "containerStatuses": [{"ready": true, "restartCount": 0, "state": {"running": {}}}]});
        assert_eq!(pod_st(running.clone()), "Running");
        assert_eq!(pod_st(json!({"phase": "Pending", "conditions": [{"type": "PodScheduled", "status": "False"}]})), "Pending");
        assert_eq!(pod_st(json!({"phase": "Pending", "containerStatuses": [{"ready": false, "state": {"waiting": {"reason": "ContainerCreating"}}}]})), "ContainerCreating");
        assert_eq!(pod_st(json!({"phase": "Running", "containerStatuses": [{"ready": false, "restartCount": 5, "state": {"waiting": {"reason": "CrashLoopBackOff"}}}]})), "CrashLoopBackOff");
        assert_eq!(pod_st(json!({"phase": "Running", "containerStatuses": [{"ready": false, "state": {"terminated": {"reason": "Error", "exitCode": 1}}}]})), "Error");
        assert_eq!(pod_st(json!({"phase": "Succeeded", "conditions": [{"type": "Ready", "status": "False"}],
            "containerStatuses": [{"ready": false, "state": {"terminated": {"reason": "Completed", "exitCode": 0}}}]})), "Completed");
        assert_eq!(pod_st(json!({"phase": "Succeeded"})), "Succeeded");
        assert_eq!(pod_st(json!({"phase": "Failed", "reason": "Evicted"})), "Evicted");
        assert_eq!(pod_st(json!({"phase": "Running", "containerStatuses": [{"state": {"terminated": {"exitCode": 137, "signal": 9}}}]})), "Signal:9");
        // Init containers.
        let mut p = pod(json!({"phase": "Pending", "initContainerStatuses": [
            {"restartCount": 2, "state": {"terminated": {"exitCode": 0, "reason": "Completed"}}},
            {"restartCount": 1, "state": {"running": {}}}],
            "containerStatuses": [{"restartCount": 0, "state": {"waiting": {"reason": "PodInitializing"}}}]}));
        p["spec"]["initContainers"] = json!([{"name": "a"}, {"name": "b"}]);
        let it = to_resource_item(&p).unwrap();
        assert_eq!(it.status.as_deref(), Some("Init:1/2"));
        assert_eq!(it.restarts, Some(3));
        assert_eq!(pod_st(json!({"phase": "Pending", "initContainerStatuses": [{"state": {"waiting": {"reason": "CrashLoopBackOff"}}}]})), "Init:CrashLoopBackOff");
        // Terminating wins.
        let mut p = pod(running);
        p["metadata"]["deletionTimestamp"] = json!("2024-01-01T00:00:00Z");
        assert_eq!(to_resource_item(&p).unwrap().status.as_deref(), Some("Terminating"));
        // Restarts summed (multi-container).
        let it = to_resource_item(&pod(json!({"phase": "Running", "containerStatuses": [
            {"ready": true, "restartCount": 2, "state": {"running": {}}},
            {"ready": true, "restartCount": 5, "state": {"running": {}}}]}))).unwrap();
        assert_eq!(it.restarts, Some(7));
        assert_eq!(it.status.as_deref(), Some("Running"));
    }

    #[test]
    fn non_pods_keep_ready_condition() {
        let node = json!({"kind": "Node", "metadata": {"name": "n"}, "status": {"phase": "x", "conditions": [{"type": "Ready", "status": "True"}]}});
        assert_eq!(to_resource_item(&node).unwrap().status.as_deref(), Some("Ready"));
    }

    #[test]
    fn secret_type_and_ingress_ports() {
        let sec = json!({"kind": "Secret", "metadata": {"name": "s"}, "type": "Opaque", "data": {"a": "b"}});
        let it = to_resource_item(&sec).unwrap();
        assert_eq!(it.secret_type.as_deref(), Some("Opaque"));
        assert_eq!(it.data_keys, Some(1));
        let ev = json!({"kind": "Event", "metadata": {"name": "e"}, "type": "Warning"});
        assert_eq!(to_resource_item(&ev).unwrap().secret_type, None);
        let ing = json!({"kind": "Ingress", "metadata": {"name": "i"}, "spec": {"rules": [{"host": "a"}]}});
        assert_eq!(to_resource_item(&ing).unwrap().ingress_ports.as_deref(), Some("80"));
        let ing = json!({"kind": "Ingress", "metadata": {"name": "i"}, "spec": {"rules": [{"host": "a"}], "tls": [{"hosts": ["a"]}]}});
        assert_eq!(to_resource_item(&ing).unwrap().ingress_ports.as_deref(), Some("80, 443"));
        let svc = json!({"kind": "Service", "metadata": {"name": "s"}, "spec": {"ports": []}});
        assert_eq!(to_resource_item(&svc).unwrap().ingress_ports, None);
    }

    #[test]
    fn service_ports_and_job_fields() {
        let svc = json!({"metadata": {"name": "s"}, "spec": {"type": "NodePort", "ports": [{"port": 80, "nodePort": 30080}, {"port": 53, "protocol": "UDP"}]}});
        let it = to_resource_item(&svc).unwrap();
        assert_eq!(it.ports.as_deref(), Some("30080:80/TCP,53/UDP"));
        let job = json!({"metadata": {"name": "j"}, "spec": {"completions": 1},
            "status": {"succeeded": 1, "startTime": "2024-01-01T00:00:00Z", "completionTime": "2024-01-01T00:02:05Z",
                       "conditions": [{"type": "Complete", "status": "True"}]}});
        let it = to_resource_item(&job).unwrap();
        assert_eq!(it.status.as_deref(), Some("Complete"));
        assert_eq!(it.completions.as_deref(), Some("1/1"));
        assert_eq!(it.duration.as_deref(), Some("2m5s"));
        assert!(to_resource_item(&json!({"metadata": {}})).is_none());
    }

    #[test]
    fn variants() {
        assert_eq!(status_variant(Some("NotReady")), StatusVariant::Default);
        assert_eq!(status_variant(Some("CrashLoopBackOff")), StatusVariant::Failed);
        assert_eq!(status_variant(Some("Completed")), StatusVariant::Running);
        assert_eq!(status_variant(Some("ContainerCreating")), StatusVariant::Pending);
        assert_eq!(status_variant(Some("Init:0/2")), StatusVariant::Pending);
        assert_eq!(status_variant(Some("Init:Error")), StatusVariant::Failed);
        assert_eq!(status_variant(Some("ExitCode:2")), StatusVariant::Failed);
        assert_eq!(status_variant(Some("Terminating")), StatusVariant::Terminating);
        assert_eq!(format_relative_age(None), "-");
        assert_eq!(format_relative_age(Some("garbage")), "-");
    }
}
