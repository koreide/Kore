use crate::constants::DEFAULT_LOG_TAIL_LINES;
use crate::error::{K8sError, Result};
use crate::events::{channel, PodLogPayload, Subscription};
use crate::state::K8sState;
use futures::StreamExt;
use kube::api::{Api, LogParams};
use tracing::{info, warn};

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

    /// Stream a pod's logs (all containers when `container` is `None`,
    /// lines prefixed `[container] `). Each call returns an independent
    /// [`Subscription`]; drop it to stop. `stop_pod_logs_stream`, context
    /// switch and shutdown cancel every log stream.
    pub async fn stream_pod_logs(
        &self,
        namespace: String,
        pod_name: String,
        container: Option<String>,
        previous: bool,
    ) -> Result<Subscription<PodLogPayload>> {
        let client = self.current_client().await?;

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
        let (app, sub) = channel::<PodLogPayload>(&self.logs_scope());

        for cont_name in containers {
            let client = client.clone();
            let handle = app.clone();
            let ns = namespace.clone();
            let pn = pod_name.clone();
            let token = app.token().clone();

            tokio::spawn(async move {
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
                                let _ = handle.send(PodLogPayload::info(
                                    "No previous logs — this container has not been restarted.\n"
                                        .to_string(),
                                ));
                            }
                            return;
                        }
                        let _ = handle.send(PodLogPayload::error(err_str));
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
                                    if handle.send(PodLogPayload::line(log_line)).is_err() {
                                        info!("Log subscriber gone, stopping log stream");
                                        break;
                                    }
                                }
                                Some(Err(err)) => {
                                    warn!(pod = %pn, container = %cont_name, error = %err, "Log stream error");
                                    let _ = handle.send(PodLogPayload::error(err.to_string()));
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

        Ok(sub)
    }

    /// Cancel every active log stream (single-pod and multi-pod). Prefer dropping the individual [`Subscription`].
    pub async fn stop_pod_logs_stream(&self) -> Result<()> {
        self.cancel_logs();
        Ok(())
    }
}
