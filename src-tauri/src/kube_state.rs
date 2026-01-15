use crate::error::{K8sError, Result};
use futures::{StreamExt, TryStreamExt};
use kube::{
    api::{Api, DeleteParams, DynamicObject, ListParams, LogParams, Resource, ResourceExt},
    config::{KubeConfigOptions, Kubeconfig},
    core::{ApiResource, GroupVersionKind, NamespaceResourceScope},
    runtime::watcher,
    Client, Config,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use std::collections::HashMap;
use std::io::ErrorKind;
use tauri::{AppHandle, Emitter};
use tokio::sync::{RwLock, oneshot};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[derive(Debug, Clone, Serialize)]
pub struct WatchEventPayload {
    pub action: String,
    pub kind: String,
    pub object: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceKind {
    Pods,
    Deployments,
    Services,
    Nodes,
}

struct PortForwardHandle {
    cancel: oneshot::Sender<()>,
}

struct StateInner {
    client: Option<Client>,
    kubeconfig: Option<Kubeconfig>,
    current_context: Option<String>,
    watch_cancel: Option<oneshot::Sender<()>>,
    port_forwards: HashMap<String, PortForwardHandle>,
}

#[derive(Clone)]
pub struct K8sState {
    inner: Arc<RwLock<StateInner>>,
}

impl K8sState {
    pub async fn new() -> Result<Self> {
        let kubeconfig = Kubeconfig::read()?;
        let ctx_name = kubeconfig
            .current_context
            .clone()
            .or_else(|| kubeconfig.contexts.first().map(|c| c.name.clone()));
        let client = Self::client_for_context(&kubeconfig, ctx_name.clone()).await?;

        Ok(Self {
            inner: Arc::new(RwLock::new(StateInner {
                client: Some(client),
                kubeconfig: Some(kubeconfig),
                current_context: ctx_name,
                watch_cancel: None,
                port_forwards: HashMap::new(),
            })),
        })
    }

    async fn client_for_context(kubeconfig: &Kubeconfig, context: Option<String>) -> Result<Client> {
        let opts = KubeConfigOptions {
            context,
            ..Default::default()
        };
        let config = Config::from_custom_kubeconfig(kubeconfig.clone(), &opts)
            .await?;
        Client::try_from(config).map_err(K8sError::Kube)
    }

    pub async fn reload_kubeconfig(&self) -> Result<Kubeconfig> {
        let cfg = Kubeconfig::read()?;
        Ok(cfg)
    }

    pub async fn list_contexts(&self) -> Result<Vec<String>> {
        let inner = self.inner.read().await;
        let kubeconfig = inner
            .kubeconfig
            .clone()
            .ok_or(K8sError::ClientMissing)?;
        Ok(kubeconfig.contexts.iter().map(|c| c.name.clone()).collect())
    }

    pub async fn list_namespaces(&self) -> Result<Vec<String>> {
        let client = self.current_client().await?;
        let api: Api<k8s_openapi::api::core::v1::Namespace> = Api::all(client);
        let list = api.list(&ListParams::default()).await.map_err(K8sError::Kube)?;
        Ok(list.items.iter().map(|ns| ns.name_any()).collect())
    }

    pub async fn switch_context(&self, name: String) -> Result<String> {
        let kubeconfig = self.reload_kubeconfig().await?;
        let client = Self::client_for_context(&kubeconfig, Some(name.clone())).await?;
        let mut inner = self.inner.write().await;
        inner.client = Some(client);
        inner.kubeconfig = Some(kubeconfig);
        inner.current_context = Some(name.clone());
        Ok(name)
    }

    pub async fn current_client(&self) -> Result<Client> {
        let inner = self.inner.read().await;
        inner
            .client
            .clone()
            .ok_or(K8sError::ClientMissing)
    }

    pub async fn list_resources(&self, kind: ResourceKind, namespace: Option<String>) -> Result<Vec<serde_json::Value>> {
        match kind {
            ResourceKind::Pods => {
                self.list_namespaced_direct::<k8s_openapi::api::core::v1::Pod>(namespace).await
            }
            ResourceKind::Deployments => {
                self.list_namespaced_direct::<k8s_openapi::api::apps::v1::Deployment>(namespace).await
            }
            ResourceKind::Services => {
                self.list_namespaced_direct::<k8s_openapi::api::core::v1::Service>(namespace).await
            }
            ResourceKind::Nodes => {
                self.list_cluster_scoped_direct::<k8s_openapi::api::core::v1::Node>().await
            }
        }
    }

    pub async fn start_watch(&self, app: AppHandle, kind: ResourceKind, namespace: Option<String>) -> Result<()> {
        // Stop any existing watch first
        {
            let mut inner = self.inner.write().await;
            if let Some(cancel_tx) = inner.watch_cancel.take() {
                let _ = cancel_tx.send(());
            }
        }

        // Create a new cancellation channel for this watch
        let (cancel_tx, cancel_rx) = oneshot::channel();
        
        // Store the cancel sender
        {
            let mut inner = self.inner.write().await;
            inner.watch_cancel = Some(cancel_tx);
        }

        match kind {
            ResourceKind::Pods => {
                self.watch_namespaced::<k8s_openapi::api::core::v1::Pod>(app, "pods", namespace, cancel_rx).await?
            }
            ResourceKind::Deployments => {
                self.watch_namespaced::<k8s_openapi::api::apps::v1::Deployment>(app, "deployments", namespace, cancel_rx).await?
            }
            ResourceKind::Services => {
                self.watch_namespaced::<k8s_openapi::api::core::v1::Service>(app, "services", namespace, cancel_rx).await?
            }
            ResourceKind::Nodes => {
                self.watch_cluster_scoped::<k8s_openapi::api::core::v1::Node>(app, "nodes", cancel_rx).await?
            }
        }
        Ok(())
    }

    async fn list_namespaced_direct<K>(&self, ns: Option<String>) -> Result<Vec<serde_json::Value>>
    where
        K: Clone + serde::de::DeserializeOwned + serde::Serialize + Resource<Scope = NamespaceResourceScope> + Send + Sync + std::fmt::Debug + 'static,
        <K as Resource>::DynamicType: Default + Eq + std::hash::Hash,
    {
        let client = self.current_client().await?;
        let api: Api<K> = match ns {
            Some(namespace) => Api::namespaced(client, &namespace),
            None => Api::all(client), // List from all namespaces
        };

        let list = api.list(&ListParams::default()).await.map_err(K8sError::Kube)?;
        let items: Vec<serde_json::Value> = list.items
            .iter()
            .map(|obj| serde_json::to_value(obj).unwrap_or_else(|_| json!({ "name": obj.name_any() })))
            .collect();

        Ok(items)
    }

    async fn list_cluster_scoped_direct<K>(&self) -> Result<Vec<serde_json::Value>>
    where
        K: Clone + serde::de::DeserializeOwned + serde::Serialize + Resource + Send + Sync + std::fmt::Debug + 'static,
        <K as Resource>::DynamicType: Default + Eq + std::hash::Hash,
    {
        let client = self.current_client().await?;
        let api: Api<K> = Api::all(client);

        let list = api.list(&ListParams::default()).await.map_err(K8sError::Kube)?;
        let items: Vec<serde_json::Value> = list.items
            .iter()
            .map(|obj| serde_json::to_value(obj).unwrap_or_else(|_| json!({ "name": obj.name_any() })))
            .collect();

        Ok(items)
    }

    async fn watch_namespaced<K>(&self, app: AppHandle, kind: &str, ns: Option<String>, cancel_rx: oneshot::Receiver<()>) -> Result<()>
    where
        K: Clone + serde::de::DeserializeOwned + serde::Serialize + Resource<Scope = NamespaceResourceScope> + Send + Sync + std::fmt::Debug + 'static,
        <K as Resource>::DynamicType: Default + Eq + std::hash::Hash,
    {
        let client = self.current_client().await?;
        let api: Api<K> = match ns {
            Some(namespace) => Api::namespaced(client, &namespace),
            None => Api::all(client), // Watch all namespaces
        };

        self.spawn_watcher(api, app, kind, cancel_rx).await
    }

    async fn watch_cluster_scoped<K>(&self, app: AppHandle, kind: &str, cancel_rx: oneshot::Receiver<()>) -> Result<()>
    where
        K: Clone + serde::de::DeserializeOwned + serde::Serialize + Resource + Send + Sync + std::fmt::Debug + 'static,
        <K as Resource>::DynamicType: Default + Eq + std::hash::Hash,
    {
        let client = self.current_client().await?;
        let api: Api<K> = Api::all(client);
        self.spawn_watcher(api, app, kind, cancel_rx).await
    }

    async fn spawn_watcher<K>(&self, api: Api<K>, app: AppHandle, kind: &str, mut cancel_rx: oneshot::Receiver<()>) -> Result<()>
    where
        K: Clone + serde::de::DeserializeOwned + serde::Serialize + Resource + Send + Sync + std::fmt::Debug + 'static,
        <K as Resource>::DynamicType: Default + Eq + std::hash::Hash,
    {
        let handle = app.clone();
        let kind_name = kind.to_string();
        let api_clone = api.clone();

        tauri::async_runtime::spawn(async move {
            let mut reconnect_delay = tokio::time::Duration::from_secs(1);
            let max_reconnect_delay = tokio::time::Duration::from_secs(30);
            
            loop {
                // Check if we should cancel before starting a new watch
                if cancel_rx.try_recv().is_ok() {
                    eprintln!("[WATCH] Watch cancelled for {}", kind_name);
                    break;
                }

                let mut stream = watcher(api_clone.clone(), watcher::Config::default()).boxed();
                
                eprintln!("[WATCH] Started watch for {}", kind_name);
                let mut watch_active = true;

                while watch_active {
                    tokio::select! {
                        // Check for cancellation
                        _ = &mut cancel_rx => {
                            eprintln!("[WATCH] Watch cancelled for {}", kind_name);
                            watch_active = false;
                            break;
                        }
                        // Process watch events
                        evt_result = stream.try_next() => {
                            match evt_result {
                                Ok(Some(watcher::Event::Applied(obj))) => {
                                    let payload = WatchEventPayload {
                                        action: "applied".into(),
                                        kind: kind_name.clone(),
                                        object: serde_json::to_value(&obj).unwrap_or_else(|_| json!({ "name": obj.name_any() })),
                                    };
                                    let _ = handle.emit("resource://event", &payload);
                                    reconnect_delay = tokio::time::Duration::from_secs(1); // Reset delay on success
                                }
                                Ok(Some(watcher::Event::Deleted(obj))) => {
                                    let payload = WatchEventPayload {
                                        action: "deleted".into(),
                                        kind: kind_name.clone(),
                                        object: serde_json::to_value(&obj).unwrap_or_else(|_| json!({ "name": obj.name_any() })),
                                    };
                                    let _ = handle.emit("resource://event", &payload);
                                }
                                Ok(Some(watcher::Event::Restarted(objs))) => {
                                    for obj in objs {
                                        let payload = WatchEventPayload {
                                            action: "applied".into(),
                                            kind: kind_name.clone(),
                                            object: serde_json::to_value(&obj)
                                                .unwrap_or_else(|_| json!({ "name": obj.name_any() })),
                                        };
                                        let _ = handle.emit("resource://event", &payload);
                                    }
                                }
                                Ok(None) => {
                                    eprintln!("[WATCH] Stream ended for {}, reconnecting...", kind_name);
                                    watch_active = false;
                                    break;
                                }
                                Err(err) => {
                                    eprintln!("[WATCH] Error in watch stream for {}: {:?}, reconnecting...", kind_name, err);
                                    let payload = WatchEventPayload {
                                        action: "error".into(),
                                        kind: kind_name.clone(),
                                        object: json!({ "message": err.to_string() }),
                                    };
                                    let _ = handle.emit("resource://event", &payload);
                                    watch_active = false;
                                    break;
                                }
                            }
                        }
                    }
                }

                // If we broke out due to cancellation, exit the loop
                if !watch_active && cancel_rx.try_recv().is_ok() {
                    break;
                }

                // Reconnect with exponential backoff
                eprintln!("[WATCH] Reconnecting watch for {} in {:?}...", kind_name, reconnect_delay);
                tokio::time::sleep(reconnect_delay).await;
                reconnect_delay = std::cmp::min(reconnect_delay * 2, max_reconnect_delay);
            }
            
            eprintln!("[WATCH] Watch task ended for {}", kind_name);
        });

        Ok(())
    }

    pub async fn fetch_logs(
        &self,
        namespace: String,
        pod: String,
        container: Option<String>,
        tail_lines: Option<i64>,
    ) -> Result<String> {
        let client = self.current_client().await?;
        let api: Api<k8s_openapi::api::core::v1::Pod> = Api::namespaced(client, &namespace);
        let mut lp = LogParams::default();
        lp.container = container;
        lp.tail_lines = tail_lines;
        api.logs(&pod, &lp).await.map_err(K8sError::Kube)
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

    pub async fn stream_pod_logs(
        &self,
        app: AppHandle,
        namespace: String,
        pod_name: String,
        container: Option<String>,
    ) -> Result<()> {
        let state = self.clone();
        let handle = app.clone();
        let event_name = format!("pod-logs://{}/{}", namespace, pod_name);
        let ns = namespace.clone();
        let pn = pod_name.clone();
        let cont = container.clone();
        
        tauri::async_runtime::spawn(async move {
            // First, get initial logs
            let mut last_size = match state.fetch_logs(ns.clone(), pn.clone(), cont.clone(), Some(200)).await {
                Ok(logs) => {
                    let _ = handle.emit(&event_name, &json!({ "logs": logs, "append": false }));
                    logs.len() as u64
                }
                Err(err) => {
                    let _ = handle.emit(&event_name, &json!({ "error": err.to_string() }));
                    return; // Exit early if initial logs fail
                }
            };
            
            // Then poll for updates
            loop {
                tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                
                match state.fetch_logs(ns.clone(), pn.clone(), cont.clone(), None).await {
                    Ok(logs) => {
                        let current_size = logs.len() as u64;
                        if current_size > last_size {
                            // Only send the new portion
                            let new_logs: String = logs.chars().skip(last_size as usize).collect();
                            if !new_logs.is_empty() {
                                let _ = handle.emit(&event_name, &json!({ "logs": new_logs, "append": true }));
                                last_size = current_size;
                            }
                        }
                    }
                    Err(err) => {
                        let _ = handle.emit(&event_name, &json!({ "error": err.to_string() }));
                        break;
                    }
                }
            }
        });
        
        Ok(())
    }

    pub async fn delete_resource(&self, kind: ResourceKind, namespace: String, name: String) -> Result<()> {
        let client = self.current_client().await?;
        let dp = DeleteParams::default();

        match kind {
            ResourceKind::Pods => {
                let api: Api<k8s_openapi::api::core::v1::Pod> = Api::namespaced(client, &namespace);
                api.delete(&name, &dp).await.map_err(K8sError::Kube)?;
            }
            ResourceKind::Deployments => {
                let api: Api<k8s_openapi::api::apps::v1::Deployment> = Api::namespaced(client, &namespace);
                api.delete(&name, &dp).await.map_err(K8sError::Kube)?;
            }
            ResourceKind::Services => {
                let api: Api<k8s_openapi::api::core::v1::Service> = Api::namespaced(client, &namespace);
                api.delete(&name, &dp).await.map_err(K8sError::Kube)?;
            }
            ResourceKind::Nodes => {
                let api: Api<k8s_openapi::api::core::v1::Node> = Api::all(client);
                api.delete(&name, &dp).await.map_err(K8sError::Kube)?;
            }
        }

        Ok(())
    }

    pub async fn get_pod_metrics(
        &self,
        namespace: String,
        pod_name: String,
    ) -> Result<serde_json::Value> {
        let client = self.current_client().await?;
        
        // Construct ApiResource for metrics.k8s.io/v1beta1 PodMetrics
        // The plural name for PodMetrics is "pods" in the metrics API
        let gvk = GroupVersionKind::gvk("metrics.k8s.io", "v1beta1", "PodMetrics");
        let api_resource = ApiResource::from_gvk_with_plural(&gvk, "pods");
        
        // Ensure the ApiResource is namespaced (which it should be for pods)
        let api: Api<DynamicObject> = Api::namespaced_with(client, &namespace, &api_resource);
        
        // Try to get the metrics
        match api.get(&pod_name).await {
            Ok(metrics) => {
                serde_json::to_value(metrics).map_err(K8sError::Serde)
            }
            Err(kube::Error::Api(kube::error::ErrorResponse { code: 404, message, reason, .. })) => {
                // For 404, check if it's likely a metrics server issue
                // Common reasons: pod not found, metrics server not available
                let error_msg = if message.contains("not found") || message.is_empty() {
                    format!("Pod '{}' metrics not found. {}", pod_name, 
                        if message.is_empty() { "Metrics Server may not be available." } else { &message })
                } else {
                    format!("Metrics Server error: {}", message)
                };
                
                eprintln!("Metrics fetch failed (404): reason={}, message={}", reason, message);
                Err(K8sError::Kube(kube::Error::Api(kube::error::ErrorResponse {
                    code: 404,
                    message: error_msg,
                    reason: "NotFound".to_string(),
                    status: "Failure".to_string(),
                })))
            }
            Err(kube::Error::Api(kube::error::ErrorResponse { code, message, reason, .. })) => {
                // Other API errors - log and pass through
                let error_msg = format!("Metrics API error ({}): {}", code, message);
                eprintln!("Metrics API error: code={}, reason={}, message={}", code, reason, message);
                Err(K8sError::Kube(kube::Error::Api(kube::error::ErrorResponse {
                    code,
                    message: error_msg,
                    reason,
                    status: "Failure".to_string(),
                })))
            }
            Err(e) => {
                // Log the actual error for debugging
                eprintln!("Error fetching metrics for pod {}/{}: {:?}", namespace, pod_name, e);
                Err(K8sError::Kube(e))
            }
        }
    }

    pub async fn start_port_forward(
        &self,
        namespace: String,
        pod_name: String,
        local_port: u16,
        pod_port: u16,
    ) -> Result<serde_json::Value> {
        let forward_id = format!("{}/{}/{}/{}", namespace, pod_name, local_port, pod_port);
        
        // Check if already exists
        {
            let inner = self.inner.read().await;
            if inner.port_forwards.contains_key(&forward_id) {
                return Err(K8sError::Kube(kube::Error::Api(kube::error::ErrorResponse {
                    code: 409,
                    message: format!("Port forward {} already exists", forward_id),
                    reason: "Conflict".to_string(),
                    status: "Failure".to_string(),
                })));
            }
        }

        // Try to bind to the port immediately to catch errors early
        let addr = format!("127.0.0.1:{}", local_port);
        let listener = TcpListener::bind(&addr).await
            .map_err(|e| {
                let error_msg = if e.kind() == ErrorKind::AddrInUse {
                    format!("Port {} is already in use", local_port)
                } else {
                    format!("Failed to bind to {}: {}", addr, e)
                };
                K8sError::Kube(kube::Error::Service(error_msg.into()))
            })?;

        // Create cancellation channel
        let (cancel_tx, cancel_rx) = oneshot::channel();
        
        // Store the cancel handle
        {
            let mut inner = self.inner.write().await;
            inner.port_forwards.insert(
                forward_id.clone(),
                PortForwardHandle { cancel: cancel_tx },
            );
        }

        let state = self.clone();
        let ns = namespace.clone();
        let pn = pod_name.clone();
        
        // Spawn the port forward task with the already-bound listener
        tauri::async_runtime::spawn(async move {
            if let Err(e) = state.run_port_forward_with_listener(ns, pn, local_port, pod_port, listener, cancel_rx).await {
                eprintln!("[PORTFORWARD] Port forward error: {:?}", e);
                // Clean up on error
                let mut inner = state.inner.write().await;
                inner.port_forwards.remove(&forward_id);
            }
        });

        Ok(json!({
            "localPort": local_port,
            "podPort": pod_port,
            "status": "active"
        }))
    }

    async fn run_port_forward_with_listener(
        &self,
        namespace: String,
        pod_name: String,
        local_port: u16,
        pod_port: u16,
        listener: TcpListener,
        mut cancel_rx: oneshot::Receiver<()>,
    ) -> Result<()> {
        let _client = self.current_client().await?;
        
        eprintln!("[PORTFORWARD] Listening on 127.0.0.1:{} -> {}/{}:{}", local_port, namespace, pod_name, pod_port);
        
        loop {
            tokio::select! {
                // Check for cancellation
                _ = &mut cancel_rx => {
                    eprintln!("[PORTFORWARD] Port forward cancelled for {}/{}:{}", namespace, pod_name, pod_port);
                    break;
                }
                // Accept new connections
                result = listener.accept() => {
                    match result {
                        Ok((stream, addr)) => {
                            eprintln!("[PORTFORWARD] New connection from {}", addr);
                            let state = self.clone();
                            let ns = namespace.clone();
                            let pn = pod_name.clone();
                            let pp = pod_port;
                            
                            // Handle each connection in a separate task
                            tauri::async_runtime::spawn(async move {
                                if let Err(e) = handle_port_forward_connection(state, ns, pn, pp, stream).await {
                                    eprintln!("[PORTFORWARD] Connection error: {:?}", e);
                                }
                            });
                        }
                        Err(e) => {
                            eprintln!("[PORTFORWARD] Accept error: {:?}", e);
                            break;
                        }
                    }
                }
            }
        }
        
        Ok(())
    }

    pub async fn stop_port_forward(
        &self,
        namespace: String,
        pod_name: String,
        local_port: u16,
        pod_port: u16,
    ) -> Result<()> {
        let forward_id = format!("{}/{}/{}/{}", namespace, pod_name, local_port, pod_port);
        
        let mut inner = self.inner.write().await;
        if let Some(handle) = inner.port_forwards.remove(&forward_id) {
            let _ = handle.cancel.send(());
            eprintln!("[PORTFORWARD] Stopped port forward {}", forward_id);
            Ok(())
        } else {
            Err(K8sError::Kube(kube::Error::Api(kube::error::ErrorResponse {
                code: 404,
                message: format!("Port forward {} not found", forward_id),
                reason: "NotFound".to_string(),
                status: "Failure".to_string(),
            })))
        }
    }
}

async fn handle_port_forward_connection(
    state: K8sState,
    namespace: String,
    pod_name: String,
    pod_port: u16,
    local_stream: TcpStream,
) -> Result<()> {
    use tokio::process::Command;
    
    eprintln!("[PORTFORWARD] Handling connection for {}/{}:{}", namespace, pod_name, pod_port);
    
    // Use kubectl port-forward as a subprocess for reliable port forwarding
    // This is a pragmatic solution that works immediately
    // The local listener is already set up, so we'll connect kubectl to our local port
    // and forward to the pod
    
    // Get a random port for kubectl to listen on (we'll forward from local_stream to kubectl)
    // Actually, we need to set up kubectl to forward to a temporary port, then connect to it
    // Or better: use kubectl port-forward in server mode and connect our local_stream to it
    
    // Spawn kubectl port-forward process
    // We'll use a temporary local port for kubectl, then proxy between local_stream and kubectl
    let temp_port = 0; // Let kubectl choose a random port
    let mut child = Command::new("kubectl")
        .args(&[
            "port-forward",
            &format!("pod/{}", pod_name),
            &format!("{}:{}", temp_port, pod_port),
            "-n",
            &namespace,
            "--address=127.0.0.1",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| K8sError::Kube(kube::Error::Service(
            format!("Failed to spawn kubectl port-forward: {}. Make sure kubectl is installed and in PATH.", e).into()
        )))?;
    
    eprintln!("[PORTFORWARD] Using kubectl port-forward as bridge");
    
    // Kill the child we just spawned (it was a test)
    let _ = child.kill().await;
    
    // Use kubectl port-forward to a temporary port, then proxy to it
    // Find an available port for kubectl
    let temp_listener = TcpListener::bind("127.0.0.1:0").await
        .map_err(|e| K8sError::Kube(kube::Error::Service(
            format!("Failed to bind temp port: {}", e).into()
        )))?;
    let temp_port = temp_listener.local_addr()
        .map_err(|e| K8sError::Kube(kube::Error::Service(
            format!("Failed to get temp port: {}", e).into()
        )))?
        .port();
    drop(temp_listener); // Release it so kubectl can use it
    
    eprintln!("[PORTFORWARD] Using temporary port {} for kubectl", temp_port);
    
    // Spawn kubectl to forward pod:port to temp_port
    let mut kubectl = Command::new("kubectl")
        .args(&[
            "port-forward",
            &format!("pod/{}", pod_name),
            &format!("{}:{}", temp_port, pod_port),
            "-n",
            &namespace,
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| K8sError::Kube(kube::Error::Service(
            format!("Failed to spawn kubectl port-forward: {}. Make sure kubectl is installed and in PATH.", e).into()
        )))?;
    
    // Wait a moment for kubectl to establish the connection
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    
    // Check if kubectl is still running (if it errored, it would have exited)
    if let Ok(Some(status)) = kubectl.try_wait() {
        if !status.success() {
            let mut stderr = kubectl.stderr.take().unwrap();
            let mut error_output = String::new();
            use tokio::io::AsyncReadExt;
            let _ = stderr.read_to_string(&mut error_output).await;
            return Err(K8sError::Kube(kube::Error::Service(
                format!("kubectl port-forward failed: {}", error_output).into()
            )));
        }
    }
    
    // Connect to kubectl's forwarded port
    let temp_addr = format!("127.0.0.1:{}", temp_port);
    let mut kubectl_stream = loop {
        match TcpStream::connect(&temp_addr).await {
            Ok(stream) => break stream,
            Err(_) => {
                // Retry a few times
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                // Check if kubectl died
                if let Ok(Some(status)) = kubectl.try_wait() {
                    return Err(K8sError::Kube(kube::Error::Service(
                        "kubectl port-forward process exited unexpectedly".into()
                    )));
                }
            }
        }
    };
    
    eprintln!("[PORTFORWARD] Connected to kubectl bridge on {}", temp_addr);
    
    // Forward between local_stream and kubectl_stream
    let (mut local_read, mut local_write) = tokio::io::split(local_stream);
    let (mut kubectl_read, mut kubectl_write) = tokio::io::split(kubectl_stream);
    
    // Forward local -> kubectl (to pod)
    let forward_to_kubectl = tauri::async_runtime::spawn(async move {
        let mut buf = vec![0u8; 4096];
        loop {
            match local_read.read(&mut buf).await {
                Ok(0) => {
                    eprintln!("[PORTFORWARD] Local connection closed");
                    break;
                }
                Ok(n) => {
                    if kubectl_write.write_all(&buf[..n]).await.is_err() {
                        eprintln!("[PORTFORWARD] Error writing to kubectl");
                        break;
                    }
                    if kubectl_write.flush().await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    eprintln!("[PORTFORWARD] Error reading from local: {:?}", e);
                    break;
                }
            }
        }
    });
    
    // Forward kubectl -> local (from pod)
    let forward_from_kubectl = tauri::async_runtime::spawn(async move {
        let mut buf = vec![0u8; 4096];
        loop {
            match kubectl_read.read(&mut buf).await {
                Ok(0) => {
                    eprintln!("[PORTFORWARD] kubectl connection closed");
                    break;
                }
                Ok(n) => {
                    if local_write.write_all(&buf[..n]).await.is_err() {
                        eprintln!("[PORTFORWARD] Error writing to local");
                        break;
                    }
                    if local_write.flush().await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    eprintln!("[PORTFORWARD] Error reading from kubectl: {:?}", e);
                    break;
                }
            }
        }
    });
    
    // Wait for forwarding to complete
    tokio::select! {
        _ = forward_to_kubectl => {
            eprintln!("[PORTFORWARD] Local -> kubectl forwarding ended");
        }
        _ = forward_from_kubectl => {
            eprintln!("[PORTFORWARD] kubectl -> Local forwarding ended");
        }
    }
    
    // Clean up kubectl process
    let _ = kubectl.kill().await;
    
    eprintln!("[PORTFORWARD] Connection closed");
    
    Ok(())
}

