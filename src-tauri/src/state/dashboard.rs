use crate::constants::MAX_DASHBOARD_RESOURCES;
use crate::error::{K8sError, Result};
use crate::state::{K8sState, ResourceKind};
use kube::config::KubeConfigOptions;
use kube::{Client, Config};
use serde::Serialize;
use tracing::warn;

#[derive(Debug, Clone, Serialize)]
pub struct ClusterHealth {
    pub score: u32,
    pub pods: PodHealth,
    pub nodes: Vec<NodeHealth>,
    pub restart_hotlist: Vec<RestartHotItem>,
    pub pending_pods: Vec<PendingPod>,
    pub recent_warnings: Vec<WarningEvent>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PodHealth {
    pub running: u32,
    pub pending: u32,
    pub failed: u32,
    pub succeeded: u32,
    pub crash_looping: u32,
    pub total: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct NodeHealth {
    pub name: String,
    pub status: String,
    pub cpu_capacity: String,
    pub memory_capacity: String,
    pub cpu_usage: String,
    pub memory_usage: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RestartHotItem {
    pub name: String,
    pub namespace: String,
    pub restarts: i64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PendingPod {
    pub name: String,
    pub namespace: String,
    pub reason: String,
    pub age: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WarningEvent {
    pub reason: String,
    pub message: String,
    pub involved_object: String,
    pub namespace: String,
    pub count: i64,
    pub last_seen: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClusterHealthEntry {
    pub context: String,
    pub health: ClusterHealth,
}

#[derive(Debug, Clone, Serialize)]
pub struct MultiClusterHealth {
    pub clusters: Vec<ClusterHealthEntry>,
}

/// Compute cluster health from raw pod, node, and event JSON values.
pub fn compute_health(
    pods: &[serde_json::Value],
    nodes: &[serde_json::Value],
    events: &[serde_json::Value],
) -> ClusterHealth {
    // Pod health counts
    let mut running = 0u32;
    let mut pending = 0u32;
    let mut failed = 0u32;
    let mut succeeded = 0u32;
    let mut crash_looping = 0u32;
    let mut restart_hotlist = Vec::new();
    let mut pending_pods = Vec::new();

    for pod in pods {
        let phase = pod
            .pointer("/status/phase")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let name = pod
            .pointer("/metadata/name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let ns = pod
            .pointer("/metadata/namespace")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // Check for CrashLoopBackOff
        let is_crash_loop = pod
            .pointer("/status/containerStatuses")
            .and_then(|v| v.as_array())
            .map(|statuses| {
                statuses.iter().any(|cs| {
                    cs.pointer("/state/waiting/reason")
                        .and_then(|v| v.as_str())
                        .map(|r| r == "CrashLoopBackOff")
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);

        if is_crash_loop {
            crash_looping += 1;
        } else {
            match phase {
                "Running" => running += 1,
                "Pending" => {
                    pending += 1;
                    let reason = pod
                        .pointer("/status/conditions")
                        .and_then(|v| v.as_array())
                        .and_then(|conditions| {
                            conditions.iter().find_map(|c| {
                                let status = c.get("status")?.as_str()?;
                                if status == "False" {
                                    c.get("reason")?.as_str().map(String::from)
                                } else {
                                    None
                                }
                            })
                        })
                        .unwrap_or_else(|| "Unknown".to_string());
                    let age = pod
                        .pointer("/metadata/creationTimestamp")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    pending_pods.push(PendingPod {
                        name: name.to_string(),
                        namespace: ns.to_string(),
                        reason,
                        age,
                    });
                }
                "Failed" => failed += 1,
                "Succeeded" => succeeded += 1,
                _ => {}
            }
        }

        // Check restarts
        let restarts: i64 = pod
            .pointer("/status/containerStatuses")
            .and_then(|v| v.as_array())
            .map(|statuses| {
                statuses
                    .iter()
                    .filter_map(|cs| cs.get("restartCount")?.as_i64())
                    .sum()
            })
            .unwrap_or(0);

        if restarts > 0 {
            restart_hotlist.push(RestartHotItem {
                name: name.to_string(),
                namespace: ns.to_string(),
                restarts,
                status: phase.to_string(),
            });
        }
    }

    restart_hotlist.sort_by(|a, b| b.restarts.cmp(&a.restarts));
    restart_hotlist.truncate(10);

    let total = running + pending + failed + succeeded + crash_looping;
    let pod_health = PodHealth {
        running,
        pending,
        failed,
        succeeded,
        crash_looping,
        total,
    };

    // Node health
    let node_health: Vec<NodeHealth> = nodes
        .iter()
        .map(|node| {
            let name = node
                .pointer("/metadata/name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let status = node
                .pointer("/status/conditions")
                .and_then(|v| v.as_array())
                .and_then(|conditions| {
                    conditions.iter().find_map(|c| {
                        let t = c.get("type")?.as_str()?;
                        let s = c.get("status")?.as_str()?;
                        if t == "Ready" {
                            Some(if s == "True" { "Ready" } else { "NotReady" })
                        } else {
                            None
                        }
                    })
                })
                .unwrap_or("Unknown")
                .to_string();

            let cpu_cap = node
                .pointer("/status/capacity/cpu")
                .and_then(|v| v.as_str())
                .unwrap_or("0")
                .to_string();
            let mem_cap = node
                .pointer("/status/capacity/memory")
                .and_then(|v| v.as_str())
                .unwrap_or("0")
                .to_string();
            let cpu_alloc = node
                .pointer("/status/allocatable/cpu")
                .and_then(|v| v.as_str())
                .unwrap_or("0")
                .to_string();
            let mem_alloc = node
                .pointer("/status/allocatable/memory")
                .and_then(|v| v.as_str())
                .unwrap_or("0")
                .to_string();

            NodeHealth {
                name,
                status,
                cpu_capacity: cpu_cap,
                memory_capacity: mem_cap,
                cpu_usage: cpu_alloc,
                memory_usage: mem_alloc,
            }
        })
        .collect();

    // Recent warnings from events
    let recent_warnings: Vec<WarningEvent> = events
        .iter()
        .filter(|e| {
            e.get("type")
                .and_then(|v| v.as_str())
                .map(|t| t == "Warning")
                .unwrap_or(false)
        })
        .take(20)
        .map(|e| {
            let involved = e
                .pointer("/involvedObject")
                .map(|io| {
                    format!(
                        "{}/{}",
                        io.get("kind").and_then(|v| v.as_str()).unwrap_or(""),
                        io.get("name").and_then(|v| v.as_str()).unwrap_or("")
                    )
                })
                .unwrap_or_default();

            WarningEvent {
                reason: e
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                message: e
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                involved_object: involved,
                namespace: e
                    .pointer("/metadata/namespace")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                count: e.get("count").and_then(|v| v.as_i64()).unwrap_or(1),
                last_seen: e
                    .get("lastTimestamp")
                    .or_else(|| e.pointer("/metadata/creationTimestamp"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            }
        })
        .collect();

    // Calculate health score (0-100)
    let node_ok = node_health.iter().filter(|n| n.status == "Ready").count() as f64;
    let node_total = node_health.len().max(1) as f64;
    let node_score = (node_ok / node_total) * 30.0;

    let pod_ok = running as f64;
    let pod_total = total.max(1) as f64;
    let pod_score = (pod_ok / pod_total) * 40.0;

    let crash_penalty = (crash_looping as f64 * 5.0).min(15.0);
    let pending_penalty = (pending as f64 * 2.0).min(10.0);
    let warning_penalty = (recent_warnings.len() as f64 * 0.5).min(5.0);

    let score = (node_score + pod_score + 30.0 - crash_penalty - pending_penalty - warning_penalty)
        .clamp(0.0, 100.0) as u32;

    ClusterHealth {
        score,
        pods: pod_health,
        nodes: node_health,
        restart_hotlist,
        pending_pods,
        recent_warnings,
    }
}

impl K8sState {
    pub async fn get_cluster_health(&self) -> Result<ClusterHealth> {
        // Fetch all resources in parallel
        let (pods_result, nodes_result, events_result) = tokio::join!(
            self.list_resources(ResourceKind::Pods, None, None),
            self.list_resources(ResourceKind::Nodes, None, None),
            self.list_resources(ResourceKind::Events, None, None),
        );

        let pods = pods_result
            .map_err(|e| {
                warn!(error = %e, "Failed to fetch pods for cluster health");
                e
            })
            .unwrap_or_default();
        let nodes = nodes_result
            .map_err(|e| {
                warn!(error = %e, "Failed to fetch nodes for cluster health");
                e
            })
            .unwrap_or_default();
        let events = events_result
            .map_err(|e| {
                warn!(error = %e, "Failed to fetch events for cluster health");
                e
            })
            .unwrap_or_default();

        Ok(compute_health(&pods, &nodes, &events))
    }

    pub async fn get_cluster_health_multi_cluster(&self) -> Result<MultiClusterHealth> {
        let inner = self.inner.read().await;
        let kubeconfig = inner.kubeconfig.clone().ok_or(K8sError::ClientMissing)?;
        drop(inner);

        let context_names: Vec<String> =
            kubeconfig.contexts.iter().map(|c| c.name.clone()).collect();

        let mut handles = Vec::new();

        for ctx_name in context_names {
            let kc = kubeconfig.clone();
            let ctx = ctx_name.clone();

            handles.push(tokio::spawn(async move {
                let opts = KubeConfigOptions {
                    context: Some(ctx.clone()),
                    ..Default::default()
                };
                let config = match Config::from_custom_kubeconfig(kc, &opts).await {
                    Ok(c) => c,
                    Err(e) => {
                        warn!(context = %ctx, error = %e, "Failed to create config for context");
                        return None;
                    }
                };
                let client = match Client::try_from(config) {
                    Ok(c) => c,
                    Err(e) => {
                        warn!(context = %ctx, error = %e, "Failed to create client for context");
                        return None;
                    }
                };

                let (pods, nodes, events) = tokio::join!(
                    fetch_resources_with_client(&client, &ResourceKind::Pods),
                    fetch_resources_with_client(&client, &ResourceKind::Nodes),
                    fetch_resources_with_client(&client, &ResourceKind::Events),
                );

                Some(ClusterHealthEntry {
                    context: ctx,
                    health: compute_health(&pods, &nodes, &events),
                })
            }));
        }

        let mut clusters = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(Some(entry)) => clusters.push(entry),
                Ok(None) => {}
                Err(e) => warn!(error = %e, "Task join error"),
            }
        }

        Ok(MultiClusterHealth { clusters })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_pod(name: &str, ns: &str, phase: &str) -> serde_json::Value {
        json!({
            "metadata": { "name": name, "namespace": ns },
            "status": { "phase": phase }
        })
    }

    fn make_pod_with_restarts(
        name: &str,
        ns: &str,
        phase: &str,
        restarts: i64,
    ) -> serde_json::Value {
        json!({
            "metadata": { "name": name, "namespace": ns },
            "status": {
                "phase": phase,
                "containerStatuses": [{
                    "restartCount": restarts
                }]
            }
        })
    }

    fn make_crashloop_pod(name: &str, ns: &str) -> serde_json::Value {
        json!({
            "metadata": { "name": name, "namespace": ns },
            "status": {
                "phase": "Running",
                "containerStatuses": [{
                    "restartCount": 5,
                    "state": { "waiting": { "reason": "CrashLoopBackOff" } }
                }]
            }
        })
    }

    fn make_node(name: &str, ready: bool) -> serde_json::Value {
        json!({
            "metadata": { "name": name },
            "status": {
                "conditions": [{
                    "type": "Ready",
                    "status": if ready { "True" } else { "False" }
                }],
                "capacity": { "cpu": "4", "memory": "16Gi" },
                "allocatable": { "cpu": "3800m", "memory": "15Gi" }
            }
        })
    }

    fn make_warning_event(reason: &str, message: &str) -> serde_json::Value {
        json!({
            "type": "Warning",
            "reason": reason,
            "message": message,
            "metadata": { "namespace": "default" },
            "involvedObject": { "kind": "Pod", "name": "my-pod" },
            "count": 1,
            "lastTimestamp": "2026-03-16T12:00:00Z"
        })
    }

    fn make_normal_event(reason: &str) -> serde_json::Value {
        json!({
            "type": "Normal",
            "reason": reason,
            "message": "normal event",
            "metadata": { "namespace": "default" },
            "involvedObject": { "kind": "Pod", "name": "my-pod" }
        })
    }

    #[test]
    fn test_all_running_all_ready_no_warnings() {
        let pods: Vec<_> = (0..5)
            .map(|i| make_pod(&format!("pod-{i}"), "default", "Running"))
            .collect();
        let nodes = vec![make_node("node-1", true), make_node("node-2", true)];
        let events: Vec<serde_json::Value> = vec![];

        let health = compute_health(&pods, &nodes, &events);

        // All nodes ready (30) + all pods running (40) + base 30 = 100
        assert_eq!(health.score, 100);
        assert_eq!(health.pods.running, 5);
        assert_eq!(health.pods.failed, 0);
        assert_eq!(health.pods.pending, 0);
        assert_eq!(health.pods.crash_looping, 0);
        assert_eq!(health.pods.total, 5);
        assert_eq!(health.nodes.len(), 2);
        assert!(health.nodes.iter().all(|n| n.status == "Ready"));
    }

    #[test]
    fn test_all_pods_failed() {
        let pods: Vec<_> = (0..5)
            .map(|i| make_pod(&format!("pod-{i}"), "default", "Failed"))
            .collect();
        let nodes = vec![make_node("node-1", true)];
        let events: Vec<serde_json::Value> = vec![];

        let health = compute_health(&pods, &nodes, &events);

        assert_eq!(health.pods.failed, 5);
        assert_eq!(health.pods.running, 0);
        // Score: node_score = 30, pod_score = 0, base = 30 → 60
        assert_eq!(health.score, 60);
    }

    #[test]
    fn test_mixed_pod_states() {
        let pods = vec![
            make_pod("pod-1", "default", "Running"),
            make_pod("pod-2", "default", "Running"),
            make_pod("pod-3", "default", "Pending"),
            make_crashloop_pod("pod-4", "default"),
        ];
        let nodes = vec![make_node("node-1", true)];
        let events: Vec<serde_json::Value> = vec![];

        let health = compute_health(&pods, &nodes, &events);

        assert_eq!(health.pods.running, 2);
        assert_eq!(health.pods.pending, 1);
        assert_eq!(health.pods.crash_looping, 1);
        assert_eq!(health.pods.total, 4);

        // crash_penalty = 5, pending_penalty = 2
        // node_score = 30, pod_score = (2/4)*40 = 20, base = 30
        // score = 30 + 20 + 30 - 5 - 2 = 73
        assert_eq!(health.score, 73);
    }

    #[test]
    fn test_crashloop_detection() {
        let pods = vec![
            make_crashloop_pod("crash-1", "default"),
            make_crashloop_pod("crash-2", "default"),
        ];
        let health = compute_health(&pods, &[], &[]);

        assert_eq!(health.pods.crash_looping, 2);
        assert_eq!(health.pods.running, 0);
    }

    #[test]
    fn test_pending_pod_reason_extraction() {
        let pod = json!({
            "metadata": { "name": "pending-pod", "namespace": "default", "creationTimestamp": "2026-03-16T10:00:00Z" },
            "status": {
                "phase": "Pending",
                "conditions": [{
                    "type": "PodScheduled",
                    "status": "False",
                    "reason": "Unschedulable"
                }]
            }
        });
        let health = compute_health(&[pod], &[], &[]);

        assert_eq!(health.pending_pods.len(), 1);
        assert_eq!(health.pending_pods[0].reason, "Unschedulable");
        assert_eq!(health.pending_pods[0].name, "pending-pod");
    }

    #[test]
    fn test_restart_hotlist_sorted_and_truncated() {
        let pods: Vec<_> = (0..15)
            .map(|i| make_pod_with_restarts(&format!("pod-{i}"), "default", "Running", i as i64))
            .collect();

        let health = compute_health(&pods, &[], &[]);

        // Should be sorted descending and truncated to 10
        assert_eq!(health.restart_hotlist.len(), 10);
        assert_eq!(health.restart_hotlist[0].restarts, 14);
        assert_eq!(health.restart_hotlist[9].restarts, 5);

        // Verify sorted descending
        for i in 0..health.restart_hotlist.len() - 1 {
            assert!(health.restart_hotlist[i].restarts >= health.restart_hotlist[i + 1].restarts);
        }
    }

    #[test]
    fn test_node_health_ready_vs_not_ready() {
        let nodes = vec![
            make_node("node-1", true),
            make_node("node-2", false),
            make_node("node-3", true),
        ];
        let health = compute_health(&[], &nodes, &[]);

        assert_eq!(health.nodes.len(), 3);
        let ready_count = health.nodes.iter().filter(|n| n.status == "Ready").count();
        let not_ready_count = health
            .nodes
            .iter()
            .filter(|n| n.status == "NotReady")
            .count();
        assert_eq!(ready_count, 2);
        assert_eq!(not_ready_count, 1);
    }

    #[test]
    fn test_warning_events_filtered_and_limited() {
        let mut events: Vec<serde_json::Value> = (0..25)
            .map(|i| make_warning_event(&format!("Reason{i}"), &format!("msg {i}")))
            .collect();
        // Add some normal events that should be filtered out
        events.push(make_normal_event("Scheduled"));
        events.push(make_normal_event("Pulled"));

        let health = compute_health(&[], &[], &events);

        // Should only have 20 warnings (limited)
        assert_eq!(health.recent_warnings.len(), 20);
        // All should be from warning events
        assert!(health
            .recent_warnings
            .iter()
            .all(|w| w.reason.starts_with("Reason")));
    }

    #[test]
    fn test_empty_inputs_base_score() {
        let health = compute_health(&[], &[], &[]);

        assert_eq!(health.pods.total, 0);
        assert_eq!(health.nodes.len(), 0);
        assert_eq!(health.recent_warnings.len(), 0);
        assert_eq!(health.restart_hotlist.len(), 0);
        assert_eq!(health.pending_pods.len(), 0);
        // node_score = 0 (no nodes), pod_score = 0 (no pods), base = 30
        assert_eq!(health.score, 30);
    }

    #[test]
    fn test_score_clamping() {
        // Many crash loops and pending pods to drive score very low
        let pods: Vec<_> = (0..20)
            .map(|i| make_crashloop_pod(&format!("crash-{i}"), "default"))
            .collect();
        let nodes = vec![make_node("node-1", false)];
        let events: Vec<serde_json::Value> = (0..30)
            .map(|i| make_warning_event(&format!("R{i}"), "msg"))
            .collect();

        let health = compute_health(&pods, &nodes, &events);

        // Score should be clamped to 0-100 (u32 is always >= 0)
        assert!(health.score <= 100);
    }
}

/// Fetch resources using a specific client (for multi-cluster). Returns empty vec on error.
async fn fetch_resources_with_client(
    client: &Client,
    kind: &ResourceKind,
) -> Vec<serde_json::Value> {
    use kube::api::{Api, ListParams, ResourceExt};
    use serde_json::json;

    let lp = ListParams::default().limit(MAX_DASHBOARD_RESOURCES);

    let result = match kind {
        ResourceKind::Pods => {
            let api: Api<k8s_openapi::api::core::v1::Pod> = Api::all(client.clone());
            api.list(&lp).await.map(|list| {
                list.items
                    .iter()
                    .map(|obj| {
                        serde_json::to_value(obj)
                            .unwrap_or_else(|_| json!({ "name": obj.name_any() }))
                    })
                    .collect()
            })
        }
        ResourceKind::Nodes => {
            let api: Api<k8s_openapi::api::core::v1::Node> = Api::all(client.clone());
            api.list(&lp).await.map(|list| {
                list.items
                    .iter()
                    .map(|obj| {
                        serde_json::to_value(obj)
                            .unwrap_or_else(|_| json!({ "name": obj.name_any() }))
                    })
                    .collect()
            })
        }
        ResourceKind::Events => {
            let api: Api<k8s_openapi::api::core::v1::Event> = Api::all(client.clone());
            api.list(&lp).await.map(|list| {
                list.items
                    .iter()
                    .map(|obj| {
                        serde_json::to_value(obj)
                            .unwrap_or_else(|_| json!({ "name": obj.name_any() }))
                    })
                    .collect()
            })
        }
        _ => return Vec::new(),
    };

    result
        .map_err(|e| {
            warn!(kind = ?kind, error = %e, "Failed to fetch resources for dashboard");
            e
        })
        .unwrap_or_default()
}
