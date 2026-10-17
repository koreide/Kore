use crate::constants::{INITIAL_RECONNECT_DELAY, MAX_RECONNECT_DELAY};
use crate::error::Result;
use crate::events::{channel, Sink, Subscription};
use crate::state::{K8sState, ResourceKind, WatchEventPayload};
use futures::{StreamExt, TryStreamExt};
use kube::api::{Api, Resource, ResourceExt};
use kube::core::NamespaceResourceScope;
use kube::runtime::watcher;
use rand::Rng;
use serde_json::json;
use tracing::{debug, info, warn};

impl K8sState {
    /// Start a watch. Every call returns an independent [`Subscription`];
    /// drop it to stop the watch.
    /// Context switch / retry / shutdown cancel all watches.
    pub async fn start_watch(
        &self,
        kind: ResourceKind,
        namespace: Option<String>,
        label_selector: Option<String>,
    ) -> Result<Subscription<WatchEventPayload>> {
        let (sink, sub) = channel::<WatchEventPayload>(&self.watch_scope());
        let app = sink;
        // A3: every payload says which context/namespace it came from.
        let context = self.current_context_name().await.unwrap_or_default();
        let scope = WatchScope {
            context,
            namespace: if kind.is_cluster_scoped() {
                None
            } else {
                namespace.clone()
            },
        };

        match kind {
            ResourceKind::Pods => {
                self.watch_namespaced::<k8s_openapi::api::core::v1::Pod>(
                    app,
                    scope.clone(),
                    "pods",
                    namespace,
                    label_selector,
                )
                .await?
            }
            ResourceKind::Deployments => {
                self.watch_namespaced::<k8s_openapi::api::apps::v1::Deployment>(
                    app,
                    scope.clone(),
                    "deployments",
                    namespace,
                    label_selector,
                )
                .await?
            }
            ResourceKind::Services => {
                self.watch_namespaced::<k8s_openapi::api::core::v1::Service>(
                    app,
                    scope.clone(),
                    "services",
                    namespace,
                    label_selector,
                )
                .await?
            }
            ResourceKind::Nodes => {
                self.watch_cluster_scoped::<k8s_openapi::api::core::v1::Node>(
                    app,
                    scope.clone(),
                    "nodes",
                    label_selector,
                )
                .await?
            }
            ResourceKind::Events => {
                self.watch_namespaced::<k8s_openapi::api::core::v1::Event>(
                    app,
                    scope.clone(),
                    "events",
                    namespace,
                    label_selector,
                )
                .await?
            }
            ResourceKind::Configmaps => {
                self.watch_namespaced::<k8s_openapi::api::core::v1::ConfigMap>(
                    app,
                    scope.clone(),
                    "configmaps",
                    namespace,
                    label_selector,
                )
                .await?
            }
            ResourceKind::Secrets => {
                self.watch_namespaced::<k8s_openapi::api::core::v1::Secret>(
                    app,
                    scope.clone(),
                    "secrets",
                    namespace,
                    label_selector,
                )
                .await?
            }
            ResourceKind::Ingresses => {
                self.watch_namespaced::<k8s_openapi::api::networking::v1::Ingress>(
                    app,
                    scope.clone(),
                    "ingresses",
                    namespace,
                    label_selector,
                )
                .await?
            }
            ResourceKind::Jobs => {
                self.watch_namespaced::<k8s_openapi::api::batch::v1::Job>(
                    app,
                    scope.clone(),
                    "jobs",
                    namespace,
                    label_selector,
                )
                .await?
            }
            ResourceKind::Cronjobs => {
                self.watch_namespaced::<k8s_openapi::api::batch::v1::CronJob>(
                    app,
                    scope.clone(),
                    "cronjobs",
                    namespace,
                    label_selector,
                )
                .await?
            }
            ResourceKind::Namespaces => {
                self.watch_cluster_scoped::<k8s_openapi::api::core::v1::Namespace>(
                    app,
                    scope.clone(),
                    "namespaces",
                    label_selector,
                )
                .await?
            }
        }
        Ok(sub)
    }

    async fn watch_namespaced<K>(
        &self,
        app: Sink<WatchEventPayload>,
        scope: WatchScope,
        kind: &str,
        ns: Option<String>,
        label_selector: Option<String>,
    ) -> Result<()>
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

        spawn_watcher(api, app, scope, kind, label_selector, self.clone());
        Ok(())
    }

    async fn watch_cluster_scoped<K>(
        &self,
        app: Sink<WatchEventPayload>,
        scope: WatchScope,
        kind: &str,
        label_selector: Option<String>,
    ) -> Result<()>
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
        spawn_watcher(api, app, scope, kind, label_selector, self.clone());
        Ok(())
    }
}

/// Context and namespace a watch was started for (copied into payloads).
#[derive(Clone)]
struct WatchScope {
    context: String,
    namespace: Option<String>,
}

impl WatchScope {
    fn payload(&self, action: &str, kind: &str, object: serde_json::Value) -> WatchEventPayload {
        WatchEventPayload {
            action: action.into(),
            kind: kind.into(),
            object,
            context: self.context.clone(),
            namespace: self.namespace.clone(),
        }
    }
}

fn spawn_watcher<K>(
    api: Api<K>,
    app: Sink<WatchEventPayload>,
    scope: WatchScope,
    kind: &str,
    label_selector: Option<String>,
    state: K8sState,
) where
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
    let cancel_token = app.token().clone();
    let handle = app;
    let kind_name = kind.to_string();
    let api_clone = api.clone();
    let record_events = kind_name == "events";

    tokio::spawn(async move {
        let mut reconnect_delay = INITIAL_RECONNECT_DELAY;

        loop {
            // Check cancellation before starting a new watch
            if cancel_token.is_cancelled() {
                info!(kind = %kind_name, "Watch cancelled");
                break;
            }

            let mut watcher_config = watcher::Config::default();
            if let Some(ref labels) = label_selector {
                watcher_config = watcher_config.labels(labels);
            }

            let mut stream = watcher(api_clone.clone(), watcher_config).boxed();

            info!(kind = %kind_name, "Started watch");

            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        info!(kind = %kind_name, "Watch cancelled");
                        return;
                    }
                    evt_result = stream.try_next() => {
                        match evt_result {
                            Ok(Some(watcher::Event::Applied(obj))) => {
                                let payload = scope.payload(
                                    "applied",
                                    &kind_name,
                                    serde_json::to_value(&obj).unwrap_or_else(|_| json!({ "name": obj.name_any() })),
                                );
                                if record_events {
                                    let st = state.clone();
                                    let obj = payload.object.clone();
                                    let ctx = scope.context.clone();
                                    tokio::spawn(async move { st.record_events_in(vec![obj], ctx).await });
                                }
                                if handle.send(payload).is_err() {
                                    info!(kind = %kind_name, "Watch subscriber gone");
                                    return;
                                }
                                reconnect_delay = INITIAL_RECONNECT_DELAY;
                            }
                            Ok(Some(watcher::Event::Deleted(obj))) => {
                                let payload = scope.payload(
                                    "deleted",
                                    &kind_name,
                                    serde_json::to_value(&obj).unwrap_or_else(|_| json!({ "name": obj.name_any() })),
                                );
                                if handle.send(payload).is_err() {
                                    info!(kind = %kind_name, "Watch subscriber gone");
                                    return;
                                }
                            }
                            Ok(Some(watcher::Event::Restarted(objs))) => {
                                if record_events {
                                    let st = state.clone();
                                    let vals: Vec<serde_json::Value> = objs
                                        .iter()
                                        .filter_map(|o| serde_json::to_value(o).ok())
                                        .collect();
                                    let ctx = scope.context.clone();
                                    tokio::spawn(async move { st.record_events_in(vals, ctx).await });
                                }
                                for obj in objs {
                                    let payload = scope.payload(
                                        "applied",
                                        &kind_name,
                                        serde_json::to_value(&obj)
                                            .unwrap_or_else(|_| json!({ "name": obj.name_any() })),
                                    );
                                    if handle.send(payload).is_err() {
                                        info!(kind = %kind_name, "Watch subscriber gone");
                                        return;
                                    }
                                }
                            }
                            Ok(None) => {
                                warn!(kind = %kind_name, "Stream ended, reconnecting");
                                break;
                            }
                            Err(err) => {
                                warn!(kind = %kind_name, error = %err, "Watch error, reconnecting");
                                let payload = scope.payload(
                                    "error",
                                    &kind_name,
                                    json!({ "message": err.to_string() }),
                                );
                                if handle.send(payload).is_err() {
                                    return;
                                }
                                break;
                            }
                        }
                    }
                }
            }

            // Reconnect with exponential backoff + jitter
            let jitter = {
                let mut rng = rand::thread_rng();
                let jitter_fraction = rng.gen_range(0.0..0.25);
                reconnect_delay.mul_f64(jitter_fraction)
            };
            let delay_with_jitter = reconnect_delay + jitter;

            debug!(kind = %kind_name, delay = ?delay_with_jitter, "Reconnecting watch");

            tokio::select! {
                _ = cancel_token.cancelled() => {
                    info!(kind = %kind_name, "Watch cancelled during reconnect backoff");
                    return;
                }
                _ = tokio::time::sleep(delay_with_jitter) => {}
            }

            reconnect_delay = std::cmp::min(reconnect_delay * 2, MAX_RECONNECT_DELAY);
        }

        info!(kind = %kind_name, "Watch task ended");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_carries_context_and_namespace() {
        let scope = WatchScope {
            context: "kind-a".into(),
            namespace: Some("ns1".into()),
        };
        let p = scope.payload("applied", "pods", json!({"metadata": {"name": "x"}}));
        assert_eq!(p.context, "kind-a");
        assert_eq!(p.namespace.as_deref(), Some("ns1"));
        assert_eq!(p.action, "applied");
        assert_eq!(p.kind, "pods");
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["context"], "kind-a");
        assert_eq!(v["namespace"], "ns1");
    }

    #[test]
    fn payload_without_scope_fields_still_deserializes() {
        let p: WatchEventPayload =
            serde_json::from_value(json!({"action": "deleted", "kind": "pods", "object": {}}))
                .unwrap();
        assert_eq!(p.context, "");
        assert_eq!(p.namespace, None);
    }
}
