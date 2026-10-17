use crate::constants::{
    LIST_PAGE_ATTEMPTS, LIST_PAGE_SIZE, LIST_PAGE_TIMEOUT, MAX_LIST_TOTAL, MAX_SEARCH_RESULTS,
};
use crate::error::{K8sError, Result};
use crate::state::{K8sState, ResourceKind};
use kube::api::{Api, DeleteParams, ListParams, Resource, ResourceExt};
use serde::Serialize;
use serde_json::json;
use std::future::Future;
use std::time::Duration;

/// Result of a full (paginated) resource list.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ResourceList {
    pub items: Vec<serde_json::Value>,
    /// True when the list stopped at [`MAX_LIST_TOTAL`] objects and more exist.
    pub truncated: bool,
}

/// True for the API's "410 Gone / Expired" answer to a stale `continue` token.
fn is_expired(e: &K8sError) -> bool {
    matches!(e, K8sError::Kube(kube::Error::Api(ae)) if ae.code == 410)
}

/// Page through a list with `continue` tokens (A4).
///
/// `fetch(token)` returns one page and the next token (`None`/empty = done).
/// Every page gets `page_timeout` per attempt and up to `attempts` attempts.
/// An expired token (410) restarts the list from scratch once. Stops at `cap`
/// objects; the second value is then `true` when more objects exist.
pub(crate) async fn paginate<T, F, Fut>(
    cap: usize,
    page_timeout: Duration,
    attempts: u32,
    mut fetch: F,
) -> Result<(Vec<T>, bool)>
where
    F: FnMut(Option<String>) -> Fut,
    Fut: Future<Output = Result<(Vec<T>, Option<String>)>>,
{
    let mut items: Vec<T> = Vec::new();
    let mut token: Option<String> = None;
    let mut restarted = false;
    loop {
        let mut page = None;
        for attempt in 1..=attempts.max(1) {
            match tokio::time::timeout(page_timeout, fetch(token.clone())).await {
                Ok(res) => {
                    page = Some(res);
                    break;
                }
                Err(_) => tracing::warn!(attempt, "List page timed out; retrying"),
            }
        }
        let page = page.ok_or_else(|| {
            K8sError::other(format!("Request timed out after {attempts} attempts"))
        })?;
        let (mut batch, next) = match page {
            Ok(p) => p,
            Err(e) if token.is_some() && !restarted && is_expired(&e) => {
                tracing::warn!("List continue token expired; restarting the list");
                restarted = true;
                items.clear();
                token = None;
                continue;
            }
            Err(e) => return Err(e),
        };
        items.append(&mut batch);
        let next = next.filter(|t| !t.is_empty());
        if items.len() >= cap {
            let truncated = items.len() > cap || next.is_some();
            items.truncate(cap);
            if truncated {
                tracing::warn!(cap, "List truncated at the safety cap");
            }
            return Ok((items, truncated));
        }
        match next {
            Some(t) => token = Some(t),
            None => return Ok((items, false)),
        }
    }
}

impl K8sState {
    pub async fn list_resources(
        &self,
        kind: ResourceKind,
        namespace: Option<String>,
        label_selector: Option<String>,
    ) -> Result<Vec<serde_json::Value>> {
        Ok(self
            .list_resources_full(kind, namespace, label_selector)
            .await?
            .items)
    }

    /// Like [`K8sState::list_resources`], with the `truncated` flag.
    pub async fn list_resources_full(
        &self,
        kind: ResourceKind,
        namespace: Option<String>,
        label_selector: Option<String>,
    ) -> Result<ResourceList> {
        use k8s_openapi::api::networking::v1 as net;
        use k8s_openapi::api::{apps::v1 as apps, batch::v1 as batch, core::v1 as core};
        let client = self.current_client().await?;
        let ns = namespace.as_deref();
        let sel = label_selector.as_deref();
        match kind {
            ResourceKind::Pods => list_api::<core::Pod>(ns_api(client, ns), sel).await,
            ResourceKind::Deployments => {
                list_api::<apps::Deployment>(ns_api(client, ns), sel).await
            }
            ResourceKind::Services => list_api::<core::Service>(ns_api(client, ns), sel).await,
            ResourceKind::Nodes => list_api::<core::Node>(Api::all(client), sel).await,
            ResourceKind::Events => {
                // Context captured before the list so a concurrent switch
                // cannot file these events under the new context.
                let ctx = self.current_context_name().await.unwrap_or_default();
                let list = list_api::<core::Event>(ns_api(client, ns), sel).await?;
                // Deliberate Fix 10: persist listed events into the SQLite store.
                if self.event_store().is_some() {
                    let st = self.clone();
                    let copy = list.items.clone();
                    tokio::spawn(async move { st.record_events_in(copy, ctx).await });
                }
                Ok(list)
            }
            ResourceKind::Configmaps => list_api::<core::ConfigMap>(ns_api(client, ns), sel).await,
            ResourceKind::Secrets => list_api::<core::Secret>(ns_api(client, ns), sel).await,
            ResourceKind::Ingresses => list_api::<net::Ingress>(ns_api(client, ns), sel).await,
            ResourceKind::Jobs => list_api::<batch::Job>(ns_api(client, ns), sel).await,
            ResourceKind::Cronjobs => list_api::<batch::CronJob>(ns_api(client, ns), sel).await,
            ResourceKind::Namespaces => list_api::<core::Namespace>(Api::all(client), sel).await,
        }
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
                let api: Api<k8s_openapi::api::core::v1::Pod> = Api::namespaced(client, &namespace);
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
            ResourceKind::Namespaces => {
                let api: Api<k8s_openapi::api::core::v1::Namespace> = Api::all(client);
                api.delete(&name, &dp).await.map_err(K8sError::Kube)?;
            }
        }

        Ok(())
    }

    pub async fn get_pod(&self, namespace: String, pod_name: String) -> Result<serde_json::Value> {
        let client = self.current_client().await?;
        let api: Api<k8s_openapi::api::core::v1::Pod> = Api::namespaced(client, &namespace);
        let pod = api.get(&pod_name).await.map_err(K8sError::Kube)?;
        serde_json::to_value(pod)
            .map(crate::json::sorted)
            .map_err(K8sError::Serde)
    }

    /// Generic describe — returns full JSON for any resource kind.
    pub async fn describe_resource(
        &self,
        kind: ResourceKind,
        namespace: String,
        name: String,
    ) -> Result<serde_json::Value> {
        let t0 = std::time::Instant::now();
        let res = self
            .describe_resource_inner(kind, namespace, name.clone())
            .await;
        let ms = t0.elapsed().as_millis() as u64;
        if ms > 2000 {
            tracing::warn!(kind = kind.as_str(), %name, ms, ok = res.is_ok(), "slow describe_resource");
        }
        res
    }

    async fn describe_resource_inner(
        &self,
        kind: ResourceKind,
        namespace: String,
        name: String,
    ) -> Result<serde_json::Value> {
        let client = self.current_client().await?;
        match kind {
            ResourceKind::Pods => {
                let api: Api<k8s_openapi::api::core::v1::Pod> = Api::namespaced(client, &namespace);
                let obj = api.get(&name).await.map_err(K8sError::Kube)?;
                serde_json::to_value(obj)
                    .map(crate::json::sorted)
                    .map_err(K8sError::Serde)
            }
            ResourceKind::Deployments => {
                let api: Api<k8s_openapi::api::apps::v1::Deployment> =
                    Api::namespaced(client, &namespace);
                let obj = api.get(&name).await.map_err(K8sError::Kube)?;
                serde_json::to_value(obj)
                    .map(crate::json::sorted)
                    .map_err(K8sError::Serde)
            }
            ResourceKind::Services => {
                let api: Api<k8s_openapi::api::core::v1::Service> =
                    Api::namespaced(client, &namespace);
                let obj = api.get(&name).await.map_err(K8sError::Kube)?;
                serde_json::to_value(obj)
                    .map(crate::json::sorted)
                    .map_err(K8sError::Serde)
            }
            ResourceKind::Nodes => {
                let api: Api<k8s_openapi::api::core::v1::Node> = Api::all(client);
                let obj = api.get(&name).await.map_err(K8sError::Kube)?;
                serde_json::to_value(obj)
                    .map(crate::json::sorted)
                    .map_err(K8sError::Serde)
            }
            ResourceKind::Events => {
                let api: Api<k8s_openapi::api::core::v1::Event> =
                    Api::namespaced(client, &namespace);
                let obj = api.get(&name).await.map_err(K8sError::Kube)?;
                serde_json::to_value(obj)
                    .map(crate::json::sorted)
                    .map_err(K8sError::Serde)
            }
            ResourceKind::Configmaps => {
                let api: Api<k8s_openapi::api::core::v1::ConfigMap> =
                    Api::namespaced(client, &namespace);
                let obj = api.get(&name).await.map_err(K8sError::Kube)?;
                serde_json::to_value(obj)
                    .map(crate::json::sorted)
                    .map_err(K8sError::Serde)
            }
            ResourceKind::Secrets => {
                let api: Api<k8s_openapi::api::core::v1::Secret> =
                    Api::namespaced(client, &namespace);
                let obj = api.get(&name).await.map_err(K8sError::Kube)?;
                serde_json::to_value(obj)
                    .map(crate::json::sorted)
                    .map_err(K8sError::Serde)
            }
            ResourceKind::Ingresses => {
                let api: Api<k8s_openapi::api::networking::v1::Ingress> =
                    Api::namespaced(client, &namespace);
                let obj = api.get(&name).await.map_err(K8sError::Kube)?;
                serde_json::to_value(obj)
                    .map(crate::json::sorted)
                    .map_err(K8sError::Serde)
            }
            ResourceKind::Jobs => {
                let api: Api<k8s_openapi::api::batch::v1::Job> =
                    Api::namespaced(client, &namespace);
                let obj = api.get(&name).await.map_err(K8sError::Kube)?;
                serde_json::to_value(obj)
                    .map(crate::json::sorted)
                    .map_err(K8sError::Serde)
            }
            ResourceKind::Cronjobs => {
                let api: Api<k8s_openapi::api::batch::v1::CronJob> =
                    Api::namespaced(client, &namespace);
                let obj = api.get(&name).await.map_err(K8sError::Kube)?;
                serde_json::to_value(obj)
                    .map(crate::json::sorted)
                    .map_err(K8sError::Serde)
            }
            ResourceKind::Namespaces => {
                let api: Api<k8s_openapi::api::core::v1::Namespace> = Api::all(client);
                let obj = api.get(&name).await.map_err(K8sError::Kube)?;
                serde_json::to_value(obj)
                    .map(crate::json::sorted)
                    .map_err(K8sError::Serde)
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
        let ctx = self.current_context_name().await.unwrap_or_default();
        let api: Api<k8s_openapi::api::core::v1::Event> = Api::namespaced(client, &namespace);

        let field_selector =
            format!("involvedObject.name={name},involvedObject.namespace={namespace}");
        let lp = ListParams::default().fields(&field_selector);

        let list = api.list(&lp).await.map_err(K8sError::Kube)?;
        let items: Vec<serde_json::Value> = list
            .items
            .iter()
            .map(|obj| {
                crate::json::sorted(
                    serde_json::to_value(obj).unwrap_or_else(|_| json!({ "name": obj.name_any() })),
                )
            })
            .collect();

        // Deliberate Fix 10: events seen by the Events tab are persisted too, so
        // History mode has them even if the Events table was never opened.
        if self.event_store().is_some() {
            let st = self.clone();
            let copy = items.clone();
            tokio::spawn(async move { st.record_events_in(copy, ctx).await });
        }
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
        let api: Api<k8s_openapi::api::apps::v1::Deployment> = Api::namespaced(client, &namespace);

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
    pub async fn restart_deployment(&self, namespace: String, name: String) -> Result<()> {
        let client = self.current_client().await?;
        let api: Api<k8s_openapi::api::apps::v1::Deployment> = Api::namespaced(client, &namespace);

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

        // Not `.force()`: kube-rs rejects force for merge patches, which would
        // make restart fail.
        let pp = kube::api::PatchParams {
            field_manager: Some("kore".to_string()),
            ..Default::default()
        };
        api.patch(&name, &pp, &kube::api::Patch::Merge(&patch))
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
        let (
            pods,
            deployments,
            services,
            configmaps,
            secrets,
            jobs,
            cronjobs,
            ingresses,
            namespaces,
        ) = tokio::join!(
            self.list_resources(ResourceKind::Pods, namespace.clone(), None),
            self.list_resources(ResourceKind::Deployments, namespace.clone(), None),
            self.list_resources(ResourceKind::Services, namespace.clone(), None),
            self.list_resources(ResourceKind::Configmaps, namespace.clone(), None),
            self.list_resources(ResourceKind::Secrets, namespace.clone(), None),
            self.list_resources(ResourceKind::Jobs, namespace.clone(), None),
            self.list_resources(ResourceKind::Cronjobs, namespace.clone(), None),
            self.list_resources(ResourceKind::Ingresses, namespace.clone(), None),
            self.list_resources(ResourceKind::Namespaces, None, None),
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
            ("namespaces", namespaces),
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
                        if let Some(obj) = item.as_object_mut() {
                            obj.insert("_kind".to_string(), json!(kind_name));
                        }
                        results.push(item);
                    }
                }
            }
        }

        results.truncate(MAX_SEARCH_RESULTS);

        Ok(results)
    }
}

/// Namespaced `Api`, or all namespaces for `None`.
fn ns_api<K>(client: kube::Client, ns: Option<&str>) -> Api<K>
where
    K: Resource<Scope = kube::core::NamespaceResourceScope>,
    <K as Resource>::DynamicType: Default,
{
    match ns {
        Some(n) => Api::namespaced(client, n),
        None => Api::all(client),
    }
}

/// List every object of `api` (paginated), as key-sorted JSON.
async fn list_api<K>(api: Api<K>, label_selector: Option<&str>) -> Result<ResourceList>
where
    K: Clone + serde::de::DeserializeOwned + serde::Serialize + Resource + std::fmt::Debug,
    <K as Resource>::DynamicType: Default,
{
    let mut base = ListParams::default().limit(LIST_PAGE_SIZE);
    if let Some(labels) = label_selector {
        base = base.labels(labels);
    }
    let (objs, truncated) = paginate(
        MAX_LIST_TOTAL,
        LIST_PAGE_TIMEOUT,
        LIST_PAGE_ATTEMPTS,
        |token| {
            let api = api.clone();
            let mut lp = base.clone();
            lp.continue_token = token;
            async move {
                let list = api.list(&lp).await.map_err(K8sError::Kube)?;
                Ok((list.items, list.metadata.continue_))
            }
        },
    )
    .await?;
    let items = objs
        .iter()
        .map(|obj| {
            crate::json::sorted(
                serde_json::to_value(obj).unwrap_or_else(|_| json!({ "name": obj.name_any() })),
            )
        })
        .collect();
    Ok(ResourceList { items, truncated })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    type Page = Result<(Vec<usize>, Option<String>)>;

    /// A fake API with `total` objects served in pages of `page`; records the
    /// tokens it was asked for.
    fn fake(
        total: usize,
        page: usize,
        calls: Arc<Mutex<Vec<Option<String>>>>,
    ) -> impl FnMut(Option<String>) -> std::future::Ready<Page> {
        move |token| {
            calls.lock().unwrap().push(token.clone());
            let start: usize = token.map(|t| t.parse().unwrap()).unwrap_or(0);
            let end = (start + page).min(total);
            let next = (end < total).then(|| end.to_string());
            std::future::ready(Ok(((start..end).collect(), next)))
        }
    }

    #[test]
    fn paginate_follows_continue_tokens() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let (items, truncated) = rt()
            .block_on(paginate(
                10_000,
                Duration::from_secs(5),
                3,
                fake(1234, 500, calls.clone()),
            ))
            .unwrap();
        assert_eq!(
            items.len(),
            1234,
            "all objects listed, not just the first page"
        );
        assert_eq!(items, (0..1234).collect::<Vec<_>>());
        assert!(!truncated);
        assert_eq!(
            *calls.lock().unwrap(),
            vec![None, Some("500".into()), Some("1000".into())]
        );
    }

    #[test]
    fn paginate_single_page_and_empty_token() {
        let (items, truncated) = rt()
            .block_on(paginate(
                10,
                Duration::from_secs(5),
                1,
                |_t: Option<String>| std::future::ready(Ok((vec![1, 2, 3], Some(String::new())))),
            ))
            .unwrap();
        assert_eq!(items, vec![1, 2, 3]);
        assert!(!truncated, "an empty continue token means done");
    }

    #[test]
    fn paginate_stops_at_cap_and_flags_truncation() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let (items, truncated) = rt()
            .block_on(paginate(
                1000,
                Duration::from_secs(5),
                3,
                fake(5000, 500, calls.clone()),
            ))
            .unwrap();
        assert_eq!(items.len(), 1000);
        assert!(truncated);
        assert_eq!(
            calls.lock().unwrap().len(),
            2,
            "no pages fetched past the cap"
        );

        // Exactly `cap` objects in total is not truncation.
        let (items, truncated) = rt()
            .block_on(paginate(
                1000,
                Duration::from_secs(5),
                3,
                fake(1000, 500, Default::default()),
            ))
            .unwrap();
        assert_eq!(items.len(), 1000);
        assert!(!truncated);
    }

    #[test]
    fn paginate_retries_a_hung_page() {
        let n = Arc::new(Mutex::new(0u32));
        let n2 = n.clone();
        let (items, _) = rt()
            .block_on(paginate(
                100,
                Duration::from_millis(50),
                3,
                move |_t: Option<String>| {
                    let first = {
                        let mut n = n2.lock().unwrap();
                        *n += 1;
                        *n == 1
                    };
                    async move {
                        if first {
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                        Ok((vec![7], None))
                    }
                },
            ))
            .unwrap();
        assert_eq!(items, vec![7]);
        assert_eq!(*n.lock().unwrap(), 2);

        let err = rt()
            .block_on(paginate(
                100,
                Duration::from_millis(20),
                2,
                |_t: Option<String>| async {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    Ok::<_, K8sError>((vec![0u8], None))
                },
            ))
            .unwrap_err();
        assert!(
            err.to_string().contains("timed out after 2 attempts"),
            "{err}"
        );
    }

    #[test]
    fn paginate_restarts_once_on_expired_token() {
        let gone = || {
            K8sError::Kube(kube::Error::Api(kube::core::ErrorResponse {
                status: "Failure".into(),
                message: "continue token expired".into(),
                reason: "Expired".into(),
                code: 410,
            }))
        };
        let calls = Arc::new(Mutex::new(Vec::<Option<String>>::new()));
        let c = calls.clone();
        let mut expired_once = false;
        let (items, _) = rt()
            .block_on(paginate(
                100,
                Duration::from_secs(5),
                1,
                move |t: Option<String>| {
                    c.lock().unwrap().push(t.clone());
                    let r = match t.as_deref() {
                        None => Ok((vec![1, 2], Some("p2".to_string()))),
                        Some(_) if !expired_once => {
                            expired_once = true;
                            Err(gone())
                        }
                        Some(_) => Ok((vec![3], None)),
                    };
                    std::future::ready(r)
                },
            ))
            .unwrap();
        assert_eq!(items, vec![1, 2, 3], "restarted list has no duplicates");
        assert_eq!(calls.lock().unwrap().len(), 4);

        // A second expiry is surfaced, not retried forever.
        let err = rt()
            .block_on(paginate(
                100,
                Duration::from_secs(5),
                1,
                move |t: Option<String>| {
                    std::future::ready(match t {
                        None => Ok((vec![1], Some("p2".to_string()))),
                        Some(_) => Err(gone()),
                    })
                },
            ))
            .unwrap_err();
        assert!(is_expired(&err));
    }
}
