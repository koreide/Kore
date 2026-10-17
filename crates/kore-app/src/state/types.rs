//! Navigation and resource types shared by views (spec 01 §2).

use serde::{Deserialize, Serialize};

pub use kore_core::ResourceKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AppView {
    Table,
    Details,
    Dashboard,
    Graph,
    Crds,
    Helm,
    HelmDetail,
    Settings,
    Chat,
    NetworkPolicies,
    Rbac,
}

impl AppView {
    /// `settings.defaultView` → view (unknown values fall back to AI Chat).
    pub fn from_setting(s: &str) -> AppView {
        match s {
            "table" => AppView::Table,
            "dashboard" => AppView::Dashboard,
            "graph" => AppView::Graph,
            "crds" => AppView::Crds,
            "helm" => AppView::Helm,
            "settings" => AppView::Settings,
            "network-policies" => AppView::NetworkPolicies,
            "rbac" => AppView::Rbac,
            _ => AppView::Chat,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            AppView::Table => "table",
            AppView::Details => "details",
            AppView::Dashboard => "dashboard",
            AppView::Graph => "graph",
            AppView::Crds => "crds",
            AppView::Helm => "helm",
            AppView::HelmDetail => "helm-detail",
            AppView::Settings => "settings",
            AppView::Chat => "chat",
            AppView::NetworkPolicies => "network-policies",
            AppView::Rbac => "rbac",
        }
    }
}

/// UI helpers on the backend's `ResourceKind`.
pub trait KindExt {
    /// Header label: kind with first letter uppercased ("Configmaps", "Cronjobs").
    fn header_label(self) -> String;
    /// ConfirmDialog title noun: the Kubernetes kind name ("Pod", "Ingress",
    /// "ConfigMap", "CronJob").
    fn singular_label(self) -> String;
    /// Lucide icon used for this kind in the sidebar / pinned list.
    fn icon(self) -> &'static str;
    fn parse(s: &str) -> Option<ResourceKind>;
}

impl KindExt for ResourceKind {
    fn header_label(self) -> String {
        capitalize(self.as_str())
    }

    fn singular_label(self) -> String {
        match self {
            ResourceKind::Pods => "Pod",
            ResourceKind::Deployments => "Deployment",
            ResourceKind::Services => "Service",
            ResourceKind::Nodes => "Node",
            ResourceKind::Events => "Event",
            ResourceKind::Configmaps => "ConfigMap",
            ResourceKind::Secrets => "Secret",
            ResourceKind::Ingresses => "Ingress",
            ResourceKind::Jobs => "Job",
            ResourceKind::Cronjobs => "CronJob",
            ResourceKind::Namespaces => "Namespace",
        }
        .to_string()
    }

    fn icon(self) -> &'static str {
        match self {
            ResourceKind::Pods => "boxes",
            ResourceKind::Deployments => "server",
            ResourceKind::Services => "activity",
            ResourceKind::Nodes => "cpu",
            ResourceKind::Jobs => "timer",
            ResourceKind::Cronjobs => "calendar",
            ResourceKind::Ingresses => "globe",
            ResourceKind::Configmaps => "file-text",
            ResourceKind::Secrets => "lock",
            ResourceKind::Events => "zap",
            ResourceKind::Namespaces => "boxes",
        }
    }

    fn parse(s: &str) -> Option<ResourceKind> {
        ResourceKind::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

pub fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn singular_labels() {
        let got: Vec<String> = ResourceKind::ALL.into_iter().map(|k| k.singular_label()).collect();
        for want in ["Pod", "Deployment", "Service", "Node", "Event", "ConfigMap", "Secret", "Ingress", "Job", "CronJob", "Namespace"] {
            assert!(got.iter().any(|g| g == want), "missing {want} in {got:?}");
        }
        assert_eq!(ResourceKind::Ingresses.singular_label(), "Ingress");
        assert_eq!(ResourceKind::Configmaps.singular_label(), "ConfigMap");
        assert_eq!(ResourceKind::Cronjobs.singular_label(), "CronJob");
    }
}
