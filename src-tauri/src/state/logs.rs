use crate::constants::DEFAULT_LOG_TAIL_LINES;
use crate::error::{K8sError, Result};
use crate::state::K8sState;
use futures::StreamExt;
use kube::api::{Api, LogParams};
use serde_json::json;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

/// Manages pod log streaming with cancellation.
#[derive(Clone)]
pub struct LogStreamer {
    pub(crate) cancel_token: Arc<RwLock<Option<CancellationToken>>>,
}

impl LogStreamer {
    pub fn new() -> Self {
        Self {
            cancel_token: Arc::new(RwLock::new(None)),
        }
    }

    /// Cancel any active log stream.
    pub async fn cancel(&self) {
        let mut token = self.cancel_token.write().await;
        if let Some(ct) = token.take() {
            ct.cancel();
            info!("Cancelled active log stream");
        }
    }
}

impl K8sState {
    pub async fn fetch_logs(
        &self,
        namespace: String,
        pod: String,
        container: Option<String>,
        tail_lines: Option<i64>,
        previous: bool,
    ) -> Result<String> {
        let client = self.current_client().await?;
        let api: Api<k8s_openapi::api::core::v1::Pod> = Api::namespaced(client, &namespace);
        let lp = LogParams {
            container,
            tail_lines,
            previous,
            ..Default::default()
        };
        api.logs(&pod, &lp).await.map_err(K8sError::Kube)
    }

    pub async fn stream_pod_logs(
        &self,
        app: AppHandle,
        namespace: String,
        pod_name: String,
        container: Option<String>,
        previous: bool,
    ) -> Result<()> {
        // Cancel any existing log stream, then create new token — atomically
        let new_token = {
            let mut token = self.logs.cancel_token.write().await;
            if let Some(old_token) = token.take() {
                old_token.cancel();
            }
            let new_token = CancellationToken::new();
            *token = Some(new_token.clone());
            new_token
        };

        let client = self.current_client().await?;
        let event_name = format!("pod-logs://{namespace}/{pod_name}");

        // Determine which containers to stream
        let containers: Vec<String> = if let Some(c) = container {
            vec![c]
        } else {
            // Fetch pod spec to discover containers
            let api: Api<k8s_openapi::api::core::v1::Pod> =
                Api::namespaced(client.clone(), &namespace);
            let pod = api.get(&pod_name).await.map_err(K8sError::Kube)?;
            let spec = pod.spec.unwrap_or_default();
            let names: Vec<String> = spec.containers.iter().map(|c| c.name.clone()).collect();
            if names.is_empty() {
                return Err(K8sError::Validation("Pod has no containers".to_string()));
            }
            names
        };

        let multi = containers.len() > 1;

        for cont_name in containers {
            let client = client.clone();
            let handle = app.clone();
            let event = event_name.clone();
            let ns = namespace.clone();
            let pn = pod_name.clone();
            let token = new_token.clone();

            tauri::async_runtime::spawn(async move {
                let api: Api<k8s_openapi::api::core::v1::Pod> = Api::namespaced(client, &ns);

                let lp = LogParams {
                    follow: !previous, // Can't follow previous logs
                    tail_lines: Some(DEFAULT_LOG_TAIL_LINES),
                    container: Some(cont_name.clone()),
                    previous,
                    ..Default::default()
                };

                let stream = match api.log_stream(&pn, &lp).await {
                    Ok(stream) => stream,
                    Err(err) => {
                        let err_str = err.to_string();
                        // "previous terminated container" not found means it was never restarted
                        if previous && err_str.contains("previous terminated container") {
                            if multi {
                                // All containers mode — silently skip
                                info!(pod = %pn, container = %cont_name, "No previous logs, skipping");
                            } else {
                                let _ = handle.emit(&event, &json!({ "logs": "No previous logs — this container has not been restarted.\n" }));
                            }
                            return;
                        }
                        if let Err(e) = handle.emit(&event, &json!({ "error": err_str })) {
                            error!(error = %e, "Failed to emit log error");
                        }
                        return;
                    }
                };

                use futures::AsyncBufReadExt;
                let mut lines_stream = stream.lines();

                loop {
                    tokio::select! {
                        _ = token.cancelled() => {
                            info!(pod = %pn, container = %cont_name, "Log stream cancelled");
                            return;
                        }
                        line_result = lines_stream.next() => {
                            match line_result {
                                Some(Ok(line)) => {
                                    let log_line = if multi {
                                        format!("[{cont_name}] {line}\n")
                                    } else {
                                        format!("{line}\n")
                                    };
                                    if let Err(e) = handle.emit(
                                        &event,
                                        &json!({ "logs": log_line, "append": true }),
                                    ) {
                                        warn!(error = %e, "Frontend listener gone, stopping log stream");
                                        break;
                                    }
                                }
                                Some(Err(err)) => {
                                    warn!(pod = %pn, container = %cont_name, error = %err, "Log stream error");
                                    if let Err(e) =
                                        handle.emit(&event, &json!({ "error": err.to_string() }))
                                    {
                                        error!(error = %e, "Failed to emit log error");
                                    }
                                    break;
                                }
                                None => {
                                    info!(pod = %pn, container = %cont_name, "Log stream ended");
                                    break;
                                }
                            }
                        }
                    }
                }
            });
        }

        Ok(())
    }

    pub async fn stop_pod_logs_stream(&self) -> Result<()> {
        self.logs.cancel().await;
        Ok(())
    }
}
