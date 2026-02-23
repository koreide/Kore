use crate::error::{K8sError, Result};
use crate::state::K8sState;
use kube::api::{Api, ListParams, Patch, PatchParams, ResourceExt};
use serde::Serialize;
use serde_json::json;
use tracing::info;

#[derive(Debug, Clone, Serialize)]
pub struct DeploymentRevision {
    pub revision: i64,
    pub name: String,
    pub created: String,
    pub image: String,
    pub change_cause: String,
    pub replicas: i32,
}

impl K8sState {
    /// List deployment revisions by reading owned ReplicaSets.
    pub async fn list_deployment_revisions(
        &self,
        namespace: String,
        deployment_name: String,
    ) -> Result<Vec<DeploymentRevision>> {
        let client = self.current_client().await?;
        let rs_api: Api<k8s_openapi::api::apps::v1::ReplicaSet> =
            Api::namespaced(client, &namespace);

        let lp = ListParams::default();
        let rs_list = rs_api.list(&lp).await.map_err(K8sError::Kube)?;

        let mut revisions: Vec<DeploymentRevision> = rs_list
            .items
            .iter()
            .filter(|rs| {
                rs.metadata
                    .owner_references
                    .as_ref()
                    .map(|refs| {
                        refs.iter()
                            .any(|r| r.kind == "Deployment" && r.name == deployment_name)
                    })
                    .unwrap_or(false)
            })
            .filter_map(|rs| {
                let annotations = rs.metadata.annotations.as_ref()?;
                let revision_str = annotations.get("deployment.kubernetes.io/revision")?;
                let revision = revision_str.parse::<i64>().ok()?;
                let change_cause = annotations
                    .get("kubernetes.io/change-cause")
                    .cloned()
                    .unwrap_or_default();

                let image = rs
                    .spec
                    .as_ref()
                    .and_then(|s| s.template.as_ref())
                    .and_then(|t| t.spec.as_ref())
                    .and_then(|ps| ps.containers.first())
                    .and_then(|c| c.image.clone())
                    .unwrap_or_default();

                let replicas = rs.spec.as_ref().and_then(|s| s.replicas).unwrap_or(0);

                let created = rs
                    .metadata
                    .creation_timestamp
                    .as_ref()
                    .map(|t| t.0.to_rfc3339())
                    .unwrap_or_default();

                Some(DeploymentRevision {
                    revision,
                    name: rs.name_any(),
                    created,
                    image,
                    change_cause,
                    replicas,
                })
            })
            .collect();

        revisions.sort_by(|a, b| b.revision.cmp(&a.revision));
        Ok(revisions)
    }

    /// Get YAML for a specific revision's ReplicaSet.
    pub async fn get_revision_yaml(&self, namespace: String, rs_name: String) -> Result<String> {
        let client = self.current_client().await?;
        let api: Api<k8s_openapi::api::apps::v1::ReplicaSet> = Api::namespaced(client, &namespace);
        let rs = api.get(&rs_name).await.map_err(K8sError::Kube)?;
        let mut value = serde_json::to_value(&rs).map_err(K8sError::Serde)?;
        if let Some(obj) = value.as_object_mut() {
            if let Some(metadata) = obj.get_mut("metadata") {
                if let Some(m) = metadata.as_object_mut() {
                    m.remove("managedFields");
                }
            }
        }
        serde_yaml::to_string(&value)
            .map_err(|e| K8sError::Validation(format!("YAML serialization failed: {e}")))
    }

    /// Rollback deployment to a specific revision by patching the deployment
    /// with the ReplicaSet's pod template.
    pub async fn rollback_deployment(
        &self,
        namespace: String,
        deployment_name: String,
        rs_name: String,
    ) -> Result<()> {
        let client = self.current_client().await?;
        let rs_api: Api<k8s_openapi::api::apps::v1::ReplicaSet> =
            Api::namespaced(client.clone(), &namespace);

        let rs = rs_api.get(&rs_name).await.map_err(K8sError::Kube)?;
        let template = rs
            .spec
            .as_ref()
            .and_then(|s| s.template.as_ref())
            .ok_or_else(|| K8sError::Validation("ReplicaSet has no template".to_string()))?;

        let deploy_api: Api<k8s_openapi::api::apps::v1::Deployment> =
            Api::namespaced(client, &namespace);

        let patch = json!({
            "spec": {
                "template": serde_json::to_value(template).map_err(K8sError::Serde)?
            }
        });

        deploy_api
            .patch(
                &deployment_name,
                &PatchParams::apply("kore").force(),
                &Patch::Merge(&patch),
            )
            .await
            .map_err(K8sError::Kube)?;

        info!(
            deployment = %deployment_name,
            replica_set = %rs_name,
            "Rolled back deployment"
        );

        Ok(())
    }
}
