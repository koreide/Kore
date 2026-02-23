use crate::error::{K8sError, Result};
use crate::state::{K8sState, ResourceKind};
use kube::api::{Api, DeleteParams, ListParams, Resource, ResourceExt};
use kube::core::NamespaceResourceScope;
use serde_json::json;

impl K8sState {
    pub async fn list_resources(
        &self,
        kind: ResourceKind,
        namespace: Option<String>,
        label_selector: Option<String>,
    ) -> Result<Vec<serde_json::Value>> {
        match kind {
            ResourceKind::Pods => {
                self.list_typed::<k8s_openapi::api::core::v1::Pod>(namespace, label_selector)
                    .await
            }
            ResourceKind::Deployments => {
                self.list_typed::<k8s_openapi::api::apps::v1::Deployment>(namespace, label_selector)
                    .await
            }
            ResourceKind::Services => {
                self.list_typed::<k8s_openapi::api::core::v1::Service>(namespace, label_selector)
                    .await
            }
            ResourceKind::Nodes => {
                self.list_cluster_scoped::<k8s_openapi::api::core::v1::Node>(label_selector)
                    .await
            }
            ResourceKind::Events => {
                self.list_typed::<k8s_openapi::api::core::v1::Event>(namespace, label_selector)
                    .await
            }
            ResourceKind::Configmaps => {
                self.list_typed::<k8s_openapi::api::core::v1::ConfigMap>(namespace, label_selector)
                    .await
            }
            ResourceKind::Secrets => {
                self.list_typed::<k8s_openapi::api::core::v1::Secret>(namespace, label_selector)
                    .await
            }
            ResourceKind::Ingresses => {
                self.list_typed::<k8s_openapi::api::networking::v1::Ingress>(namespace, label_selector)
                    .await
            }
            ResourceKind::Jobs => {
                self.list_typed::<k8s_openapi::api::batch::v1::Job>(namespace, label_selector)
                    .await
            }
            ResourceKind::Cronjobs => {
                self.list_typed::<k8s_openapi::api::batch::v1::CronJob>(namespace, label_selector)
                    .await
            }
        }
    }

    /// Generic list for namespaced resources. Pass `None` for all namespaces.
    async fn list_typed<K>(
        &self,
        ns: Option<String>,
        label_selector: Option<String>,
    ) -> Result<Vec<serde_json::Value>>
    where
        K: Clone
            + serde::de::DeserializeOwned
            + serde::Serialize
            + Resource<Scope = NamespaceResourceScope>
            + Send
            + Sync
            + std::fmt::Debug
            + 'static,
        <K as Resource>::DynamicType: Default + Eq + std::hash::Hash,
    {
        let client = self.current_client().await?;
        let api: Api<K> = match ns {
            Some(namespace) => Api::namespaced(client, &namespace),
            None => Api::all(client),
        };

        let mut lp = ListParams::default();
        if let Some(labels) = label_selector {
            lp = lp.labels(&labels);
        }

        let list = api
            .list(&lp)
            .await
            .map_err(K8sError::Kube)?;
        let items: Vec<serde_json::Value> = list
            .items
            .iter()
            .map(|obj| {
                serde_json::to_value(obj)
                    .unwrap_or_else(|_| json!({ "name": obj.name_any() }))
            })
            .collect();

        Ok(items)
    }

    /// Generic list for cluster-scoped resources (e.g., nodes).
    async fn list_cluster_scoped<K>(
        &self,
        label_selector: Option<String>,
    ) -> Result<Vec<serde_json::Value>>
    where
        K: Clone
            + serde::de::DeserializeOwned
            + serde::Serialize
            + Resource
            + Send
            + Sync
            + std::fmt::Debug
            + 'static,
        <K as Resource>::DynamicType: Default + Eq + std::hash::Hash,
    {
        let client = self.current_client().await?;
        let api: Api<K> = Api::all(client);

        let mut lp = ListParams::default();
        if let Some(labels) = label_selector {
            lp = lp.labels(&labels);
        }

        let list = api
            .list(&lp)
            .await
            .map_err(K8sError::Kube)?;
        let items: Vec<serde_json::Value> = list
            .items
            .iter()
            .map(|obj| {
                serde_json::to_value(obj)
                    .unwrap_or_else(|_| json!({ "name": obj.name_any() }))
            })
            .collect();

        Ok(items)
    }

    pub async fn delete_resource(
        &self,
        kind: ResourceKind,
        namespace: String,
        name: String,
    ) -> Result<()> {
        let client = self.current_client().await?;
        let dp = DeleteParams::default();

        match kind {
            ResourceKind::Pods => {
                let api: Api<k8s_openapi::api::core::v1::Pod> =
                    Api::namespaced(client, &namespace);
                api.delete(&name, &dp).await.map_err(K8sError::Kube)?;
            }
            ResourceKind::Deployments => {
                let api: Api<k8s_openapi::api::apps::v1::Deployment> =
                    Api::namespaced(client, &namespace);
                api.delete(&name, &dp).await.map_err(K8sError::Kube)?;
            }
            ResourceKind::Services => {
                let api: Api<k8s_openapi::api::core::v1::Service> =
                    Api::namespaced(client, &namespace);
                api.delete(&name, &dp).await.map_err(K8sError::Kube)?;
            }
            ResourceKind::Nodes => {
                let api: Api<k8s_openapi::api::core::v1::Node> = Api::all(client);
                api.delete(&name, &dp).await.map_err(K8sError::Kube)?;
            }
            ResourceKind::Events => {
                let api: Api<k8s_openapi::api::core::v1::Event> =
                    Api::namespaced(client, &namespace);
                api.delete(&name, &dp).await.map_err(K8sError::Kube)?;
            }
            ResourceKind::Configmaps => {
                let api: Api<k8s_openapi::api::core::v1::ConfigMap> =
                    Api::namespaced(client, &namespace);
                api.delete(&name, &dp).await.map_err(K8sError::Kube)?;
            }
            ResourceKind::Secrets => {
                let api: Api<k8s_openapi::api::core::v1::Secret> =
                    Api::namespaced(client, &namespace);
                api.delete(&name, &dp).await.map_err(K8sError::Kube)?;
            }
            ResourceKind::Ingresses => {
                let api: Api<k8s_openapi::api::networking::v1::Ingress> =
                    Api::namespaced(client, &namespace);
                api.delete(&name, &dp).await.map_err(K8sError::Kube)?;
            }
            ResourceKind::Jobs => {
                let api: Api<k8s_openapi::api::batch::v1::Job> =
                    Api::namespaced(client, &namespace);
                api.delete(&name, &dp).await.map_err(K8sError::Kube)?;
            }
            ResourceKind::Cronjobs => {
                let api: Api<k8s_openapi::api::batch::v1::CronJob> =
                    Api::namespaced(client, &namespace);
                api.delete(&name, &dp).await.map_err(K8sError::Kube)?;
            }
        }

        Ok(())
    }

    pub async fn get_pod(
        &self,
        namespace: String,
        pod_name: String,
    ) -> Result<serde_json::Value> {
        let client = self.current_client().await?;
        let api: Api<k8s_openapi::api::core::v1::Pod> = Api::namespaced(client, &namespace);
        let pod = api.get(&pod_name).await.map_err(K8sError::Kube)?;
        serde_json::to_value(pod).map_err(K8sError::Serde)
    }

    /// Generic describe — returns full JSON for any resource kind.
    pub async fn describe_resource(
        &self,
        kind: ResourceKind,
        namespace: String,
        name: String,
    ) -> Result<serde_json::Value> {
        let client = self.current_client().await?;
        match kind {
            ResourceKind::Pods => {
                let api: Api<k8s_openapi::api::core::v1::Pod> =
                    Api::namespaced(client, &namespace);
                let obj = api.get(&name).await.map_err(K8sError::Kube)?;
                serde_json::to_value(obj).map_err(K8sError::Serde)
            }
            ResourceKind::Deployments => {
                let api: Api<k8s_openapi::api::apps::v1::Deployment> =
                    Api::namespaced(client, &namespace);
                let obj = api.get(&name).await.map_err(K8sError::Kube)?;
                serde_json::to_value(obj).map_err(K8sError::Serde)
            }
            ResourceKind::Services => {
                let api: Api<k8s_openapi::api::core::v1::Service> =
                    Api::namespaced(client, &namespace);
                let obj = api.get(&name).await.map_err(K8sError::Kube)?;
                serde_json::to_value(obj).map_err(K8sError::Serde)
            }
            ResourceKind::Nodes => {
                let api: Api<k8s_openapi::api::core::v1::Node> = Api::all(client);
                let obj = api.get(&name).await.map_err(K8sError::Kube)?;
                serde_json::to_value(obj).map_err(K8sError::Serde)
            }
            ResourceKind::Events => {
                let api: Api<k8s_openapi::api::core::v1::Event> =
                    Api::namespaced(client, &namespace);
                let obj = api.get(&name).await.map_err(K8sError::Kube)?;
                serde_json::to_value(obj).map_err(K8sError::Serde)
            }
            ResourceKind::Configmaps => {
                let api: Api<k8s_openapi::api::core::v1::ConfigMap> =
                    Api::namespaced(client, &namespace);
                let obj = api.get(&name).await.map_err(K8sError::Kube)?;
                serde_json::to_value(obj).map_err(K8sError::Serde)
            }
            ResourceKind::Secrets => {
                let api: Api<k8s_openapi::api::core::v1::Secret> =
                    Api::namespaced(client, &namespace);
                let obj = api.get(&name).await.map_err(K8sError::Kube)?;
                serde_json::to_value(obj).map_err(K8sError::Serde)
            }
            ResourceKind::Ingresses => {
                let api: Api<k8s_openapi::api::networking::v1::Ingress> =
                    Api::namespaced(client, &namespace);
                let obj = api.get(&name).await.map_err(K8sError::Kube)?;
                serde_json::to_value(obj).map_err(K8sError::Serde)
            }
            ResourceKind::Jobs => {
                let api: Api<k8s_openapi::api::batch::v1::Job> =
                    Api::namespaced(client, &namespace);
                let obj = api.get(&name).await.map_err(K8sError::Kube)?;
                serde_json::to_value(obj).map_err(K8sError::Serde)
            }
            ResourceKind::Cronjobs => {
                let api: Api<k8s_openapi::api::batch::v1::CronJob> =
                    Api::namespaced(client, &namespace);
                let obj = api.get(&name).await.map_err(K8sError::Kube)?;
                serde_json::to_value(obj).map_err(K8sError::Serde)
            }
        }
    }

    /// List Kubernetes events for a specific resource (filtered by involvedObject).
    pub async fn list_events_for_resource(
        &self,
        _kind: String,
        namespace: String,
        name: String,
    ) -> Result<Vec<serde_json::Value>> {
        let client = self.current_client().await?;
        let api: Api<k8s_openapi::api::core::v1::Event> =
            Api::namespaced(client, &namespace);

        let field_selector = format!(
            "involvedObject.name={name},involvedObject.namespace={namespace}"
        );
        let lp = ListParams::default().fields(&field_selector);

        let list = api.list(&lp).await.map_err(K8sError::Kube)?;
        let items: Vec<serde_json::Value> = list
            .items
            .iter()
            .map(|obj| {
                serde_json::to_value(obj)
                    .unwrap_or_else(|_| json!({ "name": obj.name_any() }))
            })
            .collect();

        Ok(items)
    }

    /// Scale a deployment to a given number of replicas.
    pub async fn scale_deployment(
        &self,
        namespace: String,
        name: String,
        replicas: i32,
    ) -> Result<()> {
        let client = self.current_client().await?;
        let api: Api<k8s_openapi::api::apps::v1::Deployment> =
            Api::namespaced(client, &namespace);

        let patch = json!({
            "spec": {
                "replicas": replicas
            }
        });

        api.patch(
            &name,
            &kube::api::PatchParams::apply("kore"),
            &kube::api::Patch::Merge(&patch),
        )
        .await
        .map_err(K8sError::Kube)?;

        Ok(())
    }

    /// Restart a deployment (equivalent to kubectl rollout restart).
    pub async fn restart_deployment(
        &self,
        namespace: String,
        name: String,
    ) -> Result<()> {
        let client = self.current_client().await?;
        let api: Api<k8s_openapi::api::apps::v1::Deployment> =
            Api::namespaced(client, &namespace);

        let now = chrono::Utc::now().to_rfc3339();
        let patch = json!({
            "spec": {
                "template": {
                    "metadata": {
                        "annotations": {
                            "kubectl.kubernetes.io/restartedAt": now
                        }
                    }
                }
            }
        });

        api.patch(
            &name,
            &kube::api::PatchParams::apply("kore").force(),
            &kube::api::Patch::Merge(&patch),
        )
        .await
        .map_err(K8sError::Kube)?;

        Ok(())
    }

    /// Search across all resource types in a namespace.
    pub async fn search_resources(
        &self,
        query: String,
        namespace: Option<String>,
    ) -> Result<Vec<serde_json::Value>> {
        let query_lower = query.to_lowercase();

        // List all resource types in parallel
        let (pods, deployments, services, configmaps, secrets, jobs, cronjobs, ingresses) = tokio::join!(
            self.list_resources(ResourceKind::Pods, namespace.clone(), None),
            self.list_resources(ResourceKind::Deployments, namespace.clone(), None),
            self.list_resources(ResourceKind::Services, namespace.clone(), None),
            self.list_resources(ResourceKind::Configmaps, namespace.clone(), None),
            self.list_resources(ResourceKind::Secrets, namespace.clone(), None),
            self.list_resources(ResourceKind::Jobs, namespace.clone(), None),
            self.list_resources(ResourceKind::Cronjobs, namespace.clone(), None),
            self.list_resources(ResourceKind::Ingresses, namespace.clone(), None),
        );

        let mut results = Vec::new();

        let kinds_and_items = [
            ("pods", pods),
            ("deployments", deployments),
            ("services", services),
            ("configmaps", configmaps),
            ("secrets", secrets),
            ("jobs", jobs),
            ("cronjobs", cronjobs),
            ("ingresses", ingresses),
        ];

        for (kind_name, items_result) in kinds_and_items {
            if let Ok(items) = items_result {
                for mut item in items {
                    let name = item
                        .get("metadata")
                        .and_then(|m| m.get("name"))
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                        .to_lowercase();

                    if name.contains(&query_lower) {
                        if let Some(obj) = item.as_object_mut() { obj.insert("_kind".to_string(), json!(kind_name)); }
                        results.push(item);
                    }
                }
            }
        }

        Ok(results)
    }
}
