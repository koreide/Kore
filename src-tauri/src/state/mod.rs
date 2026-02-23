pub mod ai;
pub mod crd;
pub mod dashboard;
pub mod event_store;
pub mod exec;
pub mod graph;
pub mod helm;
pub mod logs;
pub mod metrics;
pub mod multi_cluster;
pub mod multi_logs;
pub mod port_forward;
pub mod resources;
pub mod rollback;
pub mod watcher;
pub mod yaml;

use crate::error::{K8sError, Result};
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
}

pub(crate) struct StateInner {
    pub client: Option<Client>,
    pub kubeconfig: Option<Kubeconfig>,
    pub current_context: Option<String>,
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
    pub async fn new() -> Result<Self> {
        let kubeconfig = Kubeconfig::read()?;
        let ctx_name = kubeconfig
            .current_context
            .clone()
            .or_else(|| kubeconfig.contexts.first().map(|c| c.name.clone()));
        let client = Self::client_for_context(&kubeconfig, ctx_name.clone()).await?;

        // Initialize event store
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

        info!(context = ?ctx_name, "Initialized K8sState");

        Ok(Self {
            inner: Arc::new(RwLock::new(StateInner {
                client: Some(client),
                kubeconfig: Some(kubeconfig),
                current_context: ctx_name,
            })),
            watcher: WatchManager::new(),
            logs: LogStreamer::new(),
            port_forwards: PortForwardManager::new(),
            exec_sessions: ExecManager::new(),
            event_store,
        })
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

    pub async fn list_contexts(&self) -> Result<Vec<String>> {
        let inner = self.inner.read().await;
        let kubeconfig = inner.kubeconfig.clone().ok_or(K8sError::ClientMissing)?;
        Ok(kubeconfig.contexts.iter().map(|c| c.name.clone()).collect())
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
