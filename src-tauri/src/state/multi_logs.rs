use crate::error::{K8sError, Result};
use crate::state::K8sState;
use futures::{AsyncBufReadExt, StreamExt};
use kube::api::{Api, ListParams, LogParams, ResourceExt};
use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

/// Palette of 8 colors for multi-pod log streams.
const POD_COLORS: [&str; 8] = [
    "#58d0ff", "#22c55e", "#f59e0b", "#ef4444", "#a78bfa", "#ec4899", "#06b6d4", "#84cc16",
];

impl K8sState {
    /// Stream logs from all pods matching a label selector, interleaved with pod-name prefix.
    pub async fn stream_multi_pod_logs(
        &self,
        app: AppHandle,
        namespace: String,
        label_selector: String,
        tail_lines: Option<i64>,
    ) -> Result<()> {
        let client = self.current_client().await?;
        let pod_api: Api<k8s_openapi::api::core::v1::Pod> =
            Api::namespaced(client.clone(), &namespace);

        let lp = ListParams::default().labels(&label_selector);
        let pod_list = pod_api.list(&lp).await.map_err(K8sError::Kube)?;

        if pod_list.items.is_empty() {
            return Err(K8sError::Validation(
                "No pods found matching the label selector".to_string(),
            ));
        }

        // Cancel previous multi-log stream if any
        self.logs.cancel().await;

        let cancel_token = CancellationToken::new();
        {
            let mut token = self.logs.cancel_token.write().await;
            *token = Some(cancel_token.clone());
        }

        let event_name = format!(
            "multi-pod-logs://{}/{}",
            namespace,
            label_selector.replace('=', "-").replace(',', "_")
        );

        for (i, pod) in pod_list.items.iter().enumerate() {
            let pod_name = pod.name_any();
            let color = POD_COLORS[i % POD_COLORS.len()].to_string();
            let app_handle = app.clone();
            let ns = namespace.clone();
            let client = client.clone();
            let token = cancel_token.clone();
            let event = event_name.clone();

            tauri::async_runtime::spawn(async move {
                let api: Api<k8s_openapi::api::core::v1::Pod> = Api::namespaced(client, &ns);

                let lp = LogParams {
                    follow: true,
                    tail_lines: Some(tail_lines.unwrap_or(50)),
                    ..Default::default()
                };

                match api.log_stream(&pod_name, &lp).await {
                    Ok(stream) => {
                        let mut lines_stream = stream.lines();
                        loop {
                            tokio::select! {
                                _ = token.cancelled() => {
                                    info!(pod = %pod_name, "Multi-pod log stream cancelled");
                                    break;
                                }
                                line_result = lines_stream.next() => {
                                    match line_result {
                                        Some(Ok(line)) => {
                                            let payload = serde_json::json!({
                                                "pod": pod_name,
                                                "color": color,
                                                "line": line,
                                            });
                                            if let Err(e) = app_handle.emit(&event, &payload) {
                                                error!(error = %e, "Failed to emit multi-pod log line");
                                            }
                                        }
                                        Some(Err(e)) => {
                                            warn!(pod = %pod_name, error = %e, "Log stream error");
                                            break;
                                        }
                                        None => break,
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!(pod = %pod_name, error = %e, "Failed to start log stream");
                    }
                }
            });
        }

        // Emit initial pod list
        let pods_info: Vec<serde_json::Value> = pod_list
            .items
            .iter()
            .enumerate()
            .map(|(i, pod)| {
                serde_json::json!({
                    "name": pod.name_any(),
                    "color": POD_COLORS[i % POD_COLORS.len()],
                })
            })
            .collect();

        let _ = app.emit(
            &format!("{event_name}/pods"),
            &serde_json::json!({ "pods": pods_info }),
        );

        Ok(())
    }
}
