use crate::error::{K8sError, Result};
use crate::state::K8sState;
use k8s_openapi::api::core::v1::ServiceAccount;
use k8s_openapi::api::rbac::v1::{ClusterRole, ClusterRoleBinding, Role, RoleBinding, Subject};
use kube::api::{Api, ListParams};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

// ── Identity ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(tag = "kind")]
pub enum RbacIdentity {
    User { name: String },
    Group { name: String },
    ServiceAccount { name: String, namespace: String },
}

impl std::fmt::Display for RbacIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RbacIdentity::User { name } => write!(f, "User \"{name}\""),
            RbacIdentity::Group { name } => write!(f, "Group \"{name}\""),
            RbacIdentity::ServiceAccount { name, namespace } => {
                write!(f, "ServiceAccount \"{namespace}/{name}\"")
            }
        }
    }
}

// ── Policy Rule Summary ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct PolicyRuleSummary {
    pub verbs: Vec<String>,
    pub api_groups: Vec<String>,
    pub resources: Vec<String>,
    pub resource_names: Vec<String>,
}

// ── Rule Chain Entry ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct RuleChainEntry {
    pub role_kind: String,
    pub role_name: String,
    pub role_namespace: Option<String>,
    pub binding_kind: String,
    pub binding_name: String,
    pub binding_namespace: Option<String>,
    pub matching_rule: PolicyRuleSummary,
}

// ── Permission Check ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct PermissionCheckResult {
    pub allowed: bool,
    pub identity: RbacIdentity,
    pub verb: String,
    pub resource: String,
    pub namespace: Option<String>,
    pub rule_chain: Vec<RuleChainEntry>,
    pub summary: String,
}

// ── Permission Matrix ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct PermissionMatrix {
    pub identity: RbacIdentity,
    pub namespace: Option<String>,
    pub rows: Vec<PermissionMatrixRow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PermissionMatrixRow {
    pub resource: String,
    pub api_group: String,
    pub verbs: HashMap<String, PermissionCell>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PermissionCell {
    pub status: PermissionStatus,
    pub rule_chain: Vec<RuleChainEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionStatus {
    Allowed,
    Denied,
    Conditional,
}

// ── Reverse Lookup ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct ReverseLookupResult {
    pub role_kind: String,
    pub role_name: String,
    pub role_namespace: Option<String>,
    pub rules: Vec<PolicyRuleSummary>,
    pub subjects: Vec<ReverseLookupSubject>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReverseLookupSubject {
    pub identity: RbacIdentity,
    pub binding_kind: String,
    pub binding_name: String,
    pub binding_namespace: Option<String>,
    pub scope: String,
}

// ── Role Summary ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct RoleSummary {
    pub kind: String,
    pub name: String,
    pub namespace: Option<String>,
    pub rule_count: usize,
    pub rules: Vec<PolicyRuleSummary>,
}

// ── Forbidden Analysis ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct ForbiddenAnalysis {
    pub verb: String,
    pub resource: String,
    pub namespace: Option<String>,
    pub identity: RbacIdentity,
    pub missing_permission: String,
    pub closest_rules: Vec<RuleChainEntry>,
    pub suggestion: String,
}

// ── Identity List ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct RbacIdentityList {
    pub service_accounts: Vec<IdentityEntry>,
    pub users: Vec<String>,
    pub groups: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdentityEntry {
    pub name: String,
    pub namespace: String,
}

// ── Natural Language Query ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct NaturalLanguageRbacResult {
    pub query: String,
    pub parsed_verb: Option<String>,
    pub parsed_resource: Option<String>,
    pub parsed_identity: Option<RbacIdentity>,
    pub parsed_namespace: Option<String>,
    pub result: Option<PermissionCheckResult>,
    pub who_can_results: Vec<PermissionCheckResult>,
}

// ── RBAC Snapshot ───────────────────────────────────────────────────

struct RbacSnapshot {
    roles: Vec<Role>,
    role_bindings: Vec<RoleBinding>,
    cluster_roles: Vec<ClusterRole>,
    cluster_role_bindings: Vec<ClusterRoleBinding>,
    service_accounts: Vec<ServiceAccount>,
}

// ── Helpers ─────────────────────────────────────────────────────────

fn subject_matches(subject: &Subject, identity: &RbacIdentity) -> bool {
    match identity {
        RbacIdentity::User { name } => subject.kind == "User" && subject.name == *name,
        RbacIdentity::Group { name } => subject.kind == "Group" && subject.name == *name,
        RbacIdentity::ServiceAccount { name, namespace } => {
            subject.kind == "ServiceAccount"
                && subject.name == *name
                && subject.namespace.as_deref() == Some(namespace.as_str())
        }
    }
}

fn rule_matches_verb(rule_verbs: &[String], verb: &str) -> bool {
    rule_verbs.iter().any(|v| v == "*" || v == verb)
}

fn rule_matches_resource(rule_resources: &[String], resource: &str) -> bool {
    rule_resources.iter().any(|r| r == "*" || r == resource)
}

fn rule_matches_api_group(rule_groups: &[String], api_group: &str) -> bool {
    // Empty string in rule means core API group
    rule_groups.iter().any(|g| g == "*" || g == api_group)
}

fn to_policy_rule_summary(rule: &k8s_openapi::api::rbac::v1::PolicyRule) -> PolicyRuleSummary {
    PolicyRuleSummary {
        verbs: rule.verbs.clone(),
        api_groups: rule.api_groups.clone().unwrap_or_default(),
        resources: rule.resources.clone().unwrap_or_default(),
        resource_names: rule.resource_names.clone().unwrap_or_default(),
    }
}

/// Resolve a role ref to a list of PolicyRules.
/// Handles ClusterRole aggregation.
fn resolve_role_rules<'a>(
    role_ref_kind: &str,
    role_ref_name: &str,
    role_namespace: Option<&str>,
    snapshot: &'a RbacSnapshot,
) -> Vec<&'a k8s_openapi::api::rbac::v1::PolicyRule> {
    match role_ref_kind {
        "ClusterRole" => {
            if let Some(cr) = snapshot
                .cluster_roles
                .iter()
                .find(|cr| cr.metadata.name.as_deref() == Some(role_ref_name))
            {
                // Handle aggregation
                if let Some(agg) = &cr.aggregation_rule {
                    if let Some(selectors) = &agg.cluster_role_selectors {
                        let mut rules = Vec::new();
                        for child_cr in &snapshot.cluster_roles {
                            let child_labels = child_cr
                                .metadata
                                .labels
                                .as_ref()
                                .map(|l| {
                                    l.iter()
                                        .map(|(k, v)| (k.as_str(), v.as_str()))
                                        .collect::<HashMap<_, _>>()
                                })
                                .unwrap_or_default();

                            let matches = selectors.iter().any(|sel| {
                                if let Some(match_labels) = &sel.match_labels {
                                    match_labels.iter().all(|(k, v)| {
                                        child_labels.get(k.as_str()) == Some(&v.as_str())
                                    })
                                } else {
                                    false
                                }
                            });

                            if matches {
                                if let Some(child_rules) = &child_cr.rules {
                                    rules.extend(child_rules.iter());
                                }
                            }
                        }
                        return rules;
                    }
                }

                cr.rules
                    .as_ref()
                    .map(|r| r.iter().collect())
                    .unwrap_or_default()
            } else {
                vec![]
            }
        }
        "Role" => {
            if let Some(ns) = role_namespace {
                if let Some(role) = snapshot.roles.iter().find(|r| {
                    r.metadata.name.as_deref() == Some(role_ref_name)
                        && r.metadata.namespace.as_deref() == Some(ns)
                }) {
                    role.rules
                        .as_ref()
                        .map(|r| r.iter().collect())
                        .unwrap_or_default()
                } else {
                    vec![]
                }
            } else {
                vec![]
            }
        }
        _ => vec![],
    }
}

/// Extract the base resource (before /) from a resource string.
fn base_resource(resource: &str) -> &str {
    resource.split('/').next().unwrap_or(resource)
}

/// Parse a 403 Forbidden error message from the Kubernetes API server.
fn parse_forbidden_message(msg: &str) -> Option<(String, String, String, Option<String>)> {
    // Pattern: "User \"...\" cannot VERB resource \"RESOURCE\" in API group \"GROUP\" in the namespace \"NS\""
    // Or: "RESOURCE is forbidden: User \"...\" cannot VERB resource \"RESOURCE\" ..."

    let lower = msg.to_lowercase();

    // Extract user
    let user = if let Some(start) = msg.find("User \"") {
        let after = &msg[start + 6..];
        after.split('"').next().map(|s| s.to_string())
    } else if let Some(start) = msg.find("user \"") {
        let after = &msg[start + 6..];
        after.split('"').next().map(|s| s.to_string())
    } else {
        None
    }?;

    // Extract verb: "cannot VERB resource"
    let verb = if let Some(pos) = lower.find("cannot ") {
        let after = &msg[pos + 7..];
        after.split_whitespace().next().map(|s| s.to_string())
    } else {
        None
    }?;

    // Extract resource: 'resource "RESOURCE"'
    let resource = if let Some(pos) = lower.find("resource \"") {
        let after = &msg[pos + 10..];
        after.split('"').next().map(|s| s.to_string())
    } else {
        None
    }?;

    // Extract namespace: 'namespace "NS"'
    let namespace = if let Some(pos) = lower.find("namespace \"") {
        let after = &msg[pos + 11..];
        after.split('"').next().map(|s| s.to_string())
    } else {
        None
    };

    Some((user, verb, resource, namespace))
}

fn identity_from_user_string(user: &str) -> RbacIdentity {
    // system:serviceaccount:NAMESPACE:NAME
    if let Some(rest) = user.strip_prefix("system:serviceaccount:") {
        let parts: Vec<&str> = rest.splitn(2, ':').collect();
        if parts.len() == 2 {
            return RbacIdentity::ServiceAccount {
                namespace: parts[0].to_string(),
                name: parts[1].to_string(),
            };
        }
    }
    RbacIdentity::User {
        name: user.to_string(),
    }
}

// ── Standard resources and verbs ────────────────────────────────────

const STANDARD_RESOURCES: &[(&str, &str)] = &[
    ("pods", ""),
    ("deployments", "apps"),
    ("replicasets", "apps"),
    ("statefulsets", "apps"),
    ("daemonsets", "apps"),
    ("services", ""),
    ("ingresses", "networking.k8s.io"),
    ("configmaps", ""),
    ("secrets", ""),
    ("nodes", ""),
    ("namespaces", ""),
    ("persistentvolumeclaims", ""),
    ("persistentvolumes", ""),
    ("serviceaccounts", ""),
    ("jobs", "batch"),
    ("cronjobs", "batch"),
    ("roles", "rbac.authorization.k8s.io"),
    ("rolebindings", "rbac.authorization.k8s.io"),
    ("clusterroles", "rbac.authorization.k8s.io"),
    ("clusterrolebindings", "rbac.authorization.k8s.io"),
    ("events", ""),
    ("networkpolicies", "networking.k8s.io"),
];

const STANDARD_VERBS: &[&str] = &[
    "get", "list", "watch", "create", "update", "patch", "delete",
];

// ── NL Parsing ──────────────────────────────────────────────────────

const VERB_SYNONYMS: &[(&str, &str)] = &[
    ("read", "get"),
    ("view", "get"),
    ("see", "get"),
    ("fetch", "get"),
    ("modify", "update"),
    ("edit", "update"),
    ("change", "update"),
    ("remove", "delete"),
    ("destroy", "delete"),
    ("kill", "delete"),
    ("make", "create"),
    ("add", "create"),
    ("apply", "patch"),
];

const RESOURCE_SYNONYMS: &[(&str, &str)] = &[
    ("pod", "pods"),
    ("deploy", "deployments"),
    ("deployment", "deployments"),
    ("svc", "services"),
    ("service", "services"),
    ("secret", "secrets"),
    ("configmap", "configmaps"),
    ("cm", "configmaps"),
    ("node", "nodes"),
    ("namespace", "namespaces"),
    ("ns", "namespaces"),
    ("ingress", "ingresses"),
    ("ing", "ingresses"),
    ("job", "jobs"),
    ("cronjob", "cronjobs"),
    ("cj", "cronjobs"),
    ("sa", "serviceaccounts"),
    ("serviceaccount", "serviceaccounts"),
    ("pvc", "persistentvolumeclaims"),
    ("pv", "persistentvolumes"),
    ("event", "events"),
    ("role", "roles"),
    ("clusterrole", "clusterroles"),
    ("rolebinding", "rolebindings"),
    ("clusterrolebinding", "clusterrolebindings"),
    ("netpol", "networkpolicies"),
    ("networkpolicy", "networkpolicies"),
    ("rs", "replicasets"),
    ("replicaset", "replicasets"),
    ("ds", "daemonsets"),
    ("daemonset", "daemonsets"),
    ("sts", "statefulsets"),
    ("statefulset", "statefulsets"),
];

fn normalize_verb(word: &str) -> Option<String> {
    let lower = word.to_lowercase();
    if STANDARD_VERBS.contains(&lower.as_str()) {
        return Some(lower);
    }
    VERB_SYNONYMS
        .iter()
        .find(|(syn, _)| *syn == lower.as_str())
        .map(|(_, canonical)| canonical.to_string())
}

fn normalize_resource(word: &str) -> Option<String> {
    let lower = word.to_lowercase();
    // Direct match on standard resources
    if STANDARD_RESOURCES.iter().any(|(r, _)| *r == lower.as_str()) {
        return Some(lower);
    }
    RESOURCE_SYNONYMS
        .iter()
        .find(|(syn, _)| *syn == lower.as_str())
        .map(|(_, canonical)| canonical.to_string())
}

/// Public wrapper for use in command handlers.
pub fn api_group_for_resource_cmd(resource: &str) -> String {
    api_group_for_resource(resource)
}

fn api_group_for_resource(resource: &str) -> String {
    STANDARD_RESOURCES
        .iter()
        .find(|(r, _)| *r == resource)
        .map(|(_, g)| g.to_string())
        .unwrap_or_default()
}

// ── Implementation ──────────────────────────────────────────────────

impl K8sState {
    async fn load_rbac_snapshot(&self) -> Result<RbacSnapshot> {
        let client = self.current_client().await?;

        let role_api: Api<Role> = Api::all(client.clone());
        let rb_api: Api<RoleBinding> = Api::all(client.clone());
        let cr_api: Api<ClusterRole> = Api::all(client.clone());
        let crb_api: Api<ClusterRoleBinding> = Api::all(client.clone());
        let sa_api: Api<ServiceAccount> = Api::all(client.clone());

        let lp = ListParams::default();
        let (roles, rbs, crs, crbs, sas) = tokio::join!(
            role_api.list(&lp),
            rb_api.list(&lp),
            cr_api.list(&lp),
            crb_api.list(&lp),
            sa_api.list(&lp),
        );

        Ok(RbacSnapshot {
            roles: roles.map_err(K8sError::Kube)?.items,
            role_bindings: rbs.map_err(K8sError::Kube)?.items,
            cluster_roles: crs.map_err(K8sError::Kube)?.items,
            cluster_role_bindings: crbs.map_err(K8sError::Kube)?.items,
            service_accounts: sas.map_err(K8sError::Kube)?.items,
        })
    }

    pub async fn rbac_check_permission(
        &self,
        identity: &RbacIdentity,
        verb: &str,
        resource: &str,
        api_group: &str,
        namespace: Option<&str>,
    ) -> Result<PermissionCheckResult> {
        let snapshot = self.load_rbac_snapshot().await?;
        let result = check_permission_from_snapshot(
            &snapshot, identity, verb, resource, api_group, namespace,
        );
        Ok(result)
    }

    pub async fn rbac_build_matrix(
        &self,
        identity: &RbacIdentity,
        namespace: Option<&str>,
    ) -> Result<PermissionMatrix> {
        let snapshot = self.load_rbac_snapshot().await?;

        let rows: Vec<PermissionMatrixRow> = STANDARD_RESOURCES
            .iter()
            .map(|(resource, api_group)| {
                let verbs: HashMap<String, PermissionCell> = STANDARD_VERBS
                    .iter()
                    .map(|verb| {
                        let result = check_permission_from_snapshot(
                            &snapshot, identity, verb, resource, api_group, namespace,
                        );
                        let status = if result.allowed {
                            if result
                                .rule_chain
                                .iter()
                                .any(|rc| !rc.matching_rule.resource_names.is_empty())
                            {
                                PermissionStatus::Conditional
                            } else {
                                PermissionStatus::Allowed
                            }
                        } else {
                            PermissionStatus::Denied
                        };
                        (
                            verb.to_string(),
                            PermissionCell {
                                status,
                                rule_chain: result.rule_chain,
                            },
                        )
                    })
                    .collect();

                PermissionMatrixRow {
                    resource: resource.to_string(),
                    api_group: api_group.to_string(),
                    verbs,
                }
            })
            .collect();

        Ok(PermissionMatrix {
            identity: identity.clone(),
            namespace: namespace.map(|s| s.to_string()),
            rows,
        })
    }

    pub async fn rbac_reverse_lookup(
        &self,
        role_kind: &str,
        role_name: &str,
        role_namespace: Option<&str>,
    ) -> Result<ReverseLookupResult> {
        let snapshot = self.load_rbac_snapshot().await?;

        // Get rules for this role
        let rules: Vec<PolicyRuleSummary> = match role_kind {
            "ClusterRole" => snapshot
                .cluster_roles
                .iter()
                .find(|cr| cr.metadata.name.as_deref() == Some(role_name))
                .and_then(|cr| cr.rules.as_ref())
                .map(|rules| rules.iter().map(to_policy_rule_summary).collect())
                .unwrap_or_default(),
            "Role" => snapshot
                .roles
                .iter()
                .find(|r| {
                    r.metadata.name.as_deref() == Some(role_name)
                        && r.metadata.namespace.as_deref() == role_namespace
                })
                .and_then(|r| r.rules.as_ref())
                .map(|rules| rules.iter().map(to_policy_rule_summary).collect())
                .unwrap_or_default(),
            _ => vec![],
        };

        let mut subjects = Vec::new();

        // Search ClusterRoleBindings
        for crb in &snapshot.cluster_role_bindings {
            let role_ref = &crb.role_ref;
            if role_ref.kind == role_kind && role_ref.name == role_name {
                if let Some(subs) = &crb.subjects {
                    for sub in subs {
                        let identity = subject_to_identity(sub);
                        let binding_name = crb.metadata.name.clone().unwrap_or_default();
                        subjects.push(ReverseLookupSubject {
                            identity,
                            binding_kind: "ClusterRoleBinding".to_string(),
                            binding_name,
                            binding_namespace: None,
                            scope: "cluster-wide".to_string(),
                        });
                    }
                }
            }
        }

        // Search RoleBindings
        for rb in &snapshot.role_bindings {
            let role_ref = &rb.role_ref;
            if role_ref.kind == role_kind && role_ref.name == role_name {
                if let Some(subs) = &rb.subjects {
                    for sub in subs {
                        let identity = subject_to_identity(sub);
                        let binding_ns = rb.metadata.namespace.clone();
                        let binding_name = rb.metadata.name.clone().unwrap_or_default();
                        let scope = binding_ns
                            .as_ref()
                            .map(|ns| format!("namespace: {ns}"))
                            .unwrap_or_else(|| "unknown".to_string());
                        subjects.push(ReverseLookupSubject {
                            identity,
                            binding_kind: "RoleBinding".to_string(),
                            binding_name,
                            binding_namespace: binding_ns,
                            scope,
                        });
                    }
                }
            }
        }

        Ok(ReverseLookupResult {
            role_kind: role_kind.to_string(),
            role_name: role_name.to_string(),
            role_namespace: role_namespace.map(|s| s.to_string()),
            rules,
            subjects,
        })
    }

    pub async fn rbac_analyze_forbidden(&self, error_message: &str) -> Result<ForbiddenAnalysis> {
        let (user, verb, resource, namespace) =
            parse_forbidden_message(error_message).ok_or_else(|| {
                K8sError::Validation("Could not parse forbidden error message".to_string())
            })?;

        let identity = identity_from_user_string(&user);
        let api_group = api_group_for_resource(&resource);

        let snapshot = self.load_rbac_snapshot().await?;

        // Find closest matching rules (rules that grant access to this resource
        // but not the exact verb, or the right verb but different resource)
        let closest = find_closest_rules(
            &snapshot,
            &identity,
            &verb,
            &resource,
            &api_group,
            namespace.as_deref(),
        );

        let missing_permission = format!(
            "{} {} on {} in {}",
            verb,
            resource,
            namespace.as_deref().unwrap_or("cluster scope"),
            api_group_display(&api_group),
        );

        let suggestion = format!(
            "Create a {}Binding in {} that grants the '{}' verb on '{}' to {}",
            if namespace.is_some() {
                "Role"
            } else {
                "ClusterRole"
            },
            namespace.as_deref().unwrap_or("the cluster"),
            verb,
            resource,
            identity,
        );

        Ok(ForbiddenAnalysis {
            verb,
            resource,
            namespace,
            identity,
            missing_permission,
            closest_rules: closest,
            suggestion,
        })
    }

    pub async fn rbac_list_identities(&self) -> Result<RbacIdentityList> {
        let snapshot = self.load_rbac_snapshot().await?;

        let service_accounts: Vec<IdentityEntry> = snapshot
            .service_accounts
            .iter()
            .map(|sa| IdentityEntry {
                name: sa.metadata.name.clone().unwrap_or_default(),
                namespace: sa
                    .metadata
                    .namespace
                    .clone()
                    .unwrap_or_else(|| "default".to_string()),
            })
            .collect();

        let mut users = HashSet::new();
        let mut groups = HashSet::new();

        let all_subjects = snapshot
            .cluster_role_bindings
            .iter()
            .flat_map(|crb| crb.subjects.iter().flatten())
            .chain(
                snapshot
                    .role_bindings
                    .iter()
                    .flat_map(|rb| rb.subjects.iter().flatten()),
            );

        for sub in all_subjects {
            match sub.kind.as_str() {
                "User" => {
                    users.insert(sub.name.clone());
                }
                "Group" => {
                    groups.insert(sub.name.clone());
                }
                _ => {}
            }
        }

        let mut users: Vec<String> = users.into_iter().collect();
        users.sort();
        let mut groups: Vec<String> = groups.into_iter().collect();
        groups.sort();

        Ok(RbacIdentityList {
            service_accounts,
            users,
            groups,
        })
    }

    pub async fn rbac_list_roles(&self, namespace: Option<&str>) -> Result<Vec<RoleSummary>> {
        let snapshot = self.load_rbac_snapshot().await?;
        let mut summaries = Vec::new();

        for cr in &snapshot.cluster_roles {
            let name = cr.metadata.name.clone().unwrap_or_default();
            let rules: Vec<PolicyRuleSummary> = cr
                .rules
                .as_ref()
                .map(|r| r.iter().map(to_policy_rule_summary).collect())
                .unwrap_or_default();
            let rule_count = rules.len();
            summaries.push(RoleSummary {
                kind: "ClusterRole".to_string(),
                name,
                namespace: None,
                rule_count,
                rules,
            });
        }

        let filtered_roles = if let Some(ns) = namespace {
            snapshot
                .roles
                .iter()
                .filter(|r| r.metadata.namespace.as_deref() == Some(ns))
                .collect::<Vec<_>>()
        } else {
            snapshot.roles.iter().collect()
        };

        for role in filtered_roles {
            let name = role.metadata.name.clone().unwrap_or_default();
            let ns = role.metadata.namespace.clone();
            let rules: Vec<PolicyRuleSummary> = role
                .rules
                .as_ref()
                .map(|r| r.iter().map(to_policy_rule_summary).collect())
                .unwrap_or_default();
            let rule_count = rules.len();
            summaries.push(RoleSummary {
                kind: "Role".to_string(),
                name,
                namespace: ns,
                rule_count,
                rules,
            });
        }

        summaries.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.name.cmp(&b.name)));

        Ok(summaries)
    }

    pub async fn rbac_who_can(
        &self,
        verb: &str,
        resource: &str,
        namespace: Option<&str>,
    ) -> Result<Vec<PermissionCheckResult>> {
        let snapshot = self.load_rbac_snapshot().await?;
        let api_group = api_group_for_resource(resource);

        let mut all_identities: Vec<RbacIdentity> = snapshot
            .service_accounts
            .iter()
            .map(|sa| RbacIdentity::ServiceAccount {
                name: sa.metadata.name.clone().unwrap_or_default(),
                namespace: sa
                    .metadata
                    .namespace
                    .clone()
                    .unwrap_or_else(|| "default".to_string()),
            })
            .collect();

        let all_subjects = snapshot
            .cluster_role_bindings
            .iter()
            .flat_map(|crb| crb.subjects.iter().flatten())
            .chain(
                snapshot
                    .role_bindings
                    .iter()
                    .flat_map(|rb| rb.subjects.iter().flatten()),
            );

        let mut seen = HashSet::new();
        for sub in all_subjects {
            let id = subject_to_identity(sub);
            if seen.insert(format!("{id:?}")) {
                all_identities.push(id);
            }
        }

        let results: Vec<PermissionCheckResult> = all_identities
            .iter()
            .map(|id| {
                check_permission_from_snapshot(&snapshot, id, verb, resource, &api_group, namespace)
            })
            .filter(|r| r.allowed)
            .collect();

        Ok(results)
    }

    pub async fn rbac_natural_language_query(
        &self,
        query: &str,
        namespace: Option<&str>,
    ) -> Result<NaturalLanguageRbacResult> {
        let words: Vec<&str> = query
            .split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_' && c != ':' && c != '/')
            .filter(|w| !w.is_empty())
            .collect();

        let lower_query = query.to_lowercase();
        let is_who_can = lower_query.starts_with("who can")
            || lower_query.starts_with("which")
            || lower_query.contains("who has");

        let mut parsed_verb = None;
        let mut parsed_resource = None;
        let mut parsed_identity = None;
        let mut parsed_namespace = namespace.map(|s| s.to_string());

        // Extract verb and resource from words
        for word in &words {
            if parsed_verb.is_none() {
                if let Some(v) = normalize_verb(word) {
                    parsed_verb = Some(v);
                    continue;
                }
            }
            if parsed_resource.is_none() {
                if let Some(r) = normalize_resource(word) {
                    parsed_resource = Some(r);
                    continue;
                }
            }
        }

        // Extract namespace: "in <namespace>" or "in the <namespace> namespace"
        if let Some(pos) = lower_query.find(" in ") {
            let after = &query[pos + 4..].trim();
            let ns_word = after
                .split(|c: char| c.is_whitespace())
                .next()
                .unwrap_or("")
                .trim_end_matches('?')
                .trim_matches('"');
            if ns_word != "the" && !ns_word.is_empty() {
                parsed_namespace = Some(ns_word.to_string());
            } else if ns_word == "the" {
                // "in the staging namespace"
                let rest = &after[4..].trim();
                let next = rest
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_end_matches('?');
                if !next.is_empty() && next != "namespace" {
                    parsed_namespace = Some(next.to_string());
                }
            }
        }

        // Extract identity: look for SA patterns or quoted names
        if !is_who_can {
            for word in &words {
                // ServiceAccount patterns: namespace/name or system:serviceaccount:ns:name
                if word.contains("serviceaccount:") {
                    parsed_identity = Some(identity_from_user_string(word));
                    break;
                }
                // Could be a service account name (contains - typically)
                if word.contains('/') {
                    let parts: Vec<&str> = word.splitn(2, '/').collect();
                    if parts.len() == 2 {
                        parsed_identity = Some(RbacIdentity::ServiceAccount {
                            namespace: parts[0].to_string(),
                            name: parts[1].to_string(),
                        });
                        break;
                    }
                }
                // Heuristic: words ending in -sa or containing "sa" might be SAs
                if word.ends_with("-sa") || word.ends_with("-serviceaccount") {
                    let ns = parsed_namespace.as_deref().unwrap_or("default");
                    parsed_identity = Some(RbacIdentity::ServiceAccount {
                        namespace: ns.to_string(),
                        name: word.to_string(),
                    });
                    break;
                }
            }

            // Positional fallback: in "Can <identity> <verb> <resource> ..."
            // extract words between "can" and the first recognized verb/resource
            if parsed_identity.is_none() {
                let lower_words: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
                let start = if lower_words.first().map(|w| w.as_str()) == Some("can") {
                    1
                } else {
                    0
                };
                // Collect tokens that are neither the parsed verb nor the parsed resource
                // nor filler words, up until we hit the verb
                let filler: HashSet<&str> = ["the", "a", "an", "user", "sa", "group", "if", "does"]
                    .iter()
                    .copied()
                    .collect();
                let mut identity_tokens: Vec<&str> = Vec::new();
                for w in &words[start..] {
                    let lw = w.to_lowercase();
                    // Stop once we reach the verb
                    if normalize_verb(w).is_some() {
                        break;
                    }
                    // Skip filler and resource words
                    if filler.contains(lw.as_str()) || normalize_resource(w).is_some() {
                        continue;
                    }
                    identity_tokens.push(w);
                }
                if !identity_tokens.is_empty() {
                    let raw_name = identity_tokens.join(" ");
                    parsed_identity = Some(identity_from_user_string(&raw_name));
                }
            }
        }

        let snapshot = self.load_rbac_snapshot().await?;

        // Try to resolve a bare name against known ServiceAccounts
        if let Some(RbacIdentity::User { ref name }) = parsed_identity {
            if !is_who_can {
                let sa_ns = parsed_namespace.as_deref().unwrap_or("default");
                let name_lower = name.to_lowercase();
                // Check if any SA in the target namespace (or any namespace) matches this name
                let matching_sa = snapshot
                    .service_accounts
                    .iter()
                    .find(|sa| {
                        let sa_name = sa.metadata.name.as_deref().unwrap_or("");
                        let sa_namespace = sa.metadata.namespace.as_deref().unwrap_or("default");
                        sa_name.to_lowercase() == name_lower && sa_namespace == sa_ns
                    })
                    .or_else(|| {
                        // Broaden: match any SA with this name regardless of namespace
                        snapshot.service_accounts.iter().find(|sa| {
                            sa.metadata.name.as_deref().unwrap_or("").to_lowercase() == name_lower
                        })
                    });
                if let Some(sa) = matching_sa {
                    parsed_identity = Some(RbacIdentity::ServiceAccount {
                        name: sa.metadata.name.clone().unwrap_or_default(),
                        namespace: sa
                            .metadata
                            .namespace
                            .clone()
                            .unwrap_or_else(|| "default".to_string()),
                    });
                }
            }
        }

        if is_who_can {
            // "Who can delete pods in staging?"
            let verb = parsed_verb.as_deref().unwrap_or("get");
            let resource = parsed_resource.as_deref().unwrap_or("pods");
            let api_group = api_group_for_resource(resource);

            // Collect all identities
            let mut all_identities: Vec<RbacIdentity> = snapshot
                .service_accounts
                .iter()
                .map(|sa| RbacIdentity::ServiceAccount {
                    name: sa.metadata.name.clone().unwrap_or_default(),
                    namespace: sa
                        .metadata
                        .namespace
                        .clone()
                        .unwrap_or_else(|| "default".to_string()),
                })
                .collect();

            // Add users and groups from bindings
            let all_subjects = snapshot
                .cluster_role_bindings
                .iter()
                .flat_map(|crb| crb.subjects.iter().flatten())
                .chain(
                    snapshot
                        .role_bindings
                        .iter()
                        .flat_map(|rb| rb.subjects.iter().flatten()),
                );

            let mut seen = HashSet::new();
            for sub in all_subjects {
                let id = subject_to_identity(sub);
                if seen.insert(format!("{id:?}")) {
                    all_identities.push(id);
                }
            }

            let who_can_results: Vec<PermissionCheckResult> = all_identities
                .iter()
                .map(|id| {
                    check_permission_from_snapshot(
                        &snapshot,
                        id,
                        verb,
                        resource,
                        &api_group,
                        parsed_namespace.as_deref(),
                    )
                })
                .filter(|r| r.allowed)
                .collect();

            return Ok(NaturalLanguageRbacResult {
                query: query.to_string(),
                parsed_verb,
                parsed_resource,
                parsed_identity: None,
                parsed_namespace,
                result: None,
                who_can_results,
            });
        }

        // Single identity check
        let result = if let (Some(verb), Some(resource)) =
            (parsed_verb.as_deref(), parsed_resource.as_deref())
        {
            let api_group = api_group_for_resource(resource);
            let identity = parsed_identity
                .clone()
                .unwrap_or_else(|| RbacIdentity::User {
                    name: "unknown".to_string(),
                });
            Some(check_permission_from_snapshot(
                &snapshot,
                &identity,
                verb,
                resource,
                &api_group,
                parsed_namespace.as_deref(),
            ))
        } else {
            None
        };

        Ok(NaturalLanguageRbacResult {
            query: query.to_string(),
            parsed_verb,
            parsed_resource,
            parsed_identity,
            parsed_namespace,
            result,
            who_can_results: vec![],
        })
    }
}

// ── Core permission resolution (pure, no async) ─────────────────────

fn check_permission_from_snapshot(
    snapshot: &RbacSnapshot,
    identity: &RbacIdentity,
    verb: &str,
    resource: &str,
    api_group: &str,
    namespace: Option<&str>,
) -> PermissionCheckResult {
    let mut rule_chain = Vec::new();
    let res_base = base_resource(resource);

    // 1. Check ClusterRoleBindings
    for crb in &snapshot.cluster_role_bindings {
        let subjects = match &crb.subjects {
            Some(s) => s,
            None => continue,
        };

        if !subjects.iter().any(|s| subject_matches(s, identity)) {
            continue;
        }

        let role_ref = &crb.role_ref;
        let rules = resolve_role_rules(&role_ref.kind, &role_ref.name, None, snapshot);

        for rule in rules {
            let rule_resources = rule.resources.as_deref().unwrap_or(&[]);
            let rule_api_groups = rule.api_groups.as_deref().unwrap_or(&[]);

            if rule_matches_verb(&rule.verbs, verb)
                && rule_matches_resource(rule_resources, res_base)
                && rule_matches_api_group(rule_api_groups, api_group)
            {
                rule_chain.push(RuleChainEntry {
                    role_kind: role_ref.kind.clone(),
                    role_name: role_ref.name.clone(),
                    role_namespace: None,
                    binding_kind: "ClusterRoleBinding".to_string(),
                    binding_name: crb.metadata.name.clone().unwrap_or_default(),
                    binding_namespace: None,
                    matching_rule: to_policy_rule_summary(rule),
                });
            }
        }
    }

    // 2. Check RoleBindings in the target namespace
    if let Some(ns) = namespace {
        for rb in &snapshot.role_bindings {
            if rb.metadata.namespace.as_deref() != Some(ns) {
                continue;
            }

            let subjects = match &rb.subjects {
                Some(s) => s,
                None => continue,
            };

            if !subjects.iter().any(|s| subject_matches(s, identity)) {
                continue;
            }

            let role_ref = &rb.role_ref;
            let rules = resolve_role_rules(&role_ref.kind, &role_ref.name, Some(ns), snapshot);

            for rule in rules {
                let rule_resources = rule.resources.as_deref().unwrap_or(&[]);
                let rule_api_groups = rule.api_groups.as_deref().unwrap_or(&[]);

                if rule_matches_verb(&rule.verbs, verb)
                    && rule_matches_resource(rule_resources, res_base)
                    && rule_matches_api_group(rule_api_groups, api_group)
                {
                    rule_chain.push(RuleChainEntry {
                        role_kind: role_ref.kind.clone(),
                        role_name: role_ref.name.clone(),
                        role_namespace: if role_ref.kind == "Role" {
                            Some(ns.to_string())
                        } else {
                            None
                        },
                        binding_kind: "RoleBinding".to_string(),
                        binding_name: rb.metadata.name.clone().unwrap_or_default(),
                        binding_namespace: Some(ns.to_string()),
                        matching_rule: to_policy_rule_summary(rule),
                    });
                }
            }
        }
    }

    let allowed = !rule_chain.is_empty();
    let ns_display = namespace.unwrap_or("cluster scope");

    let summary = if allowed {
        let first = &rule_chain[0];
        format!(
            "Allowed because of {} '{}' via {} '{}'",
            first.role_kind, first.role_name, first.binding_kind, first.binding_name
        )
    } else {
        format!("{identity} cannot {verb} {resource} in {ns_display}")
    };

    PermissionCheckResult {
        allowed,
        identity: identity.clone(),
        verb: verb.to_string(),
        resource: resource.to_string(),
        namespace: namespace.map(|s| s.to_string()),
        rule_chain,
        summary,
    }
}

fn subject_to_identity(sub: &Subject) -> RbacIdentity {
    match sub.kind.as_str() {
        "ServiceAccount" => RbacIdentity::ServiceAccount {
            name: sub.name.clone(),
            namespace: sub
                .namespace
                .clone()
                .unwrap_or_else(|| "default".to_string()),
        },
        "Group" => RbacIdentity::Group {
            name: sub.name.clone(),
        },
        _ => RbacIdentity::User {
            name: sub.name.clone(),
        },
    }
}

fn find_closest_rules(
    snapshot: &RbacSnapshot,
    identity: &RbacIdentity,
    verb: &str,
    resource: &str,
    api_group: &str,
    namespace: Option<&str>,
) -> Vec<RuleChainEntry> {
    let mut closest = Vec::new();
    let res_base = base_resource(resource);

    // Find rules that match the resource but not the verb, or vice versa
    let check_binding = |subjects: Option<&Vec<Subject>>,
                         role_kind: &str,
                         role_name: &str,
                         role_ns: Option<&str>,
                         binding_kind: &str,
                         binding_name: &str,
                         binding_ns: Option<&str>,
                         closest: &mut Vec<RuleChainEntry>| {
        let subjects = match subjects {
            Some(s) => s,
            None => return,
        };
        if !subjects.iter().any(|s| subject_matches(s, identity)) {
            return;
        }
        let rules = resolve_role_rules(role_kind, role_name, role_ns, snapshot);
        for rule in rules {
            let rule_resources = rule.resources.as_deref().unwrap_or(&[]);
            let rule_api_groups = rule.api_groups.as_deref().unwrap_or(&[]);

            let resource_match = rule_matches_resource(rule_resources, res_base);
            let verb_match = rule_matches_verb(&rule.verbs, verb);
            let group_match = rule_matches_api_group(rule_api_groups, api_group);

            // Partial match: exactly 2 of 3 (verb, resource, group) match
            let matches = u8::from(verb_match) + u8::from(resource_match) + u8::from(group_match);
            if matches == 2 {
                closest.push(RuleChainEntry {
                    role_kind: role_kind.to_string(),
                    role_name: role_name.to_string(),
                    role_namespace: role_ns.map(|s| s.to_string()),
                    binding_kind: binding_kind.to_string(),
                    binding_name: binding_name.to_string(),
                    binding_namespace: binding_ns.map(|s| s.to_string()),
                    matching_rule: to_policy_rule_summary(rule),
                });
            }
        }
    };

    for crb in &snapshot.cluster_role_bindings {
        let role_ref = &crb.role_ref;
        check_binding(
            crb.subjects.as_ref(),
            &role_ref.kind,
            &role_ref.name,
            None,
            "ClusterRoleBinding",
            crb.metadata.name.as_deref().unwrap_or(""),
            None,
            &mut closest,
        );
    }

    if let Some(ns) = namespace {
        for rb in &snapshot.role_bindings {
            if rb.metadata.namespace.as_deref() != Some(ns) {
                continue;
            }
            let role_ref = &rb.role_ref;
            check_binding(
                rb.subjects.as_ref(),
                &role_ref.kind,
                &role_ref.name,
                Some(ns),
                "RoleBinding",
                rb.metadata.name.as_deref().unwrap_or(""),
                Some(ns),
                &mut closest,
            );
        }
    }

    // Limit to 5 closest rules
    closest.truncate(5);
    closest
}

fn api_group_display(api_group: &str) -> String {
    if api_group.is_empty() {
        "core API group".to_string()
    } else {
        format!("API group \"{api_group}\"")
    }
}
