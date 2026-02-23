use crate::error::Result;
use crate::state::{K8sState, ResourceKind};
use serde::Serialize;

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

impl K8sState {
    pub async fn get_cluster_health(&self) -> Result<ClusterHealth> {
        // Fetch all resources in parallel
        let (pods_result, nodes_result, events_result) = tokio::join!(
            self.list_resources(ResourceKind::Pods, None, None),
            self.list_resources(ResourceKind::Nodes, None, None),
            self.list_resources(ResourceKind::Events, None, None),
        );

        let pods = pods_result.unwrap_or_default();
        let nodes = nodes_result.unwrap_or_default();
        let events = events_result.unwrap_or_default();

        // Pod health counts
        let mut running = 0u32;
        let mut pending = 0u32;
        let mut failed = 0u32;
        let mut succeeded = 0u32;
        let mut crash_looping = 0u32;
        let mut restart_hotlist = Vec::new();
        let mut pending_pods = Vec::new();

        for pod in &pods {
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
        let node_ok = node_health
            .iter()
            .filter(|n| n.status == "Ready")
            .count() as f64;
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

        Ok(ClusterHealth {
            score,
            pods: pod_health,
            nodes: node_health,
            restart_hotlist,
            pending_pods,
            recent_warnings,
        })
    }
}
