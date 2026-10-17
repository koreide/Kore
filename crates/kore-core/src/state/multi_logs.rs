use crate::error::{K8sError, Result};
use crate::events::{channel, MultiPodLogLine, MultiPodLogStream, PodColor};
use crate::state::K8sState;
use futures::{AsyncBufReadExt, StreamExt};
use kube::api::{Api, ListParams, LogParams, ResourceExt};
use tracing::{info, warn};

/// Palette of 8 colors for multi-pod log streams.
const POD_COLORS: [&str; 8] = [
    "#58d0ff", "#22c55e", "#f59e0b", "#ef4444", "#a78bfa", "#ec4899", "#06b6d4", "#84cc16",
];

impl K8sState {
    /// Stream logs from all pods matching a label selector. Returns the
    /// pod/color list and the line stream.
    /// Independent of single-pod log streams (no shared slot), but
    /// `stop_pod_logs_stream` still cancels it.
    pub async fn stream_multi_pod_logs(
        &self,
        namespace: String,
        label_selector: String,
        tail_lines: Option<i64>,
    ) -> Result<MultiPodLogStream> {
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

        let (app, lines) = channel::<MultiPodLogLine>(&self.logs_scope());

        for (i, pod) in pod_list.items.iter().enumerate() {
            let pod_name = pod.name_any();
            let color = POD_COLORS[i % POD_COLORS.len()].to_string();
            // Every regular container of the pod is streamed; with more than
            // one container each line is prefixed `[{container}] `.
            let containers = pod_containers(pod);
            let multi = containers.len() > 1;
            let targets: Vec<Option<String>> = if containers.is_empty() {
                vec![None]
            } else {
                containers.into_iter().map(Some).collect()
            };
            for container in targets {
                let prefix = match (&container, multi) {
                    (Some(c), true) => Some(container_prefix(c)),
                    _ => None,
                };
                let app_handle = app.clone();
                let ns = namespace.clone();
                let client = client.clone();
                let token = app.token().clone();
                let pod_name = pod_name.clone();
                let color = color.clone();

                tokio::spawn(async move {
                    let api: Api<k8s_openapi::api::core::v1::Pod> = Api::namespaced(client, &ns);

                    let lp = LogParams {
                        follow: true,
                        tail_lines: Some(tail_lines.unwrap_or(50)),
                        container: container.clone(),
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
                                                let line = match &prefix {
                                                    Some(p) => format!("{p}{line}"),
                                                    None => line,
                                                };
                                                let payload = MultiPodLogLine {
                                                    pod: pod_name.clone(),
                                                    color: color.clone(),
                                                    line,
                                                };
                                                if app_handle.send(payload).is_err() {
                                                    break;
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
                            warn!(pod = %pod_name, container = ?container, error = %e, "Failed to start log stream");
                        }
                    }
                });
            }
        }

        let pods: Vec<PodColor> = pod_list
            .items
            .iter()
            .enumerate()
            .map(|(i, pod)| PodColor {
                name: pod.name_any(),
                color: POD_COLORS[i % POD_COLORS.len()].to_string(),
            })
            .collect();

        Ok(MultiPodLogStream { pods, lines })
    }
}

/// Regular (non-init) container names of a pod, in spec order.
pub fn pod_containers(pod: &k8s_openapi::api::core::v1::Pod) -> Vec<String> {
    pod.spec
        .as_ref()
        .map(|s| s.containers.iter().map(|c| c.name.clone()).collect())
        .unwrap_or_default()
}

/// Line prefix for a container of a multi-container pod (as single-pod logs).
pub fn container_prefix(container: &str) -> String {
    format!("[{container}] ")
}

#[cfg(test)]
mod tests {
    use super::{container_prefix, pod_containers};
    use k8s_openapi::api::core::v1::Pod;

    #[test]
    fn lists_regular_containers_only() {
        let pod: Pod = serde_json::from_value(serde_json::json!({
            "metadata": {"name": "p"},
            "spec": {
                "initContainers": [{"name": "init"}],
                "containers": [{"name": "main"}, {"name": "sidecar"}]
            }
        }))
        .unwrap();
        assert_eq!(pod_containers(&pod), vec!["main", "sidecar"]);
        assert!(pod_containers(&Pod::default()).is_empty());
        assert_eq!(container_prefix("main"), "[main] ");
    }
}
