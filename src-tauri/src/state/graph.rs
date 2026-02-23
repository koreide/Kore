use crate::error::{K8sError, Result};
use crate::state::{K8sState, ResourceKind};
use kube::api::{Api, ListParams};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct GraphNode {
    pub id: String,
    pub name: String,
    pub namespace: String,
    pub kind: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub relation: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResourceGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

fn make_id(kind: &str, ns: &str, name: &str) -> String {
    format!("{kind}/{ns}/{name}")
}

impl K8sState {
    /// Build a resource dependency graph for a namespace (or all namespaces).
    pub async fn build_resource_graph(&self, namespace: Option<String>) -> Result<ResourceGraph> {
        let (pods, deployments, services, ingresses, jobs, cronjobs, replicasets) = tokio::join!(
            self.list_resources(ResourceKind::Pods, namespace.clone(), None),
            self.list_resources(ResourceKind::Deployments, namespace.clone(), None),
            self.list_resources(ResourceKind::Services, namespace.clone(), None),
            self.list_resources(ResourceKind::Ingresses, namespace.clone(), None),
            self.list_resources(ResourceKind::Jobs, namespace.clone(), None),
            self.list_resources(ResourceKind::Cronjobs, namespace.clone(), None),
            self.list_replicasets(namespace.clone()),
        );

        let mut nodes = Vec::new();
        let mut edges = Vec::new();

        // Helper to extract pod labels
        let pod_labels: Vec<(
            String,
            String,
            String,
            std::collections::HashMap<String, String>,
        )> = pods
            .as_ref()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|p| {
                let name = p.pointer("/metadata/name")?.as_str()?.to_string();
                let ns = p
                    .pointer("/metadata/namespace")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let status = p
                    .pointer("/status/phase")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown")
                    .to_string();
                let labels = p
                    .pointer("/metadata/labels")
                    .and_then(|v| {
                        serde_json::from_value::<std::collections::HashMap<String, String>>(
                            v.clone(),
                        )
                        .ok()
                    })
                    .unwrap_or_default();
                Some((name, ns, status, labels))
            })
            .collect();

        // Add pod nodes
        for (name, ns, status, _) in &pod_labels {
            nodes.push(GraphNode {
                id: make_id("Pod", ns, name),
                name: name.clone(),
                namespace: ns.clone(),
                kind: "Pod".to_string(),
                status: status.clone(),
            });
        }

        // Add deployment nodes + edges to ReplicaSets
        for deploy in deployments.as_ref().unwrap_or(&vec![]) {
            let name = deploy
                .pointer("/metadata/name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let ns = deploy
                .pointer("/metadata/namespace")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let ready = deploy
                .pointer("/status/readyReplicas")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let replicas = deploy
                .pointer("/status/replicas")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let status = if ready == replicas && replicas > 0 {
                "Ready"
            } else {
                "Progressing"
            };

            nodes.push(GraphNode {
                id: make_id("Deployment", ns, name),
                name: name.to_string(),
                namespace: ns.to_string(),
                kind: "Deployment".to_string(),
                status: status.to_string(),
            });
        }

        // Add ReplicaSet nodes + edges (Deployment -> RS, RS -> Pods via ownerRef)
        for rs in replicasets.as_ref().unwrap_or(&vec![]) {
            let rs_name = rs
                .pointer("/metadata/name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let rs_ns = rs
                .pointer("/metadata/namespace")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let rs_replicas = rs
                .pointer("/status/replicas")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            if rs_replicas == 0 {
                continue; // Skip inactive ReplicaSets
            }

            nodes.push(GraphNode {
                id: make_id("ReplicaSet", rs_ns, rs_name),
                name: rs_name.to_string(),
                namespace: rs_ns.to_string(),
                kind: "ReplicaSet".to_string(),
                status: "Active".to_string(),
            });

            // Deployment -> ReplicaSet edges
            if let Some(owners) = rs
                .pointer("/metadata/ownerReferences")
                .and_then(|v| v.as_array())
            {
                for owner in owners {
                    if owner.get("kind").and_then(|v| v.as_str()) == Some("Deployment") {
                        let owner_name = owner.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        edges.push(GraphEdge {
                            source: make_id("Deployment", rs_ns, owner_name),
                            target: make_id("ReplicaSet", rs_ns, rs_name),
                            relation: "owns".to_string(),
                        });
                    }
                }
            }
        }

        // Pod ownerReference edges (RS -> Pod, Job -> Pod)
        for pod in pods.as_ref().unwrap_or(&vec![]) {
            let pod_name = pod
                .pointer("/metadata/name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let pod_ns = pod
                .pointer("/metadata/namespace")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if let Some(owners) = pod
                .pointer("/metadata/ownerReferences")
                .and_then(|v| v.as_array())
            {
                for owner in owners {
                    let owner_kind = owner.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                    let owner_name = owner.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    edges.push(GraphEdge {
                        source: make_id(owner_kind, pod_ns, owner_name),
                        target: make_id("Pod", pod_ns, pod_name),
                        relation: "owns".to_string(),
                    });
                }
            }
        }

        // Service nodes + edges (Service -> Pods by selector)
        for svc in services.as_ref().unwrap_or(&vec![]) {
            let name = svc
                .pointer("/metadata/name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let ns = svc
                .pointer("/metadata/namespace")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let svc_type = svc
                .pointer("/spec/type")
                .and_then(|v| v.as_str())
                .unwrap_or("ClusterIP");

            nodes.push(GraphNode {
                id: make_id("Service", ns, name),
                name: name.to_string(),
                namespace: ns.to_string(),
                kind: "Service".to_string(),
                status: svc_type.to_string(),
            });

            // Match selector to pod labels
            if let Some(selector) = svc.pointer("/spec/selector").and_then(|v| {
                serde_json::from_value::<std::collections::HashMap<String, String>>(v.clone()).ok()
            }) {
                for (pod_name, pod_ns, _, labels) in &pod_labels {
                    if pod_ns == ns
                        && selector
                            .iter()
                            .all(|(k, v)| labels.get(k).map(|lv| lv == v).unwrap_or(false))
                    {
                        edges.push(GraphEdge {
                            source: make_id("Service", ns, name),
                            target: make_id("Pod", pod_ns, pod_name),
                            relation: "selects".to_string(),
                        });
                    }
                }
            }
        }

        // Ingress nodes + edges (Ingress -> Service)
        for ing in ingresses.as_ref().unwrap_or(&vec![]) {
            let name = ing
                .pointer("/metadata/name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let ns = ing
                .pointer("/metadata/namespace")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            nodes.push(GraphNode {
                id: make_id("Ingress", ns, name),
                name: name.to_string(),
                namespace: ns.to_string(),
                kind: "Ingress".to_string(),
                status: "Active".to_string(),
            });

            if let Some(rules) = ing.pointer("/spec/rules").and_then(|v| v.as_array()) {
                for rule in rules {
                    if let Some(paths) = rule.pointer("/http/paths").and_then(|v| v.as_array()) {
                        for path in paths {
                            if let Some(svc_name) = path
                                .pointer("/backend/service/name")
                                .and_then(|v| v.as_str())
                            {
                                edges.push(GraphEdge {
                                    source: make_id("Ingress", ns, name),
                                    target: make_id("Service", ns, svc_name),
                                    relation: "routes".to_string(),
                                });
                            }
                        }
                    }
                }
            }
        }

        // Job nodes + CronJob -> Job edges
        for job in jobs.as_ref().unwrap_or(&vec![]) {
            let name = job
                .pointer("/metadata/name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let ns = job
                .pointer("/metadata/namespace")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let status = job
                .pointer("/status/conditions")
                .and_then(|v| v.as_array())
                .and_then(|c| {
                    c.iter().find_map(|cond| {
                        if cond.get("status")?.as_str()? == "True" {
                            cond.get("type")?.as_str().map(String::from)
                        } else {
                            None
                        }
                    })
                })
                .unwrap_or_else(|| "Running".to_string());

            nodes.push(GraphNode {
                id: make_id("Job", ns, name),
                name: name.to_string(),
                namespace: ns.to_string(),
                kind: "Job".to_string(),
                status,
            });

            if let Some(owners) = job
                .pointer("/metadata/ownerReferences")
                .and_then(|v| v.as_array())
            {
                for owner in owners {
                    if owner.get("kind").and_then(|v| v.as_str()) == Some("CronJob") {
                        let owner_name = owner.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        edges.push(GraphEdge {
                            source: make_id("CronJob", ns, owner_name),
                            target: make_id("Job", ns, name),
                            relation: "owns".to_string(),
                        });
                    }
                }
            }
        }

        // CronJob nodes
        for cj in cronjobs.as_ref().unwrap_or(&vec![]) {
            let name = cj
                .pointer("/metadata/name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let ns = cj
                .pointer("/metadata/namespace")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            nodes.push(GraphNode {
                id: make_id("CronJob", ns, name),
                name: name.to_string(),
                namespace: ns.to_string(),
                kind: "CronJob".to_string(),
                status: "Active".to_string(),
            });
        }

        Ok(ResourceGraph { nodes, edges })
    }

    /// List ReplicaSets (internal helper for graph building).
    async fn list_replicasets(&self, namespace: Option<String>) -> Result<Vec<serde_json::Value>> {
        let client = self.current_client().await?;
        let api: Api<k8s_openapi::api::apps::v1::ReplicaSet> = match namespace {
            Some(ns) => Api::namespaced(client, &ns),
            None => Api::all(client),
        };

        let list = api
            .list(&ListParams::default())
            .await
            .map_err(K8sError::Kube)?;

        let items: Vec<serde_json::Value> = list
            .items
            .iter()
            .map(|obj| serde_json::to_value(obj).unwrap_or_else(|_| serde_json::json!({})))
            .collect();

        Ok(items)
    }
}
