use crate::error::{K8sError, Result};
use crate::state::K8sState;
use k8s_openapi::api::apps::v1::ReplicaSet;
use k8s_openapi::api::core::v1::{Namespace, Pod};
use k8s_openapi::api::networking::v1::{NetworkPolicy, NetworkPolicyPeer, NetworkPolicyPort};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelector;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kube::api::{Api, ListParams};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

// ── Data Structures ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct NetworkPolicyGraph {
    pub groups: Vec<PodGroup>,
    pub external_cidrs: Vec<CidrNode>,
    pub edges: Vec<TrafficEdge>,
    pub policies: Vec<PolicySummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PodGroup {
    pub id: String,
    pub name: String,
    pub namespace: String,
    pub kind: String,
    pub pod_count: usize,
    pub labels: HashMap<String, String>,
    pub is_isolated_ingress: bool,
    pub is_isolated_egress: bool,
    pub matching_policies: Vec<String>,
    pub pods: Vec<PodInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PodInfo {
    pub name: String,
    pub namespace: String,
    pub labels: HashMap<String, String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CidrNode {
    pub id: String,
    pub cidr: String,
    pub except: Vec<String>,
    pub from_policy: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrafficEdge {
    pub source: String,
    pub target: String,
    pub direction: String,
    pub ports: Vec<PortInfo>,
    pub policy_name: String,
    pub policy_namespace: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PortInfo {
    pub port: Option<i32>,
    pub named_port: Option<String>,
    pub protocol: String,
    pub end_port: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PolicySummary {
    pub name: String,
    pub namespace: String,
    pub pod_selector: HashMap<String, String>,
    pub policy_types: Vec<String>,
    pub ingress_rule_count: usize,
    pub egress_rule_count: usize,
    pub affected_pod_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrafficSimulationResult {
    pub allowed: bool,
    pub ingress_evaluation: DirectionEvaluation,
    pub egress_evaluation: DirectionEvaluation,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DirectionEvaluation {
    pub isolated: bool,
    pub policy_results: Vec<PolicyEvaluation>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PolicyEvaluation {
    pub policy_name: String,
    pub policy_namespace: String,
    pub selects_pod: bool,
    pub allows_traffic: bool,
    pub reason: String,
    pub matching_rule_index: Option<usize>,
}

// ── Helpers ──────────────────────────────────────────────────────────

fn btree_to_hash(m: &BTreeMap<String, String>) -> HashMap<String, String> {
    m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// Match pod labels against a Kubernetes LabelSelector (matchLabels + matchExpressions).
fn matches_label_selector(labels: &HashMap<String, String>, selector: &LabelSelector) -> bool {
    if let Some(match_labels) = &selector.match_labels {
        for (k, v) in match_labels {
            match labels.get(k) {
                Some(actual) if actual == v => {}
                _ => return false,
            }
        }
    }
    if let Some(expressions) = &selector.match_expressions {
        for expr in expressions {
            let label_value = labels.get(&expr.key);
            let values: std::collections::HashSet<&str> = expr
                .values
                .as_ref()
                .map(|v| v.iter().map(|s| s.as_str()).collect())
                .unwrap_or_default();

            match expr.operator.as_str() {
                "In" => match label_value {
                    Some(val) if values.contains(val.as_str()) => {}
                    _ => return false,
                },
                "NotIn" => {
                    if let Some(val) = label_value {
                        if values.contains(val.as_str()) {
                            return false;
                        }
                    }
                }
                "Exists" => {
                    if label_value.is_none() {
                        return false;
                    }
                }
                "DoesNotExist" => {
                    if label_value.is_some() {
                        return false;
                    }
                }
                _ => {}
            }
        }
    }
    true
}

fn extract_ports(ports: Option<&[NetworkPolicyPort]>) -> Vec<PortInfo> {
    match ports {
        Some(ports) => ports
            .iter()
            .map(|p| {
                let (port, named_port) = match p.port.as_ref() {
                    Some(IntOrString::Int(i)) => (Some(*i), None),
                    Some(IntOrString::String(s)) => (None, Some(s.clone())),
                    None => (None, None),
                };
                PortInfo {
                    port,
                    named_port,
                    protocol: p.protocol.clone().unwrap_or_else(|| "TCP".to_string()),
                    end_port: p.end_port,
                }
            })
            .collect(),
        None => vec![],
    }
}

/// Resolve a NetworkPolicyPeer to matching PodGroup IDs.
fn resolve_peer_groups(
    peer: &NetworkPolicyPeer,
    groups: &[PodGroup],
    ns_labels: &HashMap<String, HashMap<String, String>>,
    policy_ns: &str,
) -> Vec<String> {
    if peer.ip_block.is_some() {
        return vec![];
    }

    groups
        .iter()
        .filter(|g| {
            let ns_match = if let Some(ns_sel) = &peer.namespace_selector {
                ns_labels
                    .get(&g.namespace)
                    .is_some_and(|labels| matches_label_selector(labels, ns_sel))
            } else {
                g.namespace == policy_ns
            };
            if !ns_match {
                return false;
            }
            if let Some(pod_sel) = &peer.pod_selector {
                g.pods
                    .iter()
                    .any(|p| matches_label_selector(&p.labels, pod_sel))
            } else {
                true
            }
        })
        .map(|g| g.id.clone())
        .collect()
}

/// Check if a peer matches a specific target pod.
fn peer_matches_target(
    peer: &NetworkPolicyPeer,
    target_labels: &HashMap<String, String>,
    target_ns: &str,
    ns_labels: &HashMap<String, HashMap<String, String>>,
    policy_ns: &str,
) -> bool {
    if peer.ip_block.is_some() {
        return false;
    }
    let ns_match = if let Some(ns_sel) = &peer.namespace_selector {
        ns_labels
            .get(target_ns)
            .is_some_and(|labels| matches_label_selector(labels, ns_sel))
    } else {
        target_ns == policy_ns
    };
    if !ns_match {
        return false;
    }
    if let Some(pod_sel) = &peer.pod_selector {
        matches_label_selector(target_labels, pod_sel)
    } else {
        true
    }
}

fn port_matches(target_port: Option<i32>, target_protocol: &str, policy_ports: &[PortInfo]) -> bool {
    if policy_ports.is_empty() {
        return true;
    }
    policy_ports.iter().any(|pp| {
        if !pp.protocol.eq_ignore_ascii_case(target_protocol) {
            return false;
        }
        // Named ports cannot be resolved without pod container spec; treat as
        // non-matching to avoid silently allowing all traffic.
        if pp.named_port.is_some() {
            return false;
        }
        match (pp.port, pp.end_port, target_port) {
            (None, _, _) => true,
            (Some(p), None, Some(tp)) => p == tp,
            (Some(start), Some(end), Some(tp)) => tp >= start && tp <= end,
            (Some(_), _, None) => false,
        }
    })
}

fn infer_policy_types(spec: &k8s_openapi::api::networking::v1::NetworkPolicySpec) -> Vec<String> {
    spec.policy_types.clone().unwrap_or_else(|| {
        let mut types = vec![];
        if spec.ingress.is_some() {
            types.push("Ingress".to_string());
        }
        if spec.egress.is_some() {
            types.push("Egress".to_string());
        }
        if types.is_empty() {
            types.push("Ingress".to_string());
        }
        types
    })
}

// ── Graph Builder ────────────────────────────────────────────────────

impl K8sState {
    pub async fn build_network_policy_graph(
        &self,
        namespace: Option<String>,
    ) -> Result<NetworkPolicyGraph> {
        let client = self.current_client().await?;

        let np_api: Api<NetworkPolicy> = match &namespace {
            Some(ns) => Api::namespaced(client.clone(), ns),
            None => Api::all(client.clone()),
        };
        let pod_api: Api<Pod> = match &namespace {
            Some(ns) => Api::namespaced(client.clone(), ns),
            None => Api::all(client.clone()),
        };
        let rs_api: Api<ReplicaSet> = match &namespace {
            Some(ns) => Api::namespaced(client.clone(), ns),
            None => Api::all(client.clone()),
        };
        let ns_api: Api<Namespace> = Api::all(client.clone());

        let lp = ListParams::default();
        let (np_result, pod_result, rs_result, ns_result) = tokio::join!(
            np_api.list(&lp),
            pod_api.list(&lp),
            rs_api.list(&lp),
            ns_api.list(&lp),
        );

        let network_policies = np_result.map_err(K8sError::Kube)?.items;
        let pods = pod_result.map_err(K8sError::Kube)?.items;
        let replica_sets = rs_result.map_err(K8sError::Kube)?.items;
        let namespaces = ns_result.map_err(K8sError::Kube)?.items;

        // Namespace label lookup
        let ns_labels: HashMap<String, HashMap<String, String>> = namespaces
            .iter()
            .filter_map(|ns| {
                let name = ns.metadata.name.as_ref()?;
                let labels = ns.metadata.labels.as_ref().map(btree_to_hash).unwrap_or_default();
                Some((name.clone(), labels))
            })
            .collect();

        // RS -> Deployment owner lookup
        let rs_to_deploy: HashMap<String, (String, String)> = replica_sets
            .iter()
            .filter_map(|rs| {
                let rs_name = rs.metadata.name.as_ref()?;
                let rs_ns = rs.metadata.namespace.as_deref().unwrap_or("default");
                let owner = rs
                    .metadata
                    .owner_references
                    .as_ref()?
                    .iter()
                    .find(|o| o.kind == "Deployment")?;
                Some((
                    format!("{rs_ns}/{rs_name}"),
                    (rs_ns.to_string(), owner.name.clone()),
                ))
            })
            .collect();

        // Group pods by top-level owner
        let mut group_map: HashMap<String, PodGroup> = HashMap::new();

        for pod in &pods {
            let pod_name = pod.metadata.name.as_deref().unwrap_or("");
            let pod_ns = pod.metadata.namespace.as_deref().unwrap_or("default");
            let pod_labels = pod
                .metadata
                .labels
                .as_ref()
                .map(btree_to_hash)
                .unwrap_or_default();
            let pod_status = pod
                .status
                .as_ref()
                .and_then(|s| s.phase.as_deref())
                .unwrap_or("Unknown")
                .to_string();

            let pod_info = PodInfo {
                name: pod_name.to_string(),
                namespace: pod_ns.to_string(),
                labels: pod_labels.clone(),
                status: pod_status,
            };

            let (group_key, group_name, group_kind) =
                if let Some(owners) = &pod.metadata.owner_references {
                    if let Some(owner) = owners.first() {
                        match owner.kind.as_str() {
                            "ReplicaSet" => {
                                let rs_key = format!("{pod_ns}/{}", owner.name);
                                if let Some((_, deploy_name)) = rs_to_deploy.get(&rs_key) {
                                    (
                                        format!("Deployment/{pod_ns}/{deploy_name}"),
                                        deploy_name.clone(),
                                        "Deployment".to_string(),
                                    )
                                } else {
                                    (
                                        format!("ReplicaSet/{rs_key}"),
                                        owner.name.clone(),
                                        "ReplicaSet".to_string(),
                                    )
                                }
                            }
                            kind => (
                                format!("{kind}/{pod_ns}/{}", owner.name),
                                owner.name.clone(),
                                kind.to_string(),
                            ),
                        }
                    } else {
                        (
                            format!("Pod/{pod_ns}/{pod_name}"),
                            pod_name.to_string(),
                            "Pod".to_string(),
                        )
                    }
                } else {
                    (
                        format!("Pod/{pod_ns}/{pod_name}"),
                        pod_name.to_string(),
                        "Pod".to_string(),
                    )
                };

            let group = group_map
                .entry(group_key.clone())
                .or_insert_with(|| PodGroup {
                    id: group_key,
                    name: group_name,
                    namespace: pod_ns.to_string(),
                    kind: group_kind,
                    pod_count: 0,
                    labels: pod_labels.clone(),
                    is_isolated_ingress: false,
                    is_isolated_egress: false,
                    matching_policies: Vec::new(),
                    pods: Vec::new(),
                });
            group.pod_count += 1;
            group.pods.push(pod_info);
        }

        let mut groups: Vec<PodGroup> = group_map.into_values().collect();
        groups.sort_by(|a, b| a.id.cmp(&b.id));

        let mut edges: Vec<TrafficEdge> = Vec::new();
        let mut cidr_nodes: Vec<CidrNode> = Vec::new();
        let mut policies: Vec<PolicySummary> = Vec::new();
        let mut cidr_counter = 0usize;

        for np in &network_policies {
            let np_name = np.metadata.name.as_deref().unwrap_or("");
            let np_ns = np.metadata.namespace.as_deref().unwrap_or("default");

            let spec = match &np.spec {
                Some(s) => s,
                None => continue,
            };

            let policy_types = infer_policy_types(spec);

            let pod_selector_labels = spec
                .pod_selector
                .match_labels
                .as_ref()
                .map(btree_to_hash)
                .unwrap_or_default();

            // Find affected groups
            let mut affected_count = 0usize;
            let affected_group_ids: Vec<String> = groups
                .iter()
                .filter(|g| {
                    g.namespace == np_ns
                        && g.pods
                            .iter()
                            .any(|p| matches_label_selector(&p.labels, &spec.pod_selector))
                })
                .map(|g| {
                    affected_count += g.pod_count;
                    g.id.clone()
                })
                .collect();

            // Mark isolation and matching policies
            for group in groups.iter_mut() {
                if affected_group_ids.contains(&group.id) {
                    if policy_types.contains(&"Ingress".to_string()) {
                        group.is_isolated_ingress = true;
                    }
                    if policy_types.contains(&"Egress".to_string()) {
                        group.is_isolated_egress = true;
                    }
                    if !group.matching_policies.contains(&np_name.to_string()) {
                        group.matching_policies.push(np_name.to_string());
                    }
                }
            }

            // Process ingress rules
            let ingress_rule_count = spec.ingress.as_ref().map_or(0, |r| r.len());
            if let Some(ingress_rules) = &spec.ingress {
                for rule in ingress_rules {
                    let ports = extract_ports(rule.ports.as_deref());

                    if let Some(from_peers) = &rule.from {
                        for peer in from_peers {
                            if let Some(ip_block) = &peer.ip_block {
                                let cidr_id = format!("cidr-{cidr_counter}");
                                cidr_counter += 1;
                                cidr_nodes.push(CidrNode {
                                    id: cidr_id.clone(),
                                    cidr: ip_block.cidr.clone(),
                                    except: ip_block.except.clone().unwrap_or_default(),
                                    from_policy: np_name.to_string(),
                                });
                                for target_id in &affected_group_ids {
                                    edges.push(TrafficEdge {
                                        source: cidr_id.clone(),
                                        target: target_id.clone(),
                                        direction: "ingress".to_string(),
                                        ports: ports.clone(),
                                        policy_name: np_name.to_string(),
                                        policy_namespace: np_ns.to_string(),
                                    });
                                }
                            } else {
                                let source_groups =
                                    resolve_peer_groups(peer, &groups, &ns_labels, np_ns);
                                for source_id in &source_groups {
                                    for target_id in &affected_group_ids {
                                        if source_id != target_id {
                                            edges.push(TrafficEdge {
                                                source: source_id.clone(),
                                                target: target_id.clone(),
                                                direction: "ingress".to_string(),
                                                ports: ports.clone(),
                                                policy_name: np_name.to_string(),
                                                policy_namespace: np_ns.to_string(),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        // No `from` = allow from everywhere
                        let cidr_id = format!("cidr-{cidr_counter}");
                        cidr_counter += 1;
                        cidr_nodes.push(CidrNode {
                            id: cidr_id.clone(),
                            cidr: "0.0.0.0/0".to_string(),
                            except: vec![],
                            from_policy: np_name.to_string(),
                        });
                        for target_id in &affected_group_ids {
                            edges.push(TrafficEdge {
                                source: cidr_id.clone(),
                                target: target_id.clone(),
                                direction: "ingress".to_string(),
                                ports: ports.clone(),
                                policy_name: np_name.to_string(),
                                policy_namespace: np_ns.to_string(),
                            });
                        }
                    }
                }
            }

            // Process egress rules
            let egress_rule_count = spec.egress.as_ref().map_or(0, |r| r.len());
            if let Some(egress_rules) = &spec.egress {
                for rule in egress_rules {
                    let ports = extract_ports(rule.ports.as_deref());

                    if let Some(to_peers) = &rule.to {
                        for peer in to_peers {
                            if let Some(ip_block) = &peer.ip_block {
                                let cidr_id = format!("cidr-{cidr_counter}");
                                cidr_counter += 1;
                                cidr_nodes.push(CidrNode {
                                    id: cidr_id.clone(),
                                    cidr: ip_block.cidr.clone(),
                                    except: ip_block.except.clone().unwrap_or_default(),
                                    from_policy: np_name.to_string(),
                                });
                                for source_id in &affected_group_ids {
                                    edges.push(TrafficEdge {
                                        source: source_id.clone(),
                                        target: cidr_id.clone(),
                                        direction: "egress".to_string(),
                                        ports: ports.clone(),
                                        policy_name: np_name.to_string(),
                                        policy_namespace: np_ns.to_string(),
                                    });
                                }
                            } else {
                                let target_groups =
                                    resolve_peer_groups(peer, &groups, &ns_labels, np_ns);
                                for source_id in &affected_group_ids {
                                    for target_id in &target_groups {
                                        if source_id != target_id {
                                            edges.push(TrafficEdge {
                                                source: source_id.clone(),
                                                target: target_id.clone(),
                                                direction: "egress".to_string(),
                                                ports: ports.clone(),
                                                policy_name: np_name.to_string(),
                                                policy_namespace: np_ns.to_string(),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        // No `to` = allow to everywhere
                        let cidr_id = format!("cidr-{cidr_counter}");
                        cidr_counter += 1;
                        cidr_nodes.push(CidrNode {
                            id: cidr_id.clone(),
                            cidr: "0.0.0.0/0".to_string(),
                            except: vec![],
                            from_policy: np_name.to_string(),
                        });
                        for source_id in &affected_group_ids {
                            edges.push(TrafficEdge {
                                source: source_id.clone(),
                                target: cidr_id.clone(),
                                direction: "egress".to_string(),
                                ports: ports.clone(),
                                policy_name: np_name.to_string(),
                                policy_namespace: np_ns.to_string(),
                            });
                        }
                    }
                }
            }

            policies.push(PolicySummary {
                name: np_name.to_string(),
                namespace: np_ns.to_string(),
                pod_selector: pod_selector_labels,
                policy_types,
                ingress_rule_count,
                egress_rule_count,
                affected_pod_count: affected_count,
            });
        }

        Ok(NetworkPolicyGraph {
            groups,
            external_cidrs: cidr_nodes,
            edges,
            policies,
        })
    }

    // ── Traffic Simulation ───────────────────────────────────────────

    pub async fn simulate_network_traffic(
        &self,
        source_namespace: String,
        source_pod: String,
        dest_namespace: String,
        dest_pod: String,
        port: Option<i32>,
        protocol: Option<String>,
    ) -> Result<TrafficSimulationResult> {
        let client = self.current_client().await?;
        let protocol = protocol.unwrap_or_else(|| "TCP".to_string());

        let src_api: Api<Pod> = Api::namespaced(client.clone(), &source_namespace);
        let dst_api: Api<Pod> = Api::namespaced(client.clone(), &dest_namespace);

        let (src_result, dst_result) = tokio::join!(
            src_api.get(&source_pod),
            dst_api.get(&dest_pod),
        );

        let src = src_result.map_err(K8sError::Kube)?;
        let dst = dst_result.map_err(K8sError::Kube)?;

        let src_labels = src.metadata.labels.as_ref().map(btree_to_hash).unwrap_or_default();
        let dst_labels = dst.metadata.labels.as_ref().map(btree_to_hash).unwrap_or_default();

        // Fetch NPs and namespaces
        let src_np_api: Api<NetworkPolicy> = Api::namespaced(client.clone(), &source_namespace);
        let dst_np_api: Api<NetworkPolicy> = Api::namespaced(client.clone(), &dest_namespace);
        let ns_api: Api<Namespace> = Api::all(client.clone());

        let lp = ListParams::default();
        let (src_nps, dst_nps, all_ns) = tokio::join!(
            src_np_api.list(&lp),
            dst_np_api.list(&lp),
            ns_api.list(&lp),
        );

        let src_nps = src_nps.map_err(K8sError::Kube)?.items;
        let dst_nps = dst_nps.map_err(K8sError::Kube)?.items;
        let all_ns = all_ns.map_err(K8sError::Kube)?.items;

        let ns_labels: HashMap<String, HashMap<String, String>> = all_ns
            .iter()
            .filter_map(|ns| {
                let name = ns.metadata.name.as_ref()?;
                let labels = ns.metadata.labels.as_ref().map(btree_to_hash).unwrap_or_default();
                Some((name.clone(), labels))
            })
            .collect();

        let egress_evaluation = evaluate_direction(
            &src_nps,
            &src_labels,
            &source_namespace,
            &dst_labels,
            &dest_namespace,
            &ns_labels,
            port,
            &protocol,
            "Egress",
        );

        let ingress_evaluation = evaluate_direction(
            &dst_nps,
            &dst_labels,
            &dest_namespace,
            &src_labels,
            &source_namespace,
            &ns_labels,
            port,
            &protocol,
            "Ingress",
        );

        let egress_allowed =
            !egress_evaluation.isolated || egress_evaluation.policy_results.iter().any(|r| r.allows_traffic);
        let ingress_allowed =
            !ingress_evaluation.isolated || ingress_evaluation.policy_results.iter().any(|r| r.allows_traffic);

        let allowed = egress_allowed && ingress_allowed;

        let summary = if allowed {
            format!(
                "Traffic ALLOWED from {source_namespace}/{source_pod} to {dest_namespace}/{dest_pod}"
            )
        } else {
            let mut reasons = Vec::new();
            if !egress_allowed {
                reasons.push("egress denied");
            }
            if !ingress_allowed {
                reasons.push("ingress denied");
            }
            format!(
                "Traffic DENIED from {}/{} to {}/{}: {}",
                source_namespace,
                source_pod,
                dest_namespace,
                dest_pod,
                reasons.join(" and ")
            )
        };

        Ok(TrafficSimulationResult {
            allowed,
            ingress_evaluation,
            egress_evaluation,
            summary,
        })
    }
}

// ── Direction Evaluation (shared for ingress/egress) ─────────────────

/// Evaluate whether traffic is allowed in a given direction.
///
/// `selected_labels` / `selected_ns` — the pod being evaluated for isolation (dest for ingress, src for egress).
/// `peer_labels` / `peer_ns` — the other end (src for ingress, dest for egress).
/// `direction` — "Ingress" or "Egress".
#[allow(clippy::too_many_arguments)]
fn evaluate_direction(
    policies: &[NetworkPolicy],
    selected_labels: &HashMap<String, String>,
    _selected_ns: &str,
    peer_labels: &HashMap<String, String>,
    peer_ns: &str,
    ns_labels: &HashMap<String, HashMap<String, String>>,
    port: Option<i32>,
    protocol: &str,
    direction: &str,
) -> DirectionEvaluation {
    let mut isolated = false;
    let mut policy_results = Vec::new();

    for np in policies {
        let np_name = np.metadata.name.as_deref().unwrap_or("");
        let np_ns = np.metadata.namespace.as_deref().unwrap_or("default");

        let spec = match &np.spec {
            Some(s) => s,
            None => continue,
        };

        let policy_types = infer_policy_types(spec);
        if !policy_types.iter().any(|t| t == direction) {
            continue;
        }

        let selects_pod = matches_label_selector(selected_labels, &spec.pod_selector);
        if !selects_pod {
            policy_results.push(PolicyEvaluation {
                policy_name: np_name.to_string(),
                policy_namespace: np_ns.to_string(),
                selects_pod: false,
                allows_traffic: false,
                reason: format!("Does not select {} pod", direction.to_lowercase()),
                matching_rule_index: None,
            });
            continue;
        }

        isolated = true;

        let mut allows = false;
        let mut matching_idx = None;
        let mut reason = format!("No matching {} rule", direction.to_lowercase());

        type RulePeersAndPorts<'a> = (Option<&'a Vec<NetworkPolicyPeer>>, Option<&'a Vec<NetworkPolicyPort>>);
        let rules: Option<Vec<RulePeersAndPorts<'_>>> = if direction == "Ingress" {
            spec.ingress.as_ref().map(|rules| {
                rules
                    .iter()
                    .map(|r| (r.from.as_ref(), r.ports.as_ref()))
                    .collect()
            })
        } else {
            spec.egress.as_ref().map(|rules| {
                rules
                    .iter()
                    .map(|r| (r.to.as_ref(), r.ports.as_ref()))
                    .collect()
            })
        };

        if let Some(rules) = rules {
            for (idx, (peers, rule_ports)) in rules.iter().enumerate() {
                let ports = extract_ports(rule_ports.map(|v| v.as_slice()));
                if !port_matches(port, protocol, &ports) {
                    continue;
                }

                if let Some(peers) = peers {
                    for peer in *peers {
                        if peer_matches_target(peer, peer_labels, peer_ns, ns_labels, np_ns) {
                            allows = true;
                            matching_idx = Some(idx);
                            reason = format!("Matched {} rule {}", direction.to_lowercase(), idx);
                            break;
                        }
                    }
                } else {
                    // No peers = allow all
                    allows = true;
                    matching_idx = Some(idx);
                    reason = format!(
                        "{} rule {} allows all {}",
                        direction,
                        idx,
                        if direction == "Ingress" {
                            "sources"
                        } else {
                            "destinations"
                        }
                    );
                }

                if allows {
                    break;
                }
            }
        }

        policy_results.push(PolicyEvaluation {
            policy_name: np_name.to_string(),
            policy_namespace: np_ns.to_string(),
            selects_pod: true,
            allows_traffic: allows,
            reason,
            matching_rule_index: matching_idx,
        });
    }

    DirectionEvaluation {
        isolated,
        policy_results,
    }
}
