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

use crate::config::CoreConfig;
use crate::error::{classify_connection_error, ConnectionStatus, K8sError, Result};
use kube::{
    config::Kubeconfig,
    Client,
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use self::event_store::EventStore;
use self::exec::ExecManager;
use self::port_forward::PortForwardManager;

/// Payload of a resource watch event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchEventPayload {
    pub action: String,
    pub kind: String,
    pub object: serde_json::Value,
    /// Kube context the watch was started in (A3).
    #[serde(default)]
    pub context: String,
    /// Namespace scope of the watch (`None` = all namespaces / cluster-scoped).
    #[serde(default)]
    pub namespace: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
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

impl ResourceKind {
    pub const ALL: [ResourceKind; 11] = [
        ResourceKind::Pods,
        ResourceKind::Deployments,
        ResourceKind::Services,
        ResourceKind::Nodes,
        ResourceKind::Events,
        ResourceKind::Configmaps,
        ResourceKind::Secrets,
        ResourceKind::Ingresses,
        ResourceKind::Jobs,
        ResourceKind::Cronjobs,
        ResourceKind::Namespaces,
    ];

    /// The lowercase plural used in payloads (`"pods"`, `"cronjobs"`, …).
    pub fn as_str(&self) -> &'static str {
        match self {
            ResourceKind::Pods => "pods",
            ResourceKind::Deployments => "deployments",
            ResourceKind::Services => "services",
            ResourceKind::Nodes => "nodes",
            ResourceKind::Events => "events",
            ResourceKind::Configmaps => "configmaps",
            ResourceKind::Secrets => "secrets",
            ResourceKind::Ingresses => "ingresses",
            ResourceKind::Jobs => "jobs",
            ResourceKind::Cronjobs => "cronjobs",
            ResourceKind::Namespaces => "namespaces",
        }
    }

    pub fn is_cluster_scoped(&self) -> bool {
        matches!(self, ResourceKind::Nodes | ResourceKind::Namespaces)
    }
}

pub(crate) struct StateInner {
    pub client: Option<Client>,
    pub kubeconfig: Option<Kubeconfig>,
    pub current_context: Option<String>,
    pub connection_error: Option<K8sError>,
}

/// Cancellation scopes for streams. Every stream token is a child of `root`
/// (cancelled on context switch / retry / shutdown). Watches and log streams
/// additionally hang off their own scope so `stop_pod_logs_stream` keeps its
/// "stop all log streams" semantics.
pub(crate) struct StreamScopes {
    root: CancellationToken,
    watches: CancellationToken,
    logs: CancellationToken,
}

impl StreamScopes {
    fn new() -> Self {
        let root = CancellationToken::new();
        Self {
            watches: root.child_token(),
            logs: root.child_token(),
            root,
        }
    }
}

#[derive(Clone)]
pub struct K8sState {
    pub(crate) inner: Arc<RwLock<StateInner>>,
    pub(crate) config: Arc<CoreConfig>,
    pub(crate) scopes: Arc<Mutex<StreamScopes>>,
    pub(crate) port_forwards: PortForwardManager,
    pub(crate) exec_sessions: ExecManager,
    pub(crate) event_store: Arc<OnceLock<Option<EventStore>>>,
    pub(crate) event_retention_days: Arc<AtomicU32>,
    ready_tx: Arc<tokio::sync::watch::Sender<bool>>,
}

impl K8sState {
    /// Initialize state with default configuration (env `KUBECONFIG` or
    /// `~/.kube/config`, OS data dir). Always succeeds — connection errors are
    /// stored and surfaced through `get_connection_status`.
    pub async fn new() -> Self {
        let state = Self::new_uninit(CoreConfig::default());
        state.init().await;
        state
    }

    /// Build the state without touching the filesystem or network. Call
    /// [`K8sState::init`] (on a tokio runtime) to connect.
    pub fn new_uninit(config: CoreConfig) -> Self {
        Self::new_uninit_shared(Arc::new(config))
    }

    pub(crate) fn new_uninit_shared(config: Arc<CoreConfig>) -> Self {
        let (ready_tx, _) = tokio::sync::watch::channel(false);
        Self {
            inner: Arc::new(RwLock::new(StateInner {
                client: None,
                kubeconfig: None,
                current_context: None,
                connection_error: None,
            })),
            event_retention_days: Arc::new(AtomicU32::new(config.event_retention_days)),
            config,
            scopes: Arc::new(Mutex::new(StreamScopes::new())),
            port_forwards: PortForwardManager::new(),
            exec_sessions: ExecManager::new(),
            event_store: Arc::new(OnceLock::new()),
            ready_tx: Arc::new(ready_tx),
        }
    }

    /// Open the event store, read the kubeconfig and build the client.
    pub async fn init(&self) {
        // Event store (independent of kubeconfig). SQLite open is blocking IO.
        let data_dir = self.config.data_dir();
        let store = tokio::task::spawn_blocking(move || {
            data_dir.and_then(|dir| {
                EventStore::new(dir)
                    .map_err(|e| {
                        warn!(error = %e, "Failed to initialize event store");
                        e
                    })
                    .ok()
            })
        })
        .await
        .ok()
        .flatten();
        if let Some(store) = &store {
            let days = self.event_retention_days.load(Ordering::Relaxed);
            if let Err(e) = store.prune(days).await {
                warn!(error = %e, "Failed to prune event store");
            }
        }
        let _ = self.event_store.set(store);

        // Try to read kubeconfig and connect
        let (client, kubeconfig, ctx_name, connection_error) = match self.read_kubeconfig() {
            Ok(kc) => {
                let ctx = self
                    .config
                    .initial_context
                    .clone()
                    .or_else(|| kc.current_context.clone())
                    .or_else(|| kc.contexts.first().map(|c| c.name.clone()));
                match self.client_for_context(&kc, ctx.clone()).await {
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
                (None, None, None, Some(e))
            }
        };

        {
            let mut inner = self.inner.write().await;
            inner.client = client;
            inner.kubeconfig = kubeconfig;
            inner.current_context = ctx_name;
            inner.connection_error = connection_error;
        }
        self.ready_tx.send_replace(true);
    }

    /// Resolves once `init` has completed.
    pub async fn wait_ready(&self) {
        let mut rx = self.ready_tx.subscribe();
        let _ = rx.wait_for(|r| *r).await;
    }

    pub fn is_ready(&self) -> bool {
        *self.ready_tx.borrow()
    }

    pub(crate) fn event_store(&self) -> Option<&EventStore> {
        self.event_store.get().and_then(|s| s.as_ref())
    }

    // ── Cancellation scopes ─────────────────────────────────────────────

    pub(crate) fn stream_root(&self) -> CancellationToken {
        self.scopes.lock().expect("scopes poisoned").root.clone()
    }

    pub(crate) fn watch_scope(&self) -> CancellationToken {
        self.scopes.lock().expect("scopes poisoned").watches.clone()
    }

    pub(crate) fn logs_scope(&self) -> CancellationToken {
        self.scopes.lock().expect("scopes poisoned").logs.clone()
    }

    /// Cancel every watch, log stream, exec session and AI stream (not port-forwards).
    pub(crate) async fn cancel_streams(&self) {
        {
            let mut scopes = self.scopes.lock().expect("scopes poisoned");
            scopes.root.cancel();
            *scopes = StreamScopes::new();
        }
        self.exec_sessions.cancel_all();
        info!("Cancelled all active streams");
    }

    /// Cancel every active log stream (single-pod and multi-pod).
    pub(crate) fn cancel_logs(&self) {
        let mut scopes = self.scopes.lock().expect("scopes poisoned");
        scopes.logs.cancel();
        scopes.logs = scopes.root.child_token();
        info!("Cancelled active log streams");
    }

    // ── Kubeconfig ──────────────────────────────────────────────────────

    /// Read the kubeconfig from the configured paths, or from `KUBECONFIG` /
    /// `~/.kube/config` when none are configured.
    pub(crate) fn read_kubeconfig(&self) -> Result<Kubeconfig> {
        if self.config.kubeconfig_paths.is_empty() {
            return Ok(Kubeconfig::read()?);
        }
        let mut merged: Option<Kubeconfig> = None;
        for p in &self.config.kubeconfig_paths {
            let kc = Kubeconfig::read_from(p)?;
            merged = Some(match merged {
                None => kc,
                Some(m) => m.merge(kc)?,
            });
        }
        merged.ok_or_else(|| K8sError::Validation("No kubeconfig paths configured".into()))
    }

    /// Apply kubeconfig env to a subprocess (kubectl/helm) so it reads the
    /// same kubeconfig as kube-rs.
    pub(crate) fn apply_kubeconfig_env(&self, cmd: &mut tokio::process::Command) {
        if !self.config.kubeconfig_paths.is_empty() {
            if let Ok(joined) = std::env::join_paths(&self.config.kubeconfig_paths) {
                cmd.env("KUBECONFIG", joined);
            }
        }
    }

    pub(crate) async fn client_for_context(
        &self,
        kubeconfig: &Kubeconfig,
        context: Option<String>,
    ) -> Result<Client> {
        self.config.connector.connect(kubeconfig, context).await
    }

    pub async fn reload_kubeconfig(&self) -> Result<Kubeconfig> {
        self.read_kubeconfig()
    }

    pub async fn get_connection_status(&self) -> ConnectionStatus {
        let inner = self.inner.read().await;

        let kubeconfig_path = if self.config.kubeconfig_paths.is_empty() {
            std::env::var("KUBECONFIG").ok().or_else(|| {
                dirs::home_dir().map(|h| h.join(".kube/config").to_string_lossy().into_owned())
            })
        } else {
            std::env::join_paths(&self.config.kubeconfig_paths)
                .ok()
                .map(|p| p.to_string_lossy().into_owned())
        };

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

    async fn probe(client: &Client) -> Option<K8sError> {
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.apiserver_version(),
        )
        .await
        {
            Ok(Ok(_)) => None,
            Ok(Err(e)) => Some(K8sError::Kube(e)),
            Err(_) => Some(K8sError::Validation(
                "Cluster did not respond within 5 seconds".into(),
            )),
        }
    }

    pub async fn retry_connection(&self, context: Option<String>) -> ConnectionStatus {
        // Cancel any active operations first
        self.cancel_streams().await;

        let result = match self.read_kubeconfig() {
            Ok(kc) => {
                let ctx = context
                    .or_else(|| kc.current_context.clone())
                    .or_else(|| kc.contexts.first().map(|c| c.name.clone()));
                match self.client_for_context(&kc, ctx.clone()).await {
                    Ok(c) => {
                        // Probe cluster reachability (5s timeout)
                        let probe_err = Self::probe(&c).await;
                        if probe_err.is_none() {
                            info!(context = ?ctx, "Reconnected successfully");
                        } else {
                            warn!(context = ?ctx, "Retry: cluster unreachable");
                        }
                        (Some(c), Some(kc), ctx, probe_err)
                    }
                    Err(e) => {
                        warn!(error = %e, "Retry: failed to create client");
                        (None, Some(kc), ctx, Some(e))
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "Retry: failed to read kubeconfig");
                (None, None, None, Some(e))
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
        match self.read_kubeconfig() {
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

    /// Switch to context `name`. Returns `Err` only when the kubeconfig cannot
    /// be read or no client can be built for the context (the previous client
    /// and its streams stay). An unreachable cluster still switches (A15): the returned
    /// [`ConnectionStatus`] (same as `get_connection_status`) carries
    /// `connected: false` and the error.
    pub async fn switch_context(&self, name: String) -> Result<ConnectionStatus> {
        // Build the new client first: if that fails, the current context and
        // its streams are left untouched.
        let kubeconfig = self.reload_kubeconfig().await?;
        let client = self.client_for_context(&kubeconfig, Some(name.clone())).await?;

        // Cancel all active operations before switching
        self.cancel_streams().await;

        // Probe cluster reachability (5s timeout)
        let probe_err = Self::probe(&client).await;
        if let Some(e) = &probe_err {
            warn!(context = %name, error = %e, "Switched context, but the cluster is unreachable");
        }

        {
            let mut inner = self.inner.write().await;
            inner.client = Some(client);
            inner.kubeconfig = Some(kubeconfig);
            inner.current_context = Some(name.clone());
            inner.connection_error = probe_err;
        }
        info!(context = %name, "Switched context");
        Ok(self.get_connection_status().await)
    }

    pub async fn current_client(&self) -> Result<Client> {
        let inner = self.inner.read().await;
        inner.client.clone().ok_or(K8sError::ClientMissing)
    }

    pub async fn current_context_name(&self) -> Option<String> {
        let inner = self.inner.read().await;
        inner.current_context.clone()
    }

    /// Persist raw Event objects into the SQLite store (Deliberate Fix 10),
    /// filed under `context` (the context the events were read from, captured
    /// by the caller, not whatever is current when this runs).
    /// Runs in the background; never fails the caller.
    pub(crate) async fn record_events_in(&self, events: Vec<serde_json::Value>, context: String) {
        if events.is_empty() {
            return;
        }
        let Some(store) = self.event_store().cloned() else {
            return;
        };
        store.store_events(events, context).await;
    }

    /// Set the event retention (days) and prune immediately.
    pub async fn set_event_retention(&self, days: u32) -> Result<u64> {
        self.event_retention_days.store(days, Ordering::Relaxed);
        match self.event_store() {
            Some(store) => store.prune(days).await,
            None => Ok(0),
        }
    }

    pub fn event_retention_days(&self) -> u32 {
        self.event_retention_days.load(Ordering::Relaxed)
    }

    /// Graceful shutdown — cancel all watches, log streams, port forwards, and exec sessions.
    pub async fn shutdown(&self) {
        warn!("Shutting down K8sState — cancelling all operations");
        self.cancel_streams().await;
        self.port_forwards.cancel_all().await;
    }
}
