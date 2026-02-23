use crate::error::{K8sError, Result};
use crate::state::K8sState;
use kube::api::{Api, ApiResource, DeleteParams, DynamicObject, GroupVersionKind, ListParams, ResourceExt};
use serde::Serialize;
use serde_json::json;
use tracing::info;

#[derive(Debug, Clone, Serialize)]
pub struct CrdInfo {
    pub name: String,
    pub group: String,
    pub version: String,
    pub kind: String,
    pub scope: String,
    pub plural: String,
}

impl K8sState {
    /// List all CustomResourceDefinitions in the cluster.
    pub async fn list_crds(&self) -> Result<Vec<CrdInfo>> {
        let client = self.current_client().await?;
        let api: Api<k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition> = Api::all(client);

        let list = api
            .list(&ListParams::default())
            .await
            .map_err(K8sError::Kube)?;

        let crds: Vec<CrdInfo> = list
            .items
            .iter()
            .map(|crd| {
                let spec = &crd.spec;
                let name = crd.name_any();
                let group = spec.group.clone();
                let kind = spec.names.kind.clone();
                let plural = spec.names.plural.clone();
                let scope = match spec.scope.as_str() {
                    "Namespaced" => "Namespaced",
                    _ => "Cluster",
                }
                .to_string();

                // Use the first served version
                let version = spec
                    .versions
                    .iter()
                    .find(|v| v.served)
                    .map(|v| v.name.clone())
                    .unwrap_or_default();

                CrdInfo {
                    name,
                    group,
                    version,
                    kind,
                    scope,
                    plural,
                }
            })
            .collect();

        info!(count = crds.len(), "Discovered CRDs");
        Ok(crds)
    }

    /// List instances of a specific CRD.
    pub async fn list_crd_instances(
        &self,
        group: String,
        version: String,
        plural: String,
        namespace: Option<String>,
    ) -> Result<Vec<serde_json::Value>> {
        let client = self.current_client().await?;
        let ar = ApiResource::from_gvk_with_plural(
            &GroupVersionKind::gvk(&group, &version, &plural),
            &plural,
        );

        let api: Api<DynamicObject> = match namespace {
            Some(ns) => Api::namespaced_with(client, &ns, &ar),
            None => Api::all_with(client, &ar),
        };

        let list = api
            .list(&ListParams::default())
            .await
            .map_err(K8sError::Kube)?;

        let items: Vec<serde_json::Value> = list
            .items
            .iter()
            .map(|obj| serde_json::to_value(obj).unwrap_or_else(|_| json!({ "name": obj.name_any() })))
            .collect();

        Ok(items)
    }

    /// Get a single CRD instance.
    pub async fn get_crd_instance(
        &self,
        group: String,
        version: String,
        plural: String,
        namespace: String,
        name: String,
    ) -> Result<serde_json::Value> {
        let client = self.current_client().await?;
        let ar = ApiResource::from_gvk_with_plural(
            &GroupVersionKind::gvk(&group, &version, &plural),
            &plural,
        );

        let api: Api<DynamicObject> = if namespace.is_empty() {
            Api::all_with(client, &ar)
        } else {
            Api::namespaced_with(client, &namespace, &ar)
        };

        let obj = api.get(&name).await.map_err(K8sError::Kube)?;
        serde_json::to_value(obj).map_err(K8sError::Serde)
    }

    /// Delete a CRD instance.
    pub async fn delete_crd_instance(
        &self,
        group: String,
        version: String,
        plural: String,
        namespace: String,
        name: String,
    ) -> Result<()> {
        let client = self.current_client().await?;
        let ar = ApiResource::from_gvk_with_plural(
            &GroupVersionKind::gvk(&group, &version, &plural),
            &plural,
        );

        let api: Api<DynamicObject> = if namespace.is_empty() {
            Api::all_with(client, &ar)
        } else {
            Api::namespaced_with(client, &namespace, &ar)
        };

        api.delete(&name, &DeleteParams::default())
            .await
            .map_err(K8sError::Kube)?;

        info!(name = %name, "Deleted CRD instance");
        Ok(())
    }
}
