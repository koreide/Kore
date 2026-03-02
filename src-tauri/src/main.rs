#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod constants;
mod error;
mod state;

use commands::*;
use state::K8sState;
use tauri::Manager;
use tracing::error;

/// Enrich PATH so exec-based auth providers (aws, gcloud, az, etc.) work
/// even when launched from Finder/Spotlight (which gets a minimal PATH).
fn enrich_path() {
    let extra_dirs = [
        "/usr/local/bin",
        "/opt/homebrew/bin",
        "/opt/homebrew/sbin",
        "/usr/local/sbin",
        "/usr/local/aws-cli",
        "/Library/Frameworks/Python.framework/Versions/Current/bin",
    ];
    let current = std::env::var("PATH").unwrap_or_default();
    let mut paths: Vec<String> = current.split(':').map(|s| s.to_string()).collect();
    for dir in &extra_dirs {
        if !paths.iter().any(|p| p == dir) && std::path::Path::new(dir).is_dir() {
            paths.push(dir.to_string());
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let shell_paths = [format!("{home}/.local/bin"), format!("{home}/bin")];
        for dir in &shell_paths {
            if !paths.iter().any(|p| p == dir) && std::path::Path::new(dir).is_dir() {
                paths.push(dir.clone());
            }
        }
    }
    std::env::set_var("PATH", paths.join(":"));
}

fn main() {
    // Enrich PATH before anything else so exec-based kubeconfig auth works
    enrich_path();

    // Initialize tracing subscriber
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("kore=info,warn")),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let state = tauri::async_runtime::block_on(K8sState::new());
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Connection status
            get_connection_status,
            retry_connection,
            // Core
            list_contexts,
            list_namespaces,
            switch_context,
            list_resources,
            start_watch,
            fetch_logs,
            delete_resource,
            describe_pod,
            describe_resource,
            list_events_for_resource,
            start_pod_logs_stream,
            stop_pod_logs_stream,
            get_pod_metrics,
            start_port_forward,
            stop_port_forward,
            scale_deployment,
            restart_deployment,
            search_resources,
            exec_into_pod,
            send_exec_input,
            resize_exec,
            stop_exec,
            // Phase 1: YAML Editor + Rollback
            get_resource_yaml,
            apply_resource_yaml,
            diff_resource_yaml,
            list_deployment_revisions,
            get_revision_yaml,
            rollback_deployment,
            // Phase 2: Dashboard + Events + Multi-Pod Logs
            get_cluster_health,
            get_cluster_health_multi_cluster,
            query_stored_events,
            stream_multi_pod_logs,
            // Phase 3: CRD + Graph
            list_crds,
            list_crd_instances,
            get_crd_instance,
            delete_crd_instance,
            build_resource_graph,
            // Phase 4: Helm + Multi-Cluster
            helm_available,
            list_helm_releases,
            get_helm_values,
            get_helm_manifest,
            get_helm_history,
            rollback_helm_release,
            list_resources_multi_cluster,
            // Phase 4: AI
            ai_diagnose,
            ai_chat,
            ai_test_connection,
            list_ollama_models,
            claude_cli_available,
            list_claude_models,
            cursor_agent_available,
            list_cursor_agent_models,
            check_providers_availability,
            // Debug containers
            add_debug_container,
            list_debug_containers,
            stop_debug_container,
            // Secure key storage
            store_api_key,
            get_api_key,
            delete_api_key,
            // Favorites persistence
            load_favorites,
            save_favorites,
        ])
        .build(tauri::generate_context!())
        .unwrap_or_else(|e| {
            error!("Failed to build Tauri application: {}", e);
            std::process::exit(1);
        })
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                if let Some(state) = app.try_state::<K8sState>() {
                    tauri::async_runtime::block_on(state.shutdown());
                }
            }
        });
}
