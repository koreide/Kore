use crate::constants::{
    MAX_KUBECTL_CONNECT_RETRIES, KUBECTL_RETRY_INTERVAL, PORT_FORWARD_BUFFER_SIZE,
};
use crate::error::{K8sError, Result};
use crate::state::K8sState;
use serde_json::json;
use std::collections::HashMap;
use std::io::ErrorKind;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

/// Manages port forwarding lifecycle with cancellation tokens.
#[derive(Clone)]
pub struct PortForwardManager {
    forwards: Arc<RwLock<HashMap<String, CancellationToken>>>,
}

impl PortForwardManager {
    pub fn new() -> Self {
        Self {
            forwards: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Cancel all active port forwards.
    pub async fn cancel_all(&self) {
        let mut forwards = self.forwards.write().await;
        for (id, token) in forwards.drain() {
            token.cancel();
            info!(forward_id = %id, "Cancelled port forward");
        }
    }
}

impl K8sState {
    pub async fn start_port_forward(
        &self,
        namespace: String,
        pod_name: String,
        local_port: u16,
        pod_port: u16,
    ) -> Result<serde_json::Value> {
        let forward_id = format!("{namespace}/{pod_name}/{local_port}/{pod_port}");

        // Check if already exists
        {
            let forwards = self.port_forwards.forwards.read().await;
            if forwards.contains_key(&forward_id) {
                return Err(K8sError::PortForward(format!(
                    "Port forward {forward_id} already exists"
                )));
            }
        }

        // Bind early to catch errors
        let addr = format!("127.0.0.1:{local_port}");
        let listener = TcpListener::bind(&addr).await.map_err(|e| {
            if e.kind() == ErrorKind::AddrInUse {
                K8sError::PortForward(format!("Port {local_port} is already in use"))
            } else {
                K8sError::PortForward(format!("Failed to bind to {addr}: {e}"))
            }
        })?;

        // Create cancellation token
        let cancel_token = CancellationToken::new();

        // Store it
        {
            let mut forwards = self.port_forwards.forwards.write().await;
            forwards.insert(forward_id.clone(), cancel_token.clone());
        }

        let state = self.clone();
        let ns = namespace.clone();
        let pn = pod_name.clone();
        let fid = forward_id.clone();

        tauri::async_runtime::spawn(async move {
            if let Err(e) = run_port_forward(ns, pn, pod_port, listener, cancel_token).await {
                error!(forward_id = %fid, error = %e, "Port forward error");
            }
            // Clean up on exit
            let mut forwards = state.port_forwards.forwards.write().await;
            forwards.remove(&fid);
        });

        Ok(json!({
            "localPort": local_port,
            "podPort": pod_port,
            "status": "active"
        }))
    }

    pub async fn stop_port_forward(
        &self,
        namespace: String,
        pod_name: String,
        local_port: u16,
        pod_port: u16,
    ) -> Result<()> {
        let forward_id = format!("{namespace}/{pod_name}/{local_port}/{pod_port}");

        let mut forwards = self.port_forwards.forwards.write().await;
        if let Some(token) = forwards.remove(&forward_id) {
            token.cancel();
            info!(forward_id = %forward_id, "Stopped port forward");
            Ok(())
        } else {
            Err(K8sError::PortForward(format!(
                "Port forward {forward_id} not found"
            )))
        }
    }
}

async fn run_port_forward(
    namespace: String,
    pod_name: String,
    pod_port: u16,
    listener: TcpListener,
    cancel_token: CancellationToken,
) -> Result<()> {
    info!(
        namespace = %namespace,
        pod = %pod_name,
        port = pod_port,
        local_addr = %listener.local_addr().unwrap(),
        "Port forward listening"
    );

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                info!(pod = %pod_name, port = pod_port, "Port forward cancelled");
                break;
            }
            result = listener.accept() => {
                match result {
                    Ok((stream, addr)) => {
                        debug!(addr = %addr, "New port forward connection");
                        let ns = namespace.clone();
                        let pn = pod_name.clone();
                        let ct = cancel_token.clone();

                        tauri::async_runtime::spawn(async move {
                            if let Err(e) = handle_port_forward_connection(ns, pn, pod_port, stream, ct).await {
                                warn!(error = %e, "Port forward connection error");
                            }
                        });
                    }
                    Err(e) => {
                        error!(error = %e, "Port forward accept error");
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}

async fn handle_port_forward_connection(
    namespace: String,
    pod_name: String,
    pod_port: u16,
    local_stream: TcpStream,
    cancel_token: CancellationToken,
) -> Result<()> {
    use tokio::process::Command;

    debug!(
        namespace = %namespace,
        pod = %pod_name,
        port = pod_port,
        "Handling port forward connection"
    );

    // Find a temporary port for kubectl
    let temp_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| K8sError::PortForward(format!("Failed to bind temp port: {e}")))?;
    let temp_port = temp_listener
        .local_addr()
        .map_err(|e| K8sError::PortForward(format!("Failed to get temp port: {e}")))?
        .port();
    drop(temp_listener);

    debug!(temp_port, "Using temporary port for kubectl");

    // Spawn kubectl port-forward process
    let mut kubectl = Command::new("kubectl")
        .args([
            "port-forward",
            &format!("pod/{pod_name}"),
            &format!("{temp_port}:{pod_port}"),
            "-n",
            &namespace,
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| {
            K8sError::PortForward(format!(
                "Failed to spawn kubectl port-forward: {e}. Make sure kubectl is installed and in PATH."
            ))
        })?;

    // Wait for kubectl to establish the connection
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Check if kubectl is still running
    if let Ok(Some(status)) = kubectl.try_wait() {
        if !status.success() {
            let error_output = if let Some(mut stderr) = kubectl.stderr.take() {
                let mut buf = String::new();
                let _ = stderr.read_to_string(&mut buf).await;
                buf
            } else {
                "kubectl exited with no error output".to_string()
            };
            return Err(K8sError::PortForward(format!(
                "kubectl port-forward failed: {error_output}"
            )));
        }
    }

    // Connect to kubectl's forwarded port with bounded retries
    let temp_addr = format!("127.0.0.1:{temp_port}");
    let mut kubectl_stream = None;
    for _attempt in 0..MAX_KUBECTL_CONNECT_RETRIES {
        match TcpStream::connect(&temp_addr).await {
            Ok(stream) => {
                kubectl_stream = Some(stream);
                break;
            }
            Err(_) => {
                // Check if kubectl died
                if let Ok(Some(_status)) = kubectl.try_wait() {
                    let _ = kubectl.kill().await;
                    return Err(K8sError::PortForward(
                        "kubectl port-forward process exited unexpectedly".to_string(),
                    ));
                }
                tokio::time::sleep(KUBECTL_RETRY_INTERVAL).await;
            }
        }
    }

    let kubectl_stream = match kubectl_stream {
        Some(s) => s,
        None => {
            let _ = kubectl.kill().await;
            return Err(K8sError::PortForward(format!(
                "Failed to connect to kubectl bridge on {temp_addr} after {MAX_KUBECTL_CONNECT_RETRIES} retries"
            )));
        }
    };

    debug!(addr = %temp_addr, "Connected to kubectl bridge");

    // Forward between local_stream and kubectl_stream
    let (mut local_read, mut local_write) = tokio::io::split(local_stream);
    let (mut kubectl_read, mut kubectl_write) = tokio::io::split(kubectl_stream);

    let ct1 = cancel_token.clone();
    let ct2 = cancel_token.clone();

    let forward_to_kubectl = tauri::async_runtime::spawn(async move {
        let mut buf = vec![0u8; PORT_FORWARD_BUFFER_SIZE];
        loop {
            tokio::select! {
                _ = ct1.cancelled() => break,
                result = local_read.read(&mut buf) => {
                    match result {
                        Ok(0) => break,
                        Ok(n) => {
                            if kubectl_write.write_all(&buf[..n]).await.is_err() {
                                break;
                            }
                            if kubectl_write.flush().await.is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            }
        }
    });

    let forward_from_kubectl = tauri::async_runtime::spawn(async move {
        let mut buf = vec![0u8; PORT_FORWARD_BUFFER_SIZE];
        loop {
            tokio::select! {
                _ = ct2.cancelled() => break,
                result = kubectl_read.read(&mut buf) => {
                    match result {
                        Ok(0) => break,
                        Ok(n) => {
                            if local_write.write_all(&buf[..n]).await.is_err() {
                                break;
                            }
                            if local_write.flush().await.is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            }
        }
    });

    tokio::select! {
        _ = forward_to_kubectl => {}
        _ = forward_from_kubectl => {}
        _ = cancel_token.cancelled() => {}
    }

    // Clean up kubectl process
    let _ = kubectl.kill().await;

    debug!("Port forward connection closed");

    Ok(())
}
