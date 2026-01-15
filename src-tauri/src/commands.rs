use crate::kube_state::{K8sState, ResourceKind};
use serde::Deserialize;
use tauri::State;

#[derive(Debug, Deserialize)]
pub struct LogRequest {
    pub namespace: String,
    pub pod: String,
    pub container: Option<String>,
    pub tail_lines: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct PortForwardRequest {
    pub namespace: String,
    pub podName: String,
    pub localPort: u16,
    pub podPort: u16,
}

#[tauri::command]
pub async fn list_contexts(state: State<'_, K8sState>) -> std::result::Result<Vec<String>, String> {
    state.list_contexts().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_namespaces(state: State<'_, K8sState>) -> std::result::Result<Vec<String>, String> {
    state.list_namespaces().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn switch_context(
    state: State<'_, K8sState>,
    name: String,
) -> std::result::Result<String, String> {
    state.switch_context(name).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_resources(
    state: State<'_, K8sState>,
    kind: ResourceKind,
    namespace: Option<String>,
) -> std::result::Result<Vec<serde_json::Value>, String> {
    state
        .list_resources(kind, namespace)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn start_watch(
    app: tauri::AppHandle,
    state: State<'_, K8sState>,
    kind: ResourceKind,
    namespace: Option<String>,
) -> std::result::Result<(), String> {
    state
        .start_watch(app, kind, namespace)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fetch_logs(
    state: State<'_, K8sState>,
    req: LogRequest,
) -> std::result::Result<String, String> {
    state
        .fetch_logs(req.namespace, req.pod, req.container, req.tail_lines)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_resource(
    state: State<'_, K8sState>,
    kind: ResourceKind,
    namespace: String,
    name: String,
) -> std::result::Result<(), String> {
    state
        .delete_resource(kind, namespace, name)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn describe_pod(
    state: State<'_, K8sState>,
    namespace: String,
    podName: String,
) -> std::result::Result<serde_json::Value, String> {
    state
        .get_pod(namespace, podName)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn start_pod_logs_stream(
    app: tauri::AppHandle,
    state: State<'_, K8sState>,
    namespace: String,
    podName: String,
    container: Option<String>,
) -> std::result::Result<(), String> {
    state
        .stream_pod_logs(app, namespace, podName, container)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_pod_metrics(
    state: State<'_, K8sState>,
    namespace: String,
    podName: String,
) -> std::result::Result<serde_json::Value, String> {
    state
        .get_pod_metrics(namespace, podName)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn start_port_forward(
    state: State<'_, K8sState>,
    namespace: String,
    podName: String,
    localPort: u16,
    podPort: u16,
) -> std::result::Result<serde_json::Value, String> {
    state
        .start_port_forward(namespace, podName, localPort, podPort)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn stop_port_forward(
    state: State<'_, K8sState>,
    namespace: String,
    podName: String,
    localPort: u16,
    podPort: u16,
) -> std::result::Result<(), String> {
    state
        .stop_port_forward(namespace, podName, localPort, podPort)
        .await
        .map_err(|e| e.to_string())
}
