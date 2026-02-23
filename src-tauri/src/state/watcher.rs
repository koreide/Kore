use crate::constants::{INITIAL_RECONNECT_DELAY, MAX_RECONNECT_DELAY};
use crate::error::Result;
use crate::state::{K8sState, ResourceKind, WatchEventPayload};
use futures::{StreamExt, TryStreamExt};
use kube::api::{Api, Resource, ResourceExt};
use kube::core::NamespaceResourceScope;
use kube::runtime::watcher;
use rand::Rng;
use serde_json::json;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

/// Manages watch lifecycle with proper cancellation.
#[derive(Clone)]
pub struct WatchManager {
    cancel_token: Arc<RwLock<Option<CancellationToken>>>,
}

impl WatchManager {
    pub fn new() -> Self {
        Self {
            cancel_token: Arc::new(RwLock::new(None)),
        }
    }

    /// Cancel all active watches.
    pub async fn cancel_all(&self) {
        let mut token = self.cancel_token.write().await;
        if let Some(ct) = token.take() {
            ct.cancel();
            info!("Cancelled all active watches");
        }
    }
}

impl K8sState {
    pub async fn start_watch(
        &self,
        app: AppHandle,
        kind: ResourceKind,
        namespace: Option<String>,
        label_selector: Option<String>,
    ) -> Result<()> {
        // Atomic: cancel old watch and store new token in a single lock acquisition
        let new_token = {
            let mut token = self.watcher.cancel_token.write().await;
            if let Some(old_token) = token.take() {
                old_token.cancel();
            }
            let new_token = CancellationToken::new();
            *token = Some(new_token.clone());
            new_token
        };

        match kind {
            ResourceKind::Pods => {
                self.watch_namespaced::<k8s_openapi::api::core::v1::Pod>(
                    app, "pods", namespace, new_token, label_selector,
                )
                .await?
            }
            ResourceKind::Deployments => {
                self.watch_namespaced::<k8s_openapi::api::apps::v1::Deployment>(
                    app, "deployments", namespace, new_token, label_selector,
                )
                .await?
            }
            ResourceKind::Services => {
                self.watch_namespaced::<k8s_openapi::api::core::v1::Service>(
                    app, "services", namespace, new_token, label_selector,
                )
                .await?
            }
            ResourceKind::Nodes => {
                self.watch_cluster_scoped::<k8s_openapi::api::core::v1::Node>(
                    app, "nodes", new_token, label_selector,
                )
                .await?
            }
            ResourceKind::Events => {
                self.watch_namespaced::<k8s_openapi::api::core::v1::Event>(
                    app, "events", namespace, new_token, label_selector,
                )
                .await?
            }
            ResourceKind::Configmaps => {
                self.watch_namespaced::<k8s_openapi::api::core::v1::ConfigMap>(
                    app, "configmaps", namespace, new_token, label_selector,
                )
                .await?
            }
            ResourceKind::Secrets => {
                self.watch_namespaced::<k8s_openapi::api::core::v1::Secret>(
                    app, "secrets", namespace, new_token, label_selector,
                )
                .await?
            }
            ResourceKind::Ingresses => {
                self.watch_namespaced::<k8s_openapi::api::networking::v1::Ingress>(
                    app, "ingresses", namespace, new_token, label_selector,
                )
                .await?
            }
            ResourceKind::Jobs => {
                self.watch_namespaced::<k8s_openapi::api::batch::v1::Job>(
                    app, "jobs", namespace, new_token, label_selector,
                )
                .await?
            }
            ResourceKind::Cronjobs => {
                self.watch_namespaced::<k8s_openapi::api::batch::v1::CronJob>(
                    app, "cronjobs", namespace, new_token, label_selector,
                )
                .await?
            }
        }
        Ok(())
    }

    async fn watch_namespaced<K>(
        &self,
        app: AppHandle,
        kind: &str,
        ns: Option<String>,
        cancel_token: CancellationToken,
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

        spawn_watcher(api, app, kind, cancel_token, label_selector);
        Ok(())
    }

    async fn watch_cluster_scoped<K>(
        &self,
        app: AppHandle,
        kind: &str,
        cancel_token: CancellationToken,
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
        spawn_watcher(api, app, kind, cancel_token, label_selector);
        Ok(())
    }
}

fn spawn_watcher<K>(
    api: Api<K>,
    app: AppHandle,
    kind: &str,
    cancel_token: CancellationToken,
    label_selector: Option<String>,
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
    let handle = app.clone();
    let kind_name = kind.to_string();
    let api_clone = api.clone();

    tauri::async_runtime::spawn(async move {
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
                                let payload = WatchEventPayload {
                                    action: "applied".into(),
                                    kind: kind_name.clone(),
                                    object: serde_json::to_value(&obj).unwrap_or_else(|_| json!({ "name": obj.name_any() })),
                                };
                                if let Err(e) = handle.emit("resource://event", &payload) {
                                    error!(kind = %kind_name, error = %e, "Failed to emit applied event");
                                }
                                reconnect_delay = INITIAL_RECONNECT_DELAY;
                            }
                            Ok(Some(watcher::Event::Deleted(obj))) => {
                                let payload = WatchEventPayload {
                                    action: "deleted".into(),
                                    kind: kind_name.clone(),
                                    object: serde_json::to_value(&obj).unwrap_or_else(|_| json!({ "name": obj.name_any() })),
                                };
                                if let Err(e) = handle.emit("resource://event", &payload) {
                                    error!(kind = %kind_name, error = %e, "Failed to emit deleted event");
                                }
                            }
                            Ok(Some(watcher::Event::Restarted(objs))) => {
                                for obj in objs {
                                    let payload = WatchEventPayload {
                                        action: "applied".into(),
                                        kind: kind_name.clone(),
                                        object: serde_json::to_value(&obj)
                                            .unwrap_or_else(|_| json!({ "name": obj.name_any() })),
                                    };
                                    if let Err(e) = handle.emit("resource://event", &payload) {
                                        error!(kind = %kind_name, error = %e, "Failed to emit restarted event");
                                    }
                                }
                            }
                            Ok(None) => {
                                warn!(kind = %kind_name, "Stream ended, reconnecting");
                                break;
                            }
                            Err(err) => {
                                warn!(kind = %kind_name, error = %err, "Watch error, reconnecting");
                                let payload = WatchEventPayload {
                                    action: "error".into(),
                                    kind: kind_name.clone(),
                                    object: json!({ "message": err.to_string() }),
                                };
                                if let Err(e) = handle.emit("resource://event", &payload) {
                                    error!(kind = %kind_name, error = %e, "Failed to emit error event");
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
