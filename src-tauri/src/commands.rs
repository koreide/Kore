use crate::state::{K8sState, ResourceKind};
use serde::Deserialize;
use std::collections::HashMap;
use tauri::State;

#[derive(Debug, Deserialize)]
pub struct LogRequest {
    pub namespace: String,
    pub pod: String,
    pub container: Option<String>,
    pub tail_lines: Option<i64>,
    pub previous: Option<bool>,
}

// ── Connection status ─────────────────────────────────────────────────

#[tauri::command]
pub async fn get_connection_status(
    state: State<'_, K8sState>,
) -> std::result::Result<crate::error::ConnectionStatus, String> {
    Ok(state.get_connection_status().await)
}

#[tauri::command]
pub async fn retry_connection(
    state: State<'_, K8sState>,
    context: Option<String>,
) -> std::result::Result<crate::error::ConnectionStatus, String> {
    Ok(state.retry_connection(context).await)
}

// ── Existing commands ──────────────────────────────────────────────────

#[tauri::command]
pub async fn list_contexts(state: State<'_, K8sState>) -> std::result::Result<Vec<String>, String> {
    state.list_contexts().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_namespaces(
    state: State<'_, K8sState>,
) -> std::result::Result<Vec<String>, String> {
    state.list_namespaces().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn switch_context(
    state: State<'_, K8sState>,
    name: String,
) -> std::result::Result<String, String> {
    if name.is_empty() {
        return Err("Context name cannot be empty".to_string());
    }
    state.switch_context(name).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_resources(
    state: State<'_, K8sState>,
    kind: ResourceKind,
    namespace: Option<String>,
    #[allow(non_snake_case)] labelSelector: Option<String>,
) -> std::result::Result<Vec<serde_json::Value>, String> {
    state
        .list_resources(kind, namespace, labelSelector)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn start_watch(
    app: tauri::AppHandle,
    state: State<'_, K8sState>,
    kind: ResourceKind,
    namespace: Option<String>,
    #[allow(non_snake_case)] labelSelector: Option<String>,
) -> std::result::Result<(), String> {
    state
        .start_watch(app, kind, namespace, labelSelector)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fetch_logs(
    state: State<'_, K8sState>,
    req: LogRequest,
) -> std::result::Result<String, String> {
    if req.namespace.is_empty() || req.pod.is_empty() {
        return Err("Namespace and pod name are required".to_string());
    }
    state
        .fetch_logs(
            req.namespace,
            req.pod,
            req.container,
            req.tail_lines,
            req.previous.unwrap_or(false),
        )
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
    if namespace.is_empty() || name.is_empty() {
        return Err("Namespace and resource name are required".to_string());
    }
    state
        .delete_resource(kind, namespace, name)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn describe_pod(
    state: State<'_, K8sState>,
    namespace: String,
    #[allow(non_snake_case)] podName: String,
) -> std::result::Result<serde_json::Value, String> {
    if namespace.is_empty() || podName.is_empty() {
        return Err("Namespace and pod name are required".to_string());
    }
    state
        .get_pod(namespace, podName)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn describe_resource(
    state: State<'_, K8sState>,
    kind: ResourceKind,
    namespace: String,
    name: String,
) -> std::result::Result<serde_json::Value, String> {
    if name.is_empty() {
        return Err("Resource name is required".to_string());
    }
    state
        .describe_resource(kind, namespace, name)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_events_for_resource(
    state: State<'_, K8sState>,
    kind: String,
    namespace: String,
    name: String,
) -> std::result::Result<Vec<serde_json::Value>, String> {
    if namespace.is_empty() || name.is_empty() {
        return Err("Namespace and resource name are required".to_string());
    }
    state
        .list_events_for_resource(kind, namespace, name)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn start_pod_logs_stream(
    app: tauri::AppHandle,
    state: State<'_, K8sState>,
    namespace: String,
    #[allow(non_snake_case)] podName: String,
    container: Option<String>,
    previous: Option<bool>,
) -> std::result::Result<(), String> {
    if namespace.is_empty() || podName.is_empty() {
        return Err("Namespace and pod name are required".to_string());
    }
    state
        .stream_pod_logs(
            app,
            namespace,
            podName,
            container,
            previous.unwrap_or(false),
        )
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn stop_pod_logs_stream(state: State<'_, K8sState>) -> std::result::Result<(), String> {
    state
        .stop_pod_logs_stream()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_pod_metrics(
    state: State<'_, K8sState>,
    namespace: String,
    #[allow(non_snake_case)] podName: String,
) -> std::result::Result<serde_json::Value, String> {
    if namespace.is_empty() || podName.is_empty() {
        return Err("Namespace and pod name are required".to_string());
    }
    state
        .get_pod_metrics(namespace, podName)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn start_port_forward(
    state: State<'_, K8sState>,
    namespace: String,
    #[allow(non_snake_case)] podName: String,
    #[allow(non_snake_case)] localPort: u16,
    #[allow(non_snake_case)] podPort: u16,
) -> std::result::Result<serde_json::Value, String> {
    if namespace.is_empty() || podName.is_empty() {
        return Err("Namespace and pod name are required".to_string());
    }
    state
        .start_port_forward(namespace, podName, localPort, podPort)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn stop_port_forward(
    state: State<'_, K8sState>,
    namespace: String,
    #[allow(non_snake_case)] podName: String,
    #[allow(non_snake_case)] localPort: u16,
    #[allow(non_snake_case)] podPort: u16,
) -> std::result::Result<(), String> {
    state
        .stop_port_forward(namespace, podName, localPort, podPort)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn scale_deployment(
    state: State<'_, K8sState>,
    namespace: String,
    name: String,
    replicas: i32,
) -> std::result::Result<(), String> {
    if namespace.is_empty() || name.is_empty() {
        return Err("Namespace and deployment name are required".to_string());
    }
    if replicas < 0 {
        return Err("Replicas must be non-negative".to_string());
    }
    state
        .scale_deployment(namespace, name, replicas)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn restart_deployment(
    state: State<'_, K8sState>,
    namespace: String,
    name: String,
) -> std::result::Result<(), String> {
    if namespace.is_empty() || name.is_empty() {
        return Err("Namespace and deployment name are required".to_string());
    }
    state
        .restart_deployment(namespace, name)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn search_resources(
    state: State<'_, K8sState>,
    query: String,
    namespace: Option<String>,
) -> std::result::Result<Vec<serde_json::Value>, String> {
    if query.is_empty() {
        return Err("Search query cannot be empty".to_string());
    }
    state
        .search_resources(query, namespace)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn exec_into_pod(
    app: tauri::AppHandle,
    state: State<'_, K8sState>,
    namespace: String,
    #[allow(non_snake_case)] podName: String,
    container: Option<String>,
    shell: Option<String>,
) -> std::result::Result<String, String> {
    if namespace.is_empty() || podName.is_empty() {
        return Err("Namespace and pod name are required".to_string());
    }
    state
        .exec_into_pod(app, namespace, podName, container, shell)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn send_exec_input(
    state: State<'_, K8sState>,
    #[allow(non_snake_case)] sessionId: String,
    data: String,
) -> std::result::Result<(), String> {
    state
        .send_exec_input(sessionId, data)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn resize_exec(
    state: State<'_, K8sState>,
    #[allow(non_snake_case)] sessionId: String,
    cols: u16,
    rows: u16,
) -> std::result::Result<(), String> {
    state
        .resize_exec(sessionId, cols, rows)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn stop_exec(
    state: State<'_, K8sState>,
    #[allow(non_snake_case)] sessionId: String,
) -> std::result::Result<(), String> {
    state.stop_exec(sessionId).await.map_err(|e| e.to_string())
}

// ── Phase 1: YAML Editor ──────────────────────────────────────────────

#[tauri::command]
pub async fn get_resource_yaml(
    state: State<'_, K8sState>,
    kind: ResourceKind,
    namespace: String,
    name: String,
) -> std::result::Result<String, String> {
    state
        .get_resource_yaml(kind, namespace, name)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn apply_resource_yaml(
    state: State<'_, K8sState>,
    kind: ResourceKind,
    namespace: String,
    name: String,
    yaml: String,
) -> std::result::Result<String, String> {
    state
        .apply_resource_yaml(kind, namespace, name, yaml)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn diff_resource_yaml(
    state: State<'_, K8sState>,
    kind: ResourceKind,
    namespace: String,
    name: String,
    #[allow(non_snake_case)] newYaml: String,
) -> std::result::Result<Vec<crate::state::yaml::DiffLine>, String> {
    state
        .diff_resource_yaml(kind, namespace, name, newYaml)
        .await
        .map_err(|e| e.to_string())
}

// ── Phase 1: Deployment Rollback ──────────────────────────────────────

#[tauri::command]
pub async fn list_deployment_revisions(
    state: State<'_, K8sState>,
    namespace: String,
    name: String,
) -> std::result::Result<Vec<crate::state::rollback::DeploymentRevision>, String> {
    state
        .list_deployment_revisions(namespace, name)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_revision_yaml(
    state: State<'_, K8sState>,
    namespace: String,
    #[allow(non_snake_case)] rsName: String,
) -> std::result::Result<String, String> {
    state
        .get_revision_yaml(namespace, rsName)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn rollback_deployment(
    state: State<'_, K8sState>,
    namespace: String,
    #[allow(non_snake_case)] deploymentName: String,
    #[allow(non_snake_case)] rsName: String,
) -> std::result::Result<(), String> {
    state
        .rollback_deployment(namespace, deploymentName, rsName)
        .await
        .map_err(|e| e.to_string())
}

// ── Phase 2: Cluster Dashboard ────────────────────────────────────────

#[tauri::command]
pub async fn get_cluster_health(
    state: State<'_, K8sState>,
) -> std::result::Result<crate::state::dashboard::ClusterHealth, String> {
    state.get_cluster_health().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_cluster_health_multi_cluster(
    state: State<'_, K8sState>,
) -> std::result::Result<crate::state::dashboard::MultiClusterHealth, String> {
    state
        .get_cluster_health_multi_cluster()
        .await
        .map_err(|e| e.to_string())
}

// ── Phase 2: Event Store ──────────────────────────────────────────────

#[tauri::command]
pub async fn query_stored_events(
    state: State<'_, K8sState>,
    since: String,
    until: String,
    namespace: Option<String>,
) -> std::result::Result<Vec<crate::state::event_store::StoredEvent>, String> {
    let store = state
        .event_store
        .as_ref()
        .ok_or_else(|| "Event store not available".to_string())?;
    let ctx = state.current_context_name().await;
    store
        .query_events(&since, &until, namespace.as_deref(), ctx.as_deref())
        .await
        .map_err(|e| e.to_string())
}

// ── Phase 2: Multi-Pod Logs ───────────────────────────────────────────

#[tauri::command]
pub async fn stream_multi_pod_logs(
    app: tauri::AppHandle,
    state: State<'_, K8sState>,
    namespace: String,
    #[allow(non_snake_case)] labelSelector: String,
    #[allow(non_snake_case)] tailLines: Option<i64>,
) -> std::result::Result<(), String> {
    state
        .stream_multi_pod_logs(app, namespace, labelSelector, tailLines)
        .await
        .map_err(|e| e.to_string())
}

// ── Phase 3: CRD Browser ─────────────────────────────────────────────

#[tauri::command]
pub async fn list_crds(
    state: State<'_, K8sState>,
) -> std::result::Result<Vec<crate::state::crd::CrdInfo>, String> {
    state.list_crds().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_crd_instances(
    state: State<'_, K8sState>,
    group: String,
    version: String,
    plural: String,
    namespace: Option<String>,
) -> std::result::Result<Vec<serde_json::Value>, String> {
    state
        .list_crd_instances(group, version, plural, namespace)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_crd_instance(
    state: State<'_, K8sState>,
    group: String,
    version: String,
    plural: String,
    namespace: String,
    name: String,
) -> std::result::Result<serde_json::Value, String> {
    state
        .get_crd_instance(group, version, plural, namespace, name)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_crd_instance(
    state: State<'_, K8sState>,
    group: String,
    version: String,
    plural: String,
    namespace: String,
    name: String,
) -> std::result::Result<(), String> {
    state
        .delete_crd_instance(group, version, plural, namespace, name)
        .await
        .map_err(|e| e.to_string())
}

// ── Phase 3: Resource Graph ───────────────────────────────────────────

#[tauri::command]
pub async fn build_resource_graph(
    state: State<'_, K8sState>,
    namespace: Option<String>,
) -> std::result::Result<crate::state::graph::ResourceGraph, String> {
    state
        .build_resource_graph(namespace)
        .await
        .map_err(|e| e.to_string())
}

// ── Phase 4: Helm Management ──────────────────────────────────────────

#[tauri::command]
pub async fn helm_available(state: State<'_, K8sState>) -> std::result::Result<bool, String> {
    Ok(state.helm_available().await)
}

#[tauri::command]
pub async fn list_helm_releases(
    state: State<'_, K8sState>,
    namespace: Option<String>,
) -> std::result::Result<Vec<crate::state::helm::HelmRelease>, String> {
    state
        .list_helm_releases(namespace)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_helm_values(
    state: State<'_, K8sState>,
    release: String,
    namespace: String,
) -> std::result::Result<String, String> {
    state
        .get_helm_values(release, namespace)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_helm_manifest(
    state: State<'_, K8sState>,
    release: String,
    namespace: String,
) -> std::result::Result<String, String> {
    state
        .get_helm_manifest(release, namespace)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_helm_history(
    state: State<'_, K8sState>,
    release: String,
    namespace: String,
) -> std::result::Result<Vec<crate::state::helm::HelmRevision>, String> {
    state
        .get_helm_history(release, namespace)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn rollback_helm_release(
    state: State<'_, K8sState>,
    release: String,
    namespace: String,
    revision: String,
) -> std::result::Result<String, String> {
    state
        .rollback_helm_release(release, namespace, revision)
        .await
        .map_err(|e| e.to_string())
}

// ── Phase 4: Multi-Cluster ────────────────────────────────────────────

#[tauri::command]
pub async fn list_resources_multi_cluster(
    state: State<'_, K8sState>,
    kind: ResourceKind,
    namespace: Option<String>,
    #[allow(non_snake_case)] labelSelector: Option<String>,
) -> std::result::Result<Vec<serde_json::Value>, String> {
    state
        .list_resources_multi_cluster(kind, namespace, labelSelector)
        .await
        .map_err(|e| e.to_string())
}

// ── Phase 4: AI Troubleshooting ──────────────────────────────────────

#[tauri::command]
pub async fn ai_diagnose(
    state: State<'_, K8sState>,
    app: tauri::AppHandle,
    config: crate::state::ai::AIConfig,
    request: crate::state::ai::DiagnoseRequest,
) -> std::result::Result<(), String> {
    state
        .ai_diagnose(app, config, request)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ai_chat(
    state: State<'_, K8sState>,
    app: tauri::AppHandle,
    config: crate::state::ai::AIConfig,
    request: crate::state::ai::AIChatRequest,
) -> std::result::Result<(), String> {
    state
        .ai_chat(app, config, request)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ai_test_connection(
    config: crate::state::ai::AIConfig,
) -> std::result::Result<bool, String> {
    K8sState::ai_test_connection(&config)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_ollama_models(
    base_url: Option<String>,
) -> std::result::Result<Vec<String>, String> {
    K8sState::list_ollama_models(base_url.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn claude_cli_available() -> std::result::Result<bool, String> {
    K8sState::claude_cli_available()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_claude_models() -> std::result::Result<Vec<String>, String> {
    K8sState::list_claude_models()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cursor_agent_available() -> std::result::Result<bool, String> {
    K8sState::cursor_agent_cli_available()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_cursor_agent_models() -> std::result::Result<Vec<String>, String> {
    K8sState::list_cursor_agent_models()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn check_providers_availability() -> std::result::Result<HashMap<String, bool>, String> {
    let (claude, cursor, ollama) = tokio::join!(
        K8sState::claude_cli_available(),
        K8sState::cursor_agent_cli_available(),
        K8sState::ollama_available(None),
    );
    let mut map = HashMap::new();
    map.insert("claude_cli".to_string(), claude.unwrap_or(false));
    map.insert("cursor_agent".to_string(), cursor.unwrap_or(false));
    map.insert("ollama".to_string(), ollama.unwrap_or(false));
    Ok(map)
}

// ── Secure Key Storage ───────────────────────────────────────────────

#[tauri::command]
pub fn store_api_key(provider: String, key: String) -> std::result::Result<(), String> {
    let entry = keyring::Entry::new("kore", &provider).map_err(|e| e.to_string())?;
    entry.set_password(&key).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_api_key(provider: String) -> std::result::Result<Option<String>, String> {
    let entry = keyring::Entry::new("kore", &provider).map_err(|e| e.to_string())?;
    match entry.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub fn delete_api_key(provider: String) -> std::result::Result<(), String> {
    let entry = keyring::Entry::new("kore", &provider).map_err(|e| e.to_string())?;
    match entry.delete_password() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

// ── Debug Containers ─────────────────────────────────────────────────

#[tauri::command]
pub async fn add_debug_container(
    state: State<'_, K8sState>,
    namespace: String,
    #[allow(non_snake_case)] podName: String,
    image: String,
    #[allow(non_snake_case)] targetContainer: Option<String>,
    command: Option<Vec<String>>,
) -> std::result::Result<String, String> {
    state
        .add_debug_container(namespace, podName, image, targetContainer, command)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_debug_containers(
    state: State<'_, K8sState>,
    namespace: String,
    #[allow(non_snake_case)] podName: String,
) -> std::result::Result<Vec<serde_json::Value>, String> {
    state
        .list_debug_containers(namespace, podName)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn stop_debug_container(
    state: State<'_, K8sState>,
    namespace: String,
    #[allow(non_snake_case)] podName: String,
    #[allow(non_snake_case)] containerName: String,
) -> std::result::Result<(), String> {
    state
        .stop_debug_container(namespace, podName, containerName)
        .await
        .map_err(|e| e.to_string())
}

// ── Network Policy Visualization ─────────────────────────────────────

#[tauri::command]
pub async fn build_network_policy_graph(
    state: State<'_, K8sState>,
    namespace: Option<String>,
) -> std::result::Result<crate::state::network_policy::NetworkPolicyGraph, String> {
    state
        .build_network_policy_graph(namespace)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn simulate_network_traffic(
    state: State<'_, K8sState>,
    #[allow(non_snake_case)] sourceNamespace: String,
    #[allow(non_snake_case)] sourcePod: String,
    #[allow(non_snake_case)] destNamespace: String,
    #[allow(non_snake_case)] destPod: String,
    port: Option<i32>,
    protocol: Option<String>,
) -> std::result::Result<crate::state::network_policy::TrafficSimulationResult, String> {
    state
        .simulate_network_traffic(sourceNamespace, sourcePod, destNamespace, destPod, port, protocol)
        .await
        .map_err(|e| e.to_string())
}

// ── Favorites Persistence ────────────────────────────────────────────

fn favorites_path() -> std::result::Result<std::path::PathBuf, String> {
    let dir = dirs::home_dir()
        .ok_or_else(|| "Cannot determine home directory".to_string())?
        .join(".kore");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("favorites.json"))
}

fn read_all_favorites() -> std::result::Result<HashMap<String, Vec<String>>, String> {
    let path = favorites_path()?;
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let data = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&data).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn load_favorites(key: String) -> std::result::Result<Vec<String>, String> {
    let map = read_all_favorites()?;
    Ok(map.get(&key).cloned().unwrap_or_default())
}

#[tauri::command]
pub fn save_favorites(key: String, values: Vec<String>) -> std::result::Result<(), String> {
    let mut map = read_all_favorites()?;
    map.insert(key, values);
    let path = favorites_path()?;
    let data = serde_json::to_string_pretty(&map).map_err(|e| e.to_string())?;
    std::fs::write(&path, data).map_err(|e| e.to_string())
}
