use crate::error::{K8sError, Result};
use crate::state::{K8sState, ResourceKind};
use kube::api::{Api, Patch, PatchParams};
use similar::{ChangeTag, TextDiff};
use tracing::info;

#[derive(Debug, Clone, serde::Serialize)]
pub struct DiffLine {
    pub tag: String, // "equal", "insert", "delete"
    pub value: String,
}

impl K8sState {
    /// Get resource YAML as a formatted string.
    pub async fn get_resource_yaml(
        &self,
        kind: ResourceKind,
        namespace: String,
        name: String,
    ) -> Result<String> {
        let json_value = self.describe_resource(kind, namespace, name).await?;
        // Clean managed fields for readability
        let mut clean = json_value.clone();
        if let Some(obj) = clean.as_object_mut() {
            if let Some(metadata) = obj.get_mut("metadata") {
                if let Some(m) = metadata.as_object_mut() {
                    m.remove("managedFields");
                }
            }
        }
        serde_yaml::to_string(&clean)
            .map_err(|e| K8sError::Validation(format!("YAML serialization failed: {e}")))
    }

    /// Apply YAML to a resource using server-side apply.
    pub async fn apply_resource_yaml(
        &self,
        kind: ResourceKind,
        namespace: String,
        name: String,
        yaml_content: String,
    ) -> Result<String> {
        let value: serde_json::Value = serde_yaml::from_str(&yaml_content)
            .map_err(|e| K8sError::Validation(format!("Invalid YAML: {e}")))?;

        let client = self.current_client().await?;
        let pp = PatchParams::apply("kore").force();

        match kind {
            ResourceKind::Pods => {
                let api: Api<k8s_openapi::api::core::v1::Pod> = Api::namespaced(client, &namespace);
                api.patch(&name, &pp, &Patch::Apply(&value))
                    .await
                    .map_err(K8sError::Kube)?;
            }
            ResourceKind::Deployments => {
                let api: Api<k8s_openapi::api::apps::v1::Deployment> =
                    Api::namespaced(client, &namespace);
                api.patch(&name, &pp, &Patch::Apply(&value))
                    .await
                    .map_err(K8sError::Kube)?;
            }
            ResourceKind::Services => {
                let api: Api<k8s_openapi::api::core::v1::Service> =
                    Api::namespaced(client, &namespace);
                api.patch(&name, &pp, &Patch::Apply(&value))
                    .await
                    .map_err(K8sError::Kube)?;
            }
            ResourceKind::Configmaps => {
                let api: Api<k8s_openapi::api::core::v1::ConfigMap> =
                    Api::namespaced(client, &namespace);
                api.patch(&name, &pp, &Patch::Apply(&value))
                    .await
                    .map_err(K8sError::Kube)?;
            }
            ResourceKind::Secrets => {
                let api: Api<k8s_openapi::api::core::v1::Secret> =
                    Api::namespaced(client, &namespace);
                api.patch(&name, &pp, &Patch::Apply(&value))
                    .await
                    .map_err(K8sError::Kube)?;
            }
            ResourceKind::Ingresses => {
                let api: Api<k8s_openapi::api::networking::v1::Ingress> =
                    Api::namespaced(client, &namespace);
                api.patch(&name, &pp, &Patch::Apply(&value))
                    .await
                    .map_err(K8sError::Kube)?;
            }
            ResourceKind::Jobs => {
                let api: Api<k8s_openapi::api::batch::v1::Job> =
                    Api::namespaced(client, &namespace);
                api.patch(&name, &pp, &Patch::Apply(&value))
                    .await
                    .map_err(K8sError::Kube)?;
            }
            ResourceKind::Cronjobs => {
                let api: Api<k8s_openapi::api::batch::v1::CronJob> =
                    Api::namespaced(client, &namespace);
                api.patch(&name, &pp, &Patch::Apply(&value))
                    .await
                    .map_err(K8sError::Kube)?;
            }
            _ => {
                return Err(K8sError::Validation(format!(
                    "YAML apply not supported for {kind:?}"
                )));
            }
        }

        info!(kind = ?kind, name = %name, "Applied YAML via server-side apply");
        Ok("Applied successfully".to_string())
    }

    /// Compute diff between current live state and provided YAML.
    pub async fn diff_resource_yaml(
        &self,
        kind: ResourceKind,
        namespace: String,
        name: String,
        new_yaml: String,
    ) -> Result<Vec<DiffLine>> {
        let current_yaml = self.get_resource_yaml(kind, namespace, name).await?;
        let diff = TextDiff::from_lines(&current_yaml, &new_yaml);

        let lines: Vec<DiffLine> = diff
            .iter_all_changes()
            .map(|change| DiffLine {
                tag: match change.tag() {
                    ChangeTag::Equal => "equal".to_string(),
                    ChangeTag::Insert => "insert".to_string(),
                    ChangeTag::Delete => "delete".to_string(),
                },
                value: change.value().to_string(),
            })
            .collect();

        Ok(lines)
    }
}
