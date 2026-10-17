use crate::error::{K8sError, Result};
use crate::state::K8sState;
use kube::api::{Api, DynamicObject};
use kube::core::{ApiResource, GroupVersionKind};
use tracing::warn;

impl K8sState {
    pub async fn get_pod_metrics(
        &self,
        namespace: String,
        pod_name: String,
    ) -> Result<serde_json::Value> {
        let client = self.current_client().await?;

        let gvk = GroupVersionKind::gvk("metrics.k8s.io", "v1beta1", "PodMetrics");
        let api_resource = ApiResource::from_gvk_with_plural(&gvk, "pods");
        let api: Api<DynamicObject> = Api::namespaced_with(client, &namespace, &api_resource);

        match api.get(&pod_name).await {
            Ok(metrics) => serde_json::to_value(metrics).map_err(K8sError::Serde),
            Err(kube::Error::Api(kube::error::ErrorResponse {
                code: 404,
                message,
                reason,
                ..
            })) => {
                let error_msg = if message.contains("not found") || message.is_empty() {
                    format!(
                        "Pod '{}' metrics not found. {}",
                        pod_name,
                        if message.is_empty() {
                            "Metrics Server may not be available."
                        } else {
                            &message
                        }
                    )
                } else {
                    format!("Metrics Server error: {message}")
                };

                warn!(
                    pod = %pod_name,
                    reason = %reason,
                    "Metrics fetch failed (404)"
                );
                Err(K8sError::Kube(kube::Error::Api(
                    kube::error::ErrorResponse {
                        code: 404,
                        message: error_msg,
                        reason: "NotFound".to_string(),
                        status: "Failure".to_string(),
                    },
                )))
            }
            Err(kube::Error::Api(kube::error::ErrorResponse {
                code,
                message,
                reason,
                ..
            })) => {
                let error_msg = format!("Metrics API error ({code}): {message}");
                warn!(
                    code,
                    reason = %reason,
                    message = %message,
                    "Metrics API error"
                );
                Err(K8sError::Kube(kube::Error::Api(
                    kube::error::ErrorResponse {
                        code,
                        message: error_msg,
                        reason,
                        status: "Failure".to_string(),
                    },
                )))
            }
            Err(e) => {
                warn!(
                    pod = %pod_name,
                    namespace = %namespace,
                    error = %e,
                    "Error fetching metrics"
                );
                Err(K8sError::Kube(e))
            }
        }
    }
}
