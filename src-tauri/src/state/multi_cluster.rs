use crate::error::{K8sError, Result};
use crate::state::{K8sState, ResourceKind};
use kube::api::{Api, ListParams, Resource, ResourceExt};
use kube::config::KubeConfigOptions;
use kube::core::NamespaceResourceScope;
use kube::{Client, Config};
use serde_json::json;
use tracing::warn;

impl K8sState {
    /// List resources across all configured contexts.
    pub async fn list_resources_multi_cluster(
        &self,
        kind: ResourceKind,
        namespace: Option<String>,
        label_selector: Option<String>,
    ) -> Result<Vec<serde_json::Value>> {
        let inner = self.inner.read().await;
        let kubeconfig = inner
            .kubeconfig
            .clone()
            .ok_or(K8sError::ClientMissing)?;
        drop(inner);

        let context_names: Vec<String> = kubeconfig
            .contexts
            .iter()
            .map(|c| c.name.clone())
            .collect();

        let mut handles = Vec::new();

        for ctx_name in context_names {
            let kc = kubeconfig.clone();
            let k = kind.clone();
            let ns = namespace.clone();
            let ls = label_selector.clone();
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
                        return Vec::new();
                    }
                };
                let client = match Client::try_from(config) {
                    Ok(c) => c,
                    Err(e) => {
                        warn!(context = %ctx, error = %e, "Failed to create client for context");
                        return Vec::new();
                    }
                };

                // List resources using the temporary client
                let items = match list_resources_with_client(&client, &k, ns.as_deref(), ls.as_deref()).await {
                    Ok(items) => items,
                    Err(e) => {
                        warn!(context = %ctx, error = %e, "Failed to list resources");
                        return Vec::new();
                    }
                };

                // Tag each item with the context
                items
                    .into_iter()
                    .map(|mut item| {
                        if let Some(obj) = item.as_object_mut() {
                            obj.insert("_context".to_string(), json!(ctx));
                        }
                        item
                    })
                    .collect::<Vec<_>>()
            }));
        }

        let mut all_items = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(items) => all_items.extend(items),
                Err(e) => warn!(error = %e, "Task join error"),
            }
        }

        Ok(all_items)
    }
}

async fn list_resources_with_client(
    client: &Client,
    kind: &ResourceKind,
    namespace: Option<&str>,
    label_selector: Option<&str>,
) -> Result<Vec<serde_json::Value>> {
    let mut lp = ListParams::default();
    if let Some(ls) = label_selector {
        lp = lp.labels(ls);
    }

    match kind {
        ResourceKind::Pods => list_ns::<k8s_openapi::api::core::v1::Pod>(client, namespace, &lp).await,
        ResourceKind::Deployments => list_ns::<k8s_openapi::api::apps::v1::Deployment>(client, namespace, &lp).await,
        ResourceKind::Services => list_ns::<k8s_openapi::api::core::v1::Service>(client, namespace, &lp).await,
        ResourceKind::Nodes => list_all::<k8s_openapi::api::core::v1::Node>(client, &lp).await,
        ResourceKind::Events => list_ns::<k8s_openapi::api::core::v1::Event>(client, namespace, &lp).await,
        ResourceKind::Configmaps => list_ns::<k8s_openapi::api::core::v1::ConfigMap>(client, namespace, &lp).await,
        ResourceKind::Secrets => list_ns::<k8s_openapi::api::core::v1::Secret>(client, namespace, &lp).await,
        ResourceKind::Ingresses => list_ns::<k8s_openapi::api::networking::v1::Ingress>(client, namespace, &lp).await,
        ResourceKind::Jobs => list_ns::<k8s_openapi::api::batch::v1::Job>(client, namespace, &lp).await,
        ResourceKind::Cronjobs => list_ns::<k8s_openapi::api::batch::v1::CronJob>(client, namespace, &lp).await,
    }
}

async fn list_ns<K>(
    client: &Client,
    namespace: Option<&str>,
    lp: &ListParams,
) -> Result<Vec<serde_json::Value>>
where
    K: Clone + serde::de::DeserializeOwned + serde::Serialize + Resource<Scope = NamespaceResourceScope> + Send + Sync + std::fmt::Debug + 'static,
    <K as Resource>::DynamicType: Default + Eq + std::hash::Hash,
{
    let api: Api<K> = match namespace {
        Some(ns) => Api::namespaced(client.clone(), ns),
        None => Api::all(client.clone()),
    };
    let list = api.list(lp).await.map_err(K8sError::Kube)?;
    Ok(list
        .items
        .iter()
        .map(|obj| serde_json::to_value(obj).unwrap_or_else(|_| json!({ "name": obj.name_any() })))
        .collect())
}

async fn list_all<K>(
    client: &Client,
    lp: &ListParams,
) -> Result<Vec<serde_json::Value>>
where
    K: Clone + serde::de::DeserializeOwned + serde::Serialize + Resource + Send + Sync + std::fmt::Debug + 'static,
    <K as Resource>::DynamicType: Default + Eq + std::hash::Hash,
{
    let api: Api<K> = Api::all(client.clone());
    let list = api.list(lp).await.map_err(K8sError::Kube)?;
    Ok(list
        .items
        .iter()
        .map(|obj| serde_json::to_value(obj).unwrap_or_else(|_| json!({ "name": obj.name_any() })))
        .collect())
}
