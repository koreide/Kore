//! `kore-core`: the Kore Kubernetes backend (no UI dependency).
//!
//! [`Core`] owns a dedicated multi-thread tokio runtime. Every public async
//! method spawns its work onto that runtime and awaits the `JoinHandle`, so
//! the returned futures can be polled from **any** executor (GPUI,
//! `futures::executor::block_on`, another tokio runtime, …).
//!
//! Streams (watches, pod logs, multi-pod logs, exec output, AI responses)
//! are returned as [`Subscription<T>`]: a receiver plus a cancel guard.
//! Dropping the subscription cancels the backend task(s).
//!
//! ```no_run
//! use kore_core::{Core, ResourceKind};
//! let core = Core::new().unwrap();
//! futures::executor::block_on(async {
//!     core.ready().await;
//!     let pods = core.list_resources(ResourceKind::Pods, Some("default".into()), None).await?;
//!     let mut watch = core.start_watch(ResourceKind::Pods, Some("default".into()), None).await?;
//!     while let Some(ev) = watch.recv().await {
//!         println!("{} {}", ev.action, ev.object["metadata"]["name"]);
//!         # break;
//!     }
//!     Ok::<_, kore_core::K8sError>(())
//! }).unwrap();
//! ```

mod ambient;
pub mod config;
pub mod connector;
pub mod constants;
pub mod error;
pub mod events;
pub mod json;
pub mod prefs;
pub mod secrets;
pub mod state;
pub mod tools;

pub use config::{AiEndpoints, CoreConfig};
pub use connector::{ClusterConnector, Connector, KubeconfigConnector};
pub use secrets::{SecretStore, Secrets};
pub use error::{ConnectionStatus, K8sError};
pub use events::{
    ExecExit, ExecSession, MultiPodLogLine, MultiPodLogStream, PodColor, PodLogPayload, Sink,
    SinkClosed, Subscription,
};
pub use state::ai::{
    AIChatRequest, AIConfig, AIProvider, AIStreamEvent, ChatMessage, DiagnoseRequest,
};
pub use state::resources::ResourceList;
pub use state::update::SELF_UPDATE_SUPPORTED;
pub use state::{K8sState, ResourceKind, WatchEventPayload};
pub use tools::enrich_path;

use serde::Deserialize;
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::runtime::{Handle, Runtime};
use tracing::warn;

use state::crd::CrdInfo;
use state::dashboard::{ClusterHealth, MultiClusterHealth};
use state::event_store::StoredEvent;
use state::graph::ResourceGraph;
use state::helm::{HelmRelease, HelmRevision};
use state::network_policy::{NetworkPolicyGraph, TrafficSimulationResult};
use state::rbac::{
    ForbiddenAnalysis, NaturalLanguageRbacResult, PermissionCheckResult, PermissionMatrix,
    RbacIdentity, RbacIdentityList, ReverseLookupResult, RoleSummary,
};
use state::rollback::DeploymentRevision;
use state::update::UpdateInfo;
use state::yaml::DiffLine;

/// Per-attempt timeout and attempt count for idempotent API reads (see `Core::run_read`).
const READ_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(8);
const READ_ATTEMPTS: u32 = 3;

pub type Result<T, E = K8sError> = std::result::Result<T, E>;

/// Arguments of `fetch_logs`.
#[derive(Debug, Clone, Deserialize)]
pub struct LogRequest {
    pub namespace: String,
    pub pod: String,
    pub container: Option<String>,
    pub tail_lines: Option<i64>,
    pub previous: Option<bool>,
}

struct CoreInner {
    runtime: Mutex<Option<Runtime>>,
    handle: Handle,
    state: K8sState,
}

impl Drop for CoreInner {
    fn drop(&mut self) {
        // `shutdown_background` never blocks, so dropping the last `Core`
        // is safe from any context (including inside an async task).
        if let Ok(mut rt) = self.runtime.lock() {
            if let Some(rt) = rt.take() {
                rt.shutdown_background();
            }
        }
    }
}

/// The backend facade. Cheap to clone (`Arc` inside).
#[derive(Clone)]
pub struct Core {
    inner: Arc<CoreInner>,
}

fn join_err(e: tokio::task::JoinError) -> K8sError {
    if e.is_cancelled() {
        K8sError::Cancelled
    } else {
        K8sError::Other(format!("backend task panicked: {e}"))
    }
}

fn require(cond: bool, msg: &str) -> Result<()> {
    if cond {
        Ok(())
    } else {
        Err(K8sError::Validation(msg.to_string()))
    }
}

impl Core {
    /// Build a `Core` with default configuration. Does not block: kubeconfig
    /// reading and client construction run on the owned runtime. Await
    /// [`Core::ready`] (or just call any method — they all wait for init).
    pub fn new() -> anyhow::Result<Core> {
        Self::with_config(CoreConfig::default())
    }

    pub fn with_config(config: CoreConfig) -> anyhow::Result<Core> {
        let config = Arc::new(config);
        let thread_config = config.clone();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(config.worker_threads.max(1))
            .enable_all()
            .thread_name("kore-rt")
            .on_thread_start(move || ambient::install(thread_config.clone()))
            .build()?;
        let handle = runtime.handle().clone();
        let state = K8sState::new_uninit_shared(config);

        let init_state = state.clone();
        handle.spawn(async move { init_state.init().await });

        // Hourly event-store pruning (retention).
        let prune_state = state.clone();
        handle.spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(3600));
            tick.tick().await;
            loop {
                tick.tick().await;
                let days = prune_state.event_retention_days();
                if let Err(e) = prune_state.set_event_retention(days).await {
                    warn!(error = %e, "Periodic event prune failed");
                }
            }
        });

        Ok(Core {
            inner: Arc::new(CoreInner {
                runtime: Mutex::new(Some(runtime)),
                handle,
                state,
            }),
        })
    }

    /// Resolves when initial kubeconfig/client setup has finished (success
    /// or failure — check [`Core::get_connection_status`]).
    pub async fn ready(&self) {
        let state = self.inner.state.clone();
        let _ = self
            .inner
            .handle
            .spawn(async move { state.wait_ready().await })
            .await;
    }

    pub fn is_ready(&self) -> bool {
        self.inner.state.is_ready()
    }

    /// Spawn arbitrary work on the core runtime. The `JoinHandle` can be
    /// awaited from any executor.
    pub fn spawn<F>(&self, f: F) -> tokio::task::JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.inner.handle.spawn(f)
    }

    /// The runtime handle (for advanced callers).
    pub fn handle(&self) -> &Handle {
        &self.inner.handle
    }

    /// The underlying state (its async methods must be polled on the core
    /// runtime, e.g. via [`Core::spawn`]).
    pub fn state(&self) -> &K8sState {
        &self.inner.state
    }

    /// Run `f(state)` on the core runtime after init completed.
    async fn run<T, F, Fut>(&self, f: F) -> Result<T>
    where
        F: FnOnce(K8sState) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T>> + Send + 'static,
        T: Send + 'static,
    {
        let state = self.inner.state.clone();
        self.inner
            .handle
            .spawn(async move {
                state.wait_ready().await;
                f(state).await
            })
            .await
            .map_err(join_err)?
    }

    /// Like [`Core::run`] for idempotent API reads: each attempt gets
    /// [`READ_ATTEMPT_TIMEOUT`] and a timed-out attempt is retried (up to
    /// [`READ_ATTEMPTS`]). A request sent on a stale pooled connection can
    /// otherwise hang until TCP gives up, leaving the UI on a skeleton.
    async fn run_read<A, T, F, Fut>(&self, args: A, f: F) -> Result<T>
    where
        A: Clone + Send + 'static,
        F: Fn(K8sState, A) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T>> + Send + 'static,
        T: Send + 'static,
    {
        let state = self.inner.state.clone();
        self.inner
            .handle
            .spawn(async move {
                state.wait_ready().await;
                for attempt in 1..=READ_ATTEMPTS {
                    match tokio::time::timeout(READ_ATTEMPT_TIMEOUT, f(state.clone(), args.clone()))
                        .await
                    {
                        Ok(res) => return res,
                        Err(_) => tracing::warn!(attempt, "API read timed out; retrying"),
                    }
                }
                Err(K8sError::other(format!(
                    "Request timed out after {READ_ATTEMPTS} attempts"
                )))
            })
            .await
            .map_err(join_err)?
    }

    /// Run a blocking closure on the core runtime's blocking pool.
    async fn blocking<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce() -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        self.inner
            .handle
            .spawn_blocking(f)
            .await
            .map_err(join_err)?
    }

    /// Cancel all watches, log streams, exec sessions, AI streams and port
    /// forwards. Call from the app's quit hook.
    pub async fn shutdown(&self) {
        let _ = self
            .run(|s| async move {
                s.shutdown().await;
                Ok(())
            })
            .await;
    }

    // ── Connection / context ────────────────────────────────────────────

    pub async fn get_connection_status(&self) -> Result<ConnectionStatus> {
        self.run(|s| async move { Ok(s.get_connection_status().await) })
            .await
    }

    pub async fn retry_connection(&self, context: Option<String>) -> Result<ConnectionStatus> {
        self.run(|s| async move { Ok(s.retry_connection(context).await) })
            .await
    }

    pub async fn list_contexts(&self) -> Result<Vec<String>> {
        self.run(|s| async move { s.list_contexts().await }).await
    }

    pub async fn list_namespaces(&self) -> Result<Vec<String>> {
        self.run(|s| async move { s.list_namespaces().await }).await
    }

    /// Switch context. `Ok` even when the cluster is unreachable (A15): the
    /// returned status says whether it is connected and carries the error
    /// (identical to a following `get_connection_status`). `Err` only when
    /// the kubeconfig is unreadable or the context has no usable client.
    pub async fn switch_context(&self, name: String) -> Result<ConnectionStatus> {
        require(!name.is_empty(), "Context name cannot be empty")?;
        self.run(|s| async move { s.switch_context(name).await })
            .await
    }

    // ── Resources / watch / logs / exec / port-forward ──────────────────

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

    /// List every object of `kind` (paginated with `continue` tokens, A4).
    /// Each page has its own timeout and retries; `truncated` is set when the
    /// [`constants::MAX_LIST_TOTAL`] safety cap was hit.
    pub async fn list_resources_full(
        &self,
        kind: ResourceKind,
        namespace: Option<String>,
        label_selector: Option<String>,
    ) -> Result<ResourceList> {
        self.run(
            move |s| async move { s.list_resources_full(kind, namespace, label_selector).await },
        )
        .await
    }

    /// Start a resource watch. Each call is independent; drop the
    /// subscription to stop it. Context switch cancels it.
    pub async fn start_watch(
        &self,
        kind: ResourceKind,
        namespace: Option<String>,
        label_selector: Option<String>,
    ) -> Result<Subscription<WatchEventPayload>> {
        self.run(move |s| async move { s.start_watch(kind, namespace, label_selector).await })
            .await
    }

    pub async fn fetch_logs(&self, req: LogRequest) -> Result<String> {
        require(
            !req.namespace.is_empty() && !req.pod.is_empty(),
            "Namespace and pod name are required",
        )?;
        self.run(move |s| async move {
            s.fetch_logs(
                req.namespace,
                req.pod,
                req.container,
                req.tail_lines,
                req.previous.unwrap_or(false),
            )
            .await
        })
        .await
    }

    pub async fn delete_resource(
        &self,
        kind: ResourceKind,
        namespace: String,
        name: String,
    ) -> Result<()> {
        // A6: cluster-scoped kinds (nodes, namespaces) take no namespace.
        if kind.is_cluster_scoped() {
            require(!name.is_empty(), "Resource name is required")?;
        } else {
            require(
                !namespace.is_empty() && !name.is_empty(),
                "Namespace and resource name are required",
            )?;
        }
        self.run(move |s| async move { s.delete_resource(kind, namespace, name).await })
            .await
    }

    pub async fn describe_pod(
        &self,
        namespace: String,
        pod_name: String,
    ) -> Result<serde_json::Value> {
        require(
            !namespace.is_empty() && !pod_name.is_empty(),
            "Namespace and pod name are required",
        )?;
        self.run_read((namespace, pod_name), |s, (n, p)| async move {
            s.get_pod(n, p).await
        })
        .await
    }

    pub async fn describe_resource(
        &self,
        kind: ResourceKind,
        namespace: String,
        name: String,
    ) -> Result<serde_json::Value> {
        require(!name.is_empty(), "Resource name is required")?;
        self.run_read((namespace, name), move |s, (n, m)| async move {
            s.describe_resource(kind, n, m).await
        })
        .await
    }

    pub async fn list_events_for_resource(
        &self,
        kind: String,
        namespace: String,
        name: String,
    ) -> Result<Vec<serde_json::Value>> {
        require(
            !namespace.is_empty() && !name.is_empty(),
            "Namespace and resource name are required",
        )?;
        self.run_read((kind, namespace, name), |s, (k, n, m)| async move {
            s.list_events_for_resource(k, n, m).await
        })
        .await
    }

    /// Stream pod logs (payload identical to the `pod-logs://` event).
    pub async fn start_pod_logs_stream(
        &self,
        namespace: String,
        pod_name: String,
        container: Option<String>,
        previous: Option<bool>,
    ) -> Result<Subscription<PodLogPayload>> {
        require(
            !namespace.is_empty() && !pod_name.is_empty(),
            "Namespace and pod name are required",
        )?;
        self.run(move |s| async move {
            s.stream_pod_logs(namespace, pod_name, container, previous.unwrap_or(false))
                .await
        })
        .await
    }

    /// Cancel every active log stream (single- and multi-pod).
    pub async fn stop_pod_logs_stream(&self) -> Result<()> {
        self.run(|s| async move { s.stop_pod_logs_stream().await })
            .await
    }

    pub async fn get_pod_metrics(
        &self,
        namespace: String,
        pod_name: String,
    ) -> Result<serde_json::Value> {
        require(
            !namespace.is_empty() && !pod_name.is_empty(),
            "Namespace and pod name are required",
        )?;
        self.run_read((namespace, pod_name), |s, (n, p)| async move {
            s.get_pod_metrics(n, p).await
        })
        .await
    }

    pub async fn start_port_forward(
        &self,
        namespace: String,
        pod_name: String,
        local_port: u16,
        pod_port: u16,
    ) -> Result<serde_json::Value> {
        require(
            !namespace.is_empty() && !pod_name.is_empty(),
            "Namespace and pod name are required",
        )?;
        self.run(move |s| async move {
            s.start_port_forward(namespace, pod_name, local_port, pod_port)
                .await
        })
        .await
    }

    pub async fn stop_port_forward(
        &self,
        namespace: String,
        pod_name: String,
        local_port: u16,
        pod_port: u16,
    ) -> Result<()> {
        self.run(move |s| async move {
            s.stop_port_forward(namespace, pod_name, local_port, pod_port)
                .await
        })
        .await
    }

    pub async fn scale_deployment(
        &self,
        namespace: String,
        name: String,
        replicas: i32,
    ) -> Result<()> {
        require(
            !namespace.is_empty() && !name.is_empty(),
            "Namespace and deployment name are required",
        )?;
        require(replicas >= 0, "Replicas must be non-negative")?;
        self.run(move |s| async move { s.scale_deployment(namespace, name, replicas).await })
            .await
    }

    pub async fn restart_deployment(&self, namespace: String, name: String) -> Result<()> {
        require(
            !namespace.is_empty() && !name.is_empty(),
            "Namespace and deployment name are required",
        )?;
        self.run(move |s| async move { s.restart_deployment(namespace, name).await })
            .await
    }

    pub async fn search_resources(
        &self,
        query: String,
        namespace: Option<String>,
    ) -> Result<Vec<serde_json::Value>> {
        require(!query.is_empty(), "Search query cannot be empty")?;
        self.run(move |s| async move { s.search_resources(query, namespace).await })
            .await
    }

    /// Start an exec session (`shell` defaults to `/bin/sh`).
    pub async fn exec_into_pod(
        &self,
        namespace: String,
        pod_name: String,
        container: Option<String>,
        shell: Option<String>,
    ) -> Result<ExecSession> {
        require(
            !namespace.is_empty() && !pod_name.is_empty(),
            "Namespace and pod name are required",
        )?;
        self.run(
            move |s| async move { s.exec_into_pod(namespace, pod_name, container, shell).await },
        )
        .await
    }

    pub async fn send_exec_input(&self, session_id: String, data: String) -> Result<()> {
        self.run(move |s| async move { s.send_exec_input(session_id, data).await })
            .await
    }

    /// Raw-bytes stdin (terminal input is not necessarily UTF-8).
    pub async fn send_exec_bytes(&self, session_id: String, data: Vec<u8>) -> Result<()> {
        self.run(move |s| async move { s.send_exec_bytes(session_id, data).await })
            .await
    }

    pub async fn resize_exec(&self, session_id: String, cols: u16, rows: u16) -> Result<()> {
        self.run(move |s| async move { s.resize_exec(session_id, cols, rows).await })
            .await
    }

    pub async fn stop_exec(&self, session_id: String) -> Result<()> {
        self.run(move |s| async move { s.stop_exec(session_id).await })
            .await
    }

    // ── YAML / rollback ──────────────────────────────────────────────────

    pub async fn get_resource_yaml(
        &self,
        kind: ResourceKind,
        namespace: String,
        name: String,
    ) -> Result<String> {
        self.run_read((namespace, name), move |s, (n, m)| async move {
            s.get_resource_yaml(kind, n, m).await
        })
        .await
    }

    pub async fn apply_resource_yaml(
        &self,
        kind: ResourceKind,
        namespace: String,
        name: String,
        yaml: String,
    ) -> Result<String> {
        self.run(move |s| async move { s.apply_resource_yaml(kind, namespace, name, yaml).await })
            .await
    }

    pub async fn diff_resource_yaml(
        &self,
        kind: ResourceKind,
        namespace: String,
        name: String,
        new_yaml: String,
    ) -> Result<Vec<DiffLine>> {
        self.run(
            move |s| async move { s.diff_resource_yaml(kind, namespace, name, new_yaml).await },
        )
        .await
    }

    pub async fn list_deployment_revisions(
        &self,
        namespace: String,
        name: String,
    ) -> Result<Vec<DeploymentRevision>> {
        self.run_read((namespace, name), |s, (n, m)| async move {
            s.list_deployment_revisions(n, m).await
        })
        .await
    }

    pub async fn get_revision_yaml(&self, namespace: String, rs_name: String) -> Result<String> {
        self.run_read((namespace, rs_name), |s, (n, r)| async move {
            s.get_revision_yaml(n, r).await
        })
        .await
    }

    pub async fn rollback_deployment(
        &self,
        namespace: String,
        deployment_name: String,
        rs_name: String,
    ) -> Result<()> {
        self.run(move |s| async move {
            s.rollback_deployment(namespace, deployment_name, rs_name)
                .await
        })
        .await
    }

    // ── Dashboard / events / multi-pod logs ─────────────────────────────

    pub async fn get_cluster_health(&self) -> Result<ClusterHealth> {
        self.run(|s| async move { s.get_cluster_health().await })
            .await
    }

    pub async fn get_cluster_health_multi_cluster(&self) -> Result<MultiClusterHealth> {
        self.run(|s| async move { s.get_cluster_health_multi_cluster().await })
            .await
    }

    /// Events from the SQLite history store for the current context
    /// (`since`/`until` are RFC3339).
    pub async fn query_stored_events(
        &self,
        since: String,
        until: String,
        namespace: Option<String>,
    ) -> Result<Vec<StoredEvent>> {
        self.run(move |s| async move {
            let store = s
                .event_store()
                .cloned()
                .ok_or_else(|| K8sError::other("Event store not available"))?;
            let ctx = s.current_context_name().await;
            store
                .query_events(&since, &until, namespace.as_deref(), ctx.as_deref())
                .await
        })
        .await
    }

    /// Set event retention (days) and prune now (the Settings "Event
    /// Retention Period"). Returns the number of pruned rows.
    pub async fn set_event_retention(&self, days: u32) -> Result<u64> {
        self.run(move |s| async move { s.set_event_retention(days).await })
            .await
    }

    pub async fn stream_multi_pod_logs(
        &self,
        namespace: String,
        label_selector: String,
        tail_lines: Option<i64>,
    ) -> Result<MultiPodLogStream> {
        self.run(move |s| async move {
            s.stream_multi_pod_logs(namespace, label_selector, tail_lines)
                .await
        })
        .await
    }

    // ── CRDs / graph ─────────────────────────────────────────────────────

    pub async fn list_crds(&self) -> Result<Vec<CrdInfo>> {
        self.run(|s| async move { s.list_crds().await }).await
    }

    pub async fn list_crd_instances(
        &self,
        group: String,
        version: String,
        plural: String,
        namespace: Option<String>,
    ) -> Result<Vec<serde_json::Value>> {
        self.run(move |s| async move {
            s.list_crd_instances(group, version, plural, namespace)
                .await
        })
        .await
    }

    pub async fn get_crd_instance(
        &self,
        group: String,
        version: String,
        plural: String,
        namespace: String,
        name: String,
    ) -> Result<serde_json::Value> {
        self.run(move |s| async move {
            s.get_crd_instance(group, version, plural, namespace, name)
                .await
        })
        .await
    }

    pub async fn delete_crd_instance(
        &self,
        group: String,
        version: String,
        plural: String,
        namespace: String,
        name: String,
    ) -> Result<()> {
        self.run(move |s| async move {
            s.delete_crd_instance(group, version, plural, namespace, name)
                .await
        })
        .await
    }

    pub async fn build_resource_graph(&self, namespace: Option<String>) -> Result<ResourceGraph> {
        self.run(move |s| async move { s.build_resource_graph(namespace).await })
            .await
    }

    // ── Helm / multi-cluster ────────────────────────────────────────────

    pub async fn helm_available(&self) -> Result<bool> {
        self.run(|s| async move { Ok(s.helm_available().await) })
            .await
    }

    pub async fn list_helm_releases(&self, namespace: Option<String>) -> Result<Vec<HelmRelease>> {
        self.run(move |s| async move { s.list_helm_releases(namespace).await })
            .await
    }

    pub async fn get_helm_values(&self, release: String, namespace: String) -> Result<String> {
        self.run(move |s| async move { s.get_helm_values(release, namespace).await })
            .await
    }

    /// `helm get notes` (NOTES.txt), with the `NOTES:` header stripped.
    pub async fn get_helm_notes(&self, release: String, namespace: String) -> Result<String> {
        self.run(move |s| async move { s.get_helm_notes(release, namespace).await })
            .await
    }

    pub async fn get_helm_manifest(&self, release: String, namespace: String) -> Result<String> {
        self.run(move |s| async move { s.get_helm_manifest(release, namespace).await })
            .await
    }

    pub async fn get_helm_history(
        &self,
        release: String,
        namespace: String,
    ) -> Result<Vec<HelmRevision>> {
        self.run(move |s| async move { s.get_helm_history(release, namespace).await })
            .await
    }

    pub async fn rollback_helm_release(
        &self,
        release: String,
        namespace: String,
        revision: String,
    ) -> Result<String> {
        self.run(
            move |s| async move { s.rollback_helm_release(release, namespace, revision).await },
        )
        .await
    }

    pub async fn list_resources_multi_cluster(
        &self,
        kind: ResourceKind,
        namespace: Option<String>,
        label_selector: Option<String>,
    ) -> Result<Vec<serde_json::Value>> {
        self.run(move |s| async move {
            s.list_resources_multi_cluster(kind, namespace, label_selector)
                .await
        })
        .await
    }

    // ── AI ──────────────────────────────────────────────────────────────

    /// Diagnose a resource; the response streams on the subscription
    /// (payload identical to `ai-response://`). Dropping it cancels the
    /// request (and kills a `claude`/`agent` child).
    pub async fn ai_diagnose(
        &self,
        config: AIConfig,
        request: DiagnoseRequest,
    ) -> Result<Subscription<AIStreamEvent>> {
        self.run(move |s| async move { s.ai_diagnose(config, request).await })
            .await
    }

    /// Cluster chat with tool calling (payload identical to `ai-chat://`).
    pub async fn ai_chat(
        &self,
        config: AIConfig,
        request: AIChatRequest,
    ) -> Result<Subscription<AIStreamEvent>> {
        self.run(move |s| async move { s.ai_chat(config, request).await })
            .await
    }

    /// Deliberate Fix 12: Test Connection that reports HTTP errors with the
    /// reason (`Err("HTTP 503 Service Unavailable: ...")`).
    pub async fn ai_test_connection_checked(&self, config: AIConfig) -> Result<()> {
        self.run(move |_| async move { K8sState::ai_test_connection_checked(&config).await })
            .await
    }

    pub async fn ai_test_connection(&self, config: AIConfig) -> Result<bool> {
        self.run(move |_| async move { K8sState::ai_test_connection(&config).await })
            .await
    }

    pub async fn list_ollama_models(&self, base_url: Option<String>) -> Result<Vec<String>> {
        self.run(move |_| async move { K8sState::list_ollama_models(base_url.as_deref()).await })
            .await
    }

    pub async fn claude_cli_available(&self) -> Result<bool> {
        self.run(|_| async move { K8sState::claude_cli_available().await })
            .await
    }

    pub async fn list_claude_models(&self) -> Result<Vec<String>> {
        self.run(|_| async move { K8sState::list_claude_models().await })
            .await
    }

    pub async fn cursor_agent_available(&self) -> Result<bool> {
        self.run(|_| async move { K8sState::cursor_agent_cli_available().await })
            .await
    }

    pub async fn list_cursor_agent_models(&self) -> Result<Vec<String>> {
        self.run(|_| async move { K8sState::list_cursor_agent_models().await })
            .await
    }

    /// `{claude_cli, cursor_agent, ollama}` availability. Deliberate Fix 13:
    /// the Ollama probe uses `ollama_base_url` (the configured base URL)
    /// instead of always `http://localhost:11434`.
    pub async fn check_providers_availability(
        &self,
        ollama_base_url: Option<String>,
    ) -> Result<HashMap<String, bool>> {
        self.run(move |_| async move {
            let (claude, cursor, ollama) = tokio::join!(
                K8sState::claude_cli_available(),
                K8sState::cursor_agent_cli_available(),
                K8sState::ollama_available(ollama_base_url.as_deref()),
            );
            let mut map = HashMap::new();
            map.insert("claude_cli".to_string(), claude.unwrap_or(false));
            map.insert("cursor_agent".to_string(), cursor.unwrap_or(false));
            map.insert("ollama".to_string(), ollama.unwrap_or(false));
            Ok(map)
        })
        .await
    }

    /// Default Ollama base URL: `CoreConfig::ai_endpoints.ollama_default`, else `http://localhost:11434`.
    pub fn ollama_default_url(&self) -> String {
        self.inner.state.config.ai_endpoints.ollama_default.clone().unwrap_or_else(|| "http://localhost:11434".into())
    }

    // ── Secure key storage (OS keychain; blocking pool) ─────────────────

    pub async fn store_api_key(&self, provider: String, key: String) -> Result<()> {
        let store = self.inner.state.config.secrets.clone();
        self.blocking(move || store.0.store(&provider, &key))
            .await
    }

    pub async fn get_api_key(&self, provider: String) -> Result<Option<String>> {
        let store = self.inner.state.config.secrets.clone();
        self.blocking(move || store.0.get(&provider)).await
    }

    pub async fn delete_api_key(&self, provider: String) -> Result<()> {
        let store = self.inner.state.config.secrets.clone();
        self.blocking(move || store.0.delete(&provider))
            .await
    }

    // ── Debug containers ────────────────────────────────────────────────

    pub async fn add_debug_container(
        &self,
        namespace: String,
        pod_name: String,
        image: String,
        target_container: Option<String>,
        command: Option<Vec<String>>,
    ) -> Result<String> {
        self.run(move |s| async move {
            s.add_debug_container(namespace, pod_name, image, target_container, command)
                .await
        })
        .await
    }

    pub async fn list_debug_containers(
        &self,
        namespace: String,
        pod_name: String,
    ) -> Result<Vec<serde_json::Value>> {
        self.run_read((namespace, pod_name), |s, (n, p)| async move {
            s.list_debug_containers(n, p).await
        })
        .await
    }

    pub async fn stop_debug_container(
        &self,
        namespace: String,
        pod_name: String,
        container_name: String,
    ) -> Result<()> {
        self.run(move |s| async move {
            s.stop_debug_container(namespace, pod_name, container_name)
                .await
        })
        .await
    }

    // ── Network policies ────────────────────────────────────────────────

    pub async fn build_network_policy_graph(
        &self,
        namespace: Option<String>,
    ) -> Result<NetworkPolicyGraph> {
        self.run(move |s| async move { s.build_network_policy_graph(namespace).await })
            .await
    }

    pub async fn simulate_network_traffic(
        &self,
        source_namespace: String,
        source_pod: String,
        dest_namespace: String,
        dest_pod: String,
        port: Option<i32>,
        protocol: Option<String>,
    ) -> Result<TrafficSimulationResult> {
        self.run(move |s| async move {
            s.simulate_network_traffic(
                source_namespace,
                source_pod,
                dest_namespace,
                dest_pod,
                port,
                protocol,
            )
            .await
        })
        .await
    }

    // ── RBAC ────────────────────────────────────────────────────────────

    pub async fn rbac_check_permission(
        &self,
        identity: RbacIdentity,
        verb: String,
        resource: String,
        namespace: Option<String>,
    ) -> Result<PermissionCheckResult> {
        self.run(move |s| async move {
            let api_group = state::rbac::api_group_for_resource_cmd(&resource);
            s.rbac_check_permission(
                &identity,
                &verb,
                &resource,
                &api_group,
                namespace.as_deref(),
            )
            .await
        })
        .await
    }

    pub async fn rbac_build_matrix(
        &self,
        identity: RbacIdentity,
        namespace: Option<String>,
    ) -> Result<PermissionMatrix> {
        self.run(move |s| async move { s.rbac_build_matrix(&identity, namespace.as_deref()).await })
            .await
    }

    pub async fn rbac_reverse_lookup(
        &self,
        role_kind: String,
        role_name: String,
        role_namespace: Option<String>,
    ) -> Result<ReverseLookupResult> {
        self.run(move |s| async move {
            s.rbac_reverse_lookup(&role_kind, &role_name, role_namespace.as_deref())
                .await
        })
        .await
    }

    pub async fn rbac_analyze_forbidden(&self, error_message: String) -> Result<ForbiddenAnalysis> {
        self.run(move |s| async move { s.rbac_analyze_forbidden(&error_message).await })
            .await
    }

    pub async fn rbac_list_identities(&self) -> Result<RbacIdentityList> {
        self.run(|s| async move { s.rbac_list_identities().await })
            .await
    }

    pub async fn rbac_list_roles(&self, namespace: Option<String>) -> Result<Vec<RoleSummary>> {
        self.run(move |s| async move { s.rbac_list_roles(namespace.as_deref()).await })
            .await
    }

    pub async fn rbac_natural_language_query(
        &self,
        query: String,
        namespace: Option<String>,
    ) -> Result<NaturalLanguageRbacResult> {
        self.run(move |s| async move {
            s.rbac_natural_language_query(&query, namespace.as_deref())
                .await
        })
        .await
    }

    pub async fn rbac_who_can(
        &self,
        verb: String,
        resource: String,
        namespace: Option<String>,
    ) -> Result<Vec<PermissionCheckResult>> {
        self.run(
            move |s| async move { s.rbac_who_can(&verb, &resource, namespace.as_deref()).await },
        )
        .await
    }

    // ── Update checker ──────────────────────────────────────────────────

    pub async fn check_for_updates(&self) -> Result<UpdateInfo> {
        let url = self.inner.state.config.update_url.clone();
        self.run(|_| async move {
            state::update::check_for_updates(&url)
                .await
                .map_err(K8sError::Other)
        })
        .await
    }

    pub async fn perform_update(&self) -> Result<String> {
        let url = self.inner.state.config.update_url.clone();
        self.run(|_| async move {
            state::update::perform_update(&url)
                .await
                .map_err(K8sError::Other)
        })
        .await
    }

    // ── Favorites ───────────────────────────────────────────────────────

    pub async fn load_favorites(&self, key: String) -> Result<Vec<String>> {
        let dir = self.inner.state.config.favorites_dir();
        self.blocking(move || prefs::load_favorites(dir.as_deref(), &key))
            .await
    }

    pub async fn save_favorites(&self, key: String, values: Vec<String>) -> Result<()> {
        let dir = self.inner.state.config.favorites_dir();
        self.blocking(move || prefs::save_favorites(dir.as_deref(), key, values))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offline_core() -> (Core, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let cfg = CoreConfig {
            kubeconfig_paths: vec![dir.path().join("missing-kubeconfig")],
            data_dir: Some(dir.path().join("data")),
            favorites_dir: Some(dir.path().join("kore")),
            ..Default::default()
        };
        (Core::with_config(cfg).unwrap(), dir)
    }

    #[test]
    fn core_works_without_tokio_context() {
        let (core, _dir) = offline_core();
        futures::executor::block_on(async {
            core.ready().await;
            let st = core.get_connection_status().await.unwrap();
            assert!(!st.connected);
            assert_eq!(st.error_kind.as_deref(), Some("no_kubeconfig"));
            assert!(core.list_contexts().await.unwrap().is_empty());
            let err = core
                .list_resources(ResourceKind::Pods, None, None)
                .await
                .unwrap_err();
            assert!(matches!(err, K8sError::ClientMissing));
            // Validation still happens before hitting the backend
            let err = core.switch_context(String::new()).await.unwrap_err();
            assert_eq!(
                err.to_string(),
                "Validation error: Context name cannot be empty"
            );
            // Favorites go to the configured dir
            core.save_favorites("k".into(), vec!["v".into()])
                .await
                .unwrap();
            assert_eq!(core.load_favorites("k".into()).await.unwrap(), vec!["v"]);
            // Event store opened in the configured data dir
            let evs = core
                .query_stored_events(
                    "2000-01-01T00:00:00Z".into(),
                    "2100-01-01T00:00:00Z".into(),
                    None,
                )
                .await
                .unwrap();
            assert!(evs.is_empty());
        });
        assert!(_dir.path().join("data").join("events.db").exists());
    }

    /// A6: cluster-scoped kinds need no namespace; namespaced kinds still do.
    #[test]
    fn delete_cluster_scoped_needs_no_namespace() {
        let (core, _dir) = offline_core();
        futures::executor::block_on(async {
            for kind in [ResourceKind::Nodes, ResourceKind::Namespaces] {
                // Passes validation and reaches the backend (no client here).
                let err = core
                    .delete_resource(kind, String::new(), "x".into())
                    .await
                    .unwrap_err();
                assert!(matches!(err, K8sError::ClientMissing), "{kind:?}: {err}");
                let err = core
                    .delete_resource(kind, String::new(), String::new())
                    .await
                    .unwrap_err();
                assert!(matches!(err, K8sError::Validation(_)), "{kind:?}: {err}");
            }
            for kind in ResourceKind::ALL.iter().filter(|k| !k.is_cluster_scoped()) {
                let err = core
                    .delete_resource(*kind, String::new(), "x".into())
                    .await
                    .unwrap_err();
                assert_eq!(
                    err.to_string(),
                    "Validation error: Namespace and resource name are required",
                    "{kind:?}"
                );
            }
        });
    }

    /// A15: switching to an unreachable context succeeds (the UI follows the
    /// switch) and the returned status reports the connection error.
    #[test]
    fn switch_context_returns_connection_status() {
        let dir = tempfile::tempdir().unwrap();
        let kc = dir.path().join("kubeconfig");
        // 127.0.0.1:9 (discard) refuses connections immediately.
        std::fs::write(
            &kc,
            r#"apiVersion: v1
kind: Config
clusters:
- name: dead
  cluster: {server: "https://127.0.0.1:9", insecure-skip-tls-verify: true}
users:
- name: u
  user: {token: t}
contexts:
- name: dead
  context: {cluster: dead, user: u}
- name: dead-2
  context: {cluster: dead, user: u}
current-context: dead
"#,
        )
        .unwrap();
        let core = Core::with_config(CoreConfig {
            kubeconfig_paths: vec![kc],
            data_dir: Some(dir.path().join("data")),
            favorites_dir: Some(dir.path().join("kore")),
            ..Default::default()
        })
        .unwrap();
        futures::executor::block_on(async {
            core.ready().await;
            let w = core
                .start_watch(ResourceKind::Pods, None, None)
                .await
                .unwrap();
            // A failed switch (unknown context) leaves the current streams alone.
            assert!(core.switch_context("nope".into()).await.is_err());
            assert!(!w.is_cancelled());
            let st = core.switch_context("dead-2".into()).await.unwrap();
            assert!(w.is_cancelled(), "a successful switch cancels watches");
            assert!(!st.connected);
            assert!(st.error.is_some());
            assert!(st.error_kind.is_some());
            assert_eq!(st.current_context.as_deref(), Some("dead-2"));
            assert_eq!(st.contexts_available, vec!["dead", "dead-2"]);
            // Same as what get_connection_status reports afterwards.
            let again = core.get_connection_status().await.unwrap();
            assert_eq!(again.current_context, st.current_context);
            assert_eq!(again.error_kind, st.error_kind);
        });
    }

    #[test]
    fn dropping_core_inside_async_context_does_not_panic() {
        let (core, _dir) = offline_core();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            core.ready().await;
            drop(core);
        });
    }
}
