pub mod ai;
pub mod crd;
pub mod dashboard;
pub mod debug;
pub mod event_store;
pub mod exec;
pub mod graph;
pub mod helm;
pub mod logs;
pub mod metrics;
pub mod multi_cluster;
pub mod multi_logs;
pub mod network_policy;
pub mod port_forward;
pub mod rbac;
pub mod resources;
pub mod rollback;
pub mod update;
pub mod watcher;
pub mod yaml;

use crate::error::{classify_connection_error, ConnectionStatus, K8sError, Result};
use kube::{
    config::{KubeConfigOptions, Kubeconfig},
    Client, Config,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

use self::event_store::EventStore;
use self::exec::ExecManager;
use self::logs::LogStreamer;
use self::port_forward::PortForwardManager;
use self::watcher::WatchManager;

#[derive(Debug, Clone, Serialize)]
pub struct WatchEventPayload {
    pub action: String,
    pub kind: String,
    pub object: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceKind {
    Pods,
    Deployments,
    Services,
    Nodes,
    Events,
    Configmaps,
    Secrets,
    Ingresses,
    Jobs,
    Cronjobs,
    Namespaces,
}

pub(crate) struct StateInner {
    pub client: Option<Client>,
    pub kubeconfig: Option<Kubeconfig>,
    pub current_context: Option<String>,
    pub connection_error: Option<K8sError>,
}

#[derive(Clone)]
pub struct K8sState {
    pub(crate) inner: Arc<RwLock<StateInner>>,
    pub(crate) watcher: WatchManager,
    pub(crate) logs: LogStreamer,
    pub(crate) port_forwards: PortForwardManager,
    pub(crate) exec_sessions: ExecManager,
    pub(crate) event_store: Option<EventStore>,
}

impl K8sState {
    /// Initialize state. Always succeeds — stores connection errors internally
    /// so the frontend can display them instead of crashing.
    pub async fn new() -> Self {
        // Initialize event store (independent of kubeconfig)
        let event_store = dirs::data_dir()
            .map(|d| d.join("kore"))
            .and_then(|data_dir| {
                EventStore::new(data_dir)
                    .map_err(|e| {
                        warn!(error = %e, "Failed to initialize event store");
                        e
                    })
                    .ok()
            });

        // Try to read kubeconfig and connect
        let (client, kubeconfig, ctx_name, connection_error) = match Kubeconfig::read() {
            Ok(kc) => {
                let ctx = kc
                    .current_context
                    .clone()
                    .or_else(|| kc.contexts.first().map(|c| c.name.clone()));
                match Self::client_for_context(&kc, ctx.clone()).await {
                    Ok(c) => {
                        info!(context = ?ctx, "Initialized K8sState");
                        (Some(c), Some(kc), ctx, None)
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to create Kubernetes client");
                        (None, Some(kc), ctx, Some(e))
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "Failed to read kubeconfig");
                (None, None, None, Some(K8sError::Kubeconfig(e)))
            }
        };

        Self {
            inner: Arc::new(RwLock::new(StateInner {
                client,
                kubeconfig,
                current_context: ctx_name,
                connection_error,
            })),
            watcher: WatchManager::new(),
            logs: LogStreamer::new(),
            port_forwards: PortForwardManager::new(),
            exec_sessions: ExecManager::new(),
            event_store,
        }
    }

    pub(crate) async fn client_for_context(
        kubeconfig: &Kubeconfig,
        context: Option<String>,
    ) -> Result<Client> {
        let opts = KubeConfigOptions {
            context,
            ..Default::default()
        };
        let config = Config::from_custom_kubeconfig(kubeconfig.clone(), &opts).await?;
        Client::try_from(config).map_err(K8sError::Kube)
    }

    pub async fn reload_kubeconfig(&self) -> Result<Kubeconfig> {
        let cfg = Kubeconfig::read()?;
        Ok(cfg)
    }

    pub async fn get_connection_status(&self) -> ConnectionStatus {
        let inner = self.inner.read().await;

        let kubeconfig_path = std::env::var("KUBECONFIG").ok().or_else(|| {
            dirs::home_dir().map(|h| h.join(".kube/config").to_string_lossy().into_owned())
        });

        let contexts_available: Vec<String> = inner
            .kubeconfig
            .as_ref()
            .map(|kc| kc.contexts.iter().map(|c| c.name.clone()).collect())
            .unwrap_or_default();

        match &inner.connection_error {
            None if inner.client.is_some() => ConnectionStatus {
                connected: true,
                error: None,
                error_kind: None,
                kubeconfig_path,
                contexts_available,
                current_context: inner.current_context.clone(),
            },
            Some(err) => ConnectionStatus {
                connected: false,
                error: Some(err.to_string()),
                error_kind: Some(classify_connection_error(err).to_string()),
                kubeconfig_path,
                contexts_available,
                current_context: inner.current_context.clone(),
            },
            None => ConnectionStatus {
                connected: false,
                error: Some("Client not initialized".to_string()),
                error_kind: Some("unknown".to_string()),
                kubeconfig_path,
                contexts_available,
                current_context: inner.current_context.clone(),
            },
        }
    }

    pub async fn retry_connection(&self, context: Option<String>) -> ConnectionStatus {
        // Cancel any active operations first
        self.watcher.cancel_all().await;
        self.logs.cancel().await;
        self.exec_sessions.cancel_all().await;

        let result = match Kubeconfig::read() {
            Ok(kc) => {
                let ctx = context
                    .or_else(|| kc.current_context.clone())
                    .or_else(|| kc.contexts.first().map(|c| c.name.clone()));
                match Self::client_for_context(&kc, ctx.clone()).await {
                    Ok(c) => {
                        info!(context = ?ctx, "Reconnected successfully");
                        (Some(c), Some(kc), ctx, None)
                    }
                    Err(e) => {
                        warn!(error = %e, "Retry: failed to create client");
                        (None, Some(kc), ctx, Some(e))
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "Retry: failed to read kubeconfig");
                (None, None, None, Some(K8sError::Kubeconfig(e)))
            }
        };

        let mut inner = self.inner.write().await;
        inner.client = result.0;
        inner.kubeconfig = result.1;
        inner.current_context = result.2;
        inner.connection_error = result.3;
        drop(inner);

        self.get_connection_status().await
    }

    pub async fn list_contexts(&self) -> Result<Vec<String>> {
        let inner = self.inner.read().await;
        // Try stored kubeconfig first
        if let Some(kc) = &inner.kubeconfig {
            return Ok(kc.contexts.iter().map(|c| c.name.clone()).collect());
        }
        drop(inner);
        // Fallback: try reading kubeconfig fresh (may have been fixed since startup)
        match Kubeconfig::read() {
            Ok(kc) => Ok(kc.contexts.iter().map(|c| c.name.clone()).collect()),
            Err(_) => Ok(vec![]),
        }
    }

    pub async fn list_namespaces(&self) -> Result<Vec<String>> {
        let client = self.current_client().await?;
        let api: kube::Api<k8s_openapi::api::core::v1::Namespace> = kube::Api::all(client);
        let list = api
            .list(&kube::api::ListParams::default())
            .await
            .map_err(K8sError::Kube)?;
        Ok(list
            .items
            .iter()
            .map(kube::api::ResourceExt::name_any)
            .collect())
    }

    pub async fn switch_context(&self, name: String) -> Result<String> {
        // Cancel all active operations before switching
        self.watcher.cancel_all().await;
        self.logs.cancel().await;
        self.exec_sessions.cancel_all().await;

        let kubeconfig = self.reload_kubeconfig().await?;
        let client = Self::client_for_context(&kubeconfig, Some(name.clone())).await?;
        let mut inner = self.inner.write().await;
        inner.client = Some(client);
        inner.kubeconfig = Some(kubeconfig);
        inner.current_context = Some(name.clone());
        info!(context = %name, "Switched context");
        Ok(name)
    }

    pub async fn current_client(&self) -> Result<Client> {
        let inner = self.inner.read().await;
        inner.client.clone().ok_or(K8sError::ClientMissing)
    }

    pub async fn current_context_name(&self) -> Option<String> {
        let inner = self.inner.read().await;
        inner.current_context.clone()
    }

    /// Graceful shutdown — cancel all watches, log streams, port forwards, and exec sessions.
    pub async fn shutdown(&self) {
        warn!("Shutting down K8sState — cancelling all operations");
        self.watcher.cancel_all().await;
        self.logs.cancel().await;
        self.port_forwards.cancel_all().await;
        self.exec_sessions.cancel_all().await;
    }
}
