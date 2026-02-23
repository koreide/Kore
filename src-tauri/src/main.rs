#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod constants;
mod error;
mod state;

use commands::*;
use state::K8sState;
use tauri::Manager;
use tracing::error;

fn main() {
    // Initialize tracing subscriber
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("kore=info,warn")),
        )
        .init();

    tauri::Builder::default()
        .setup(|app| {
            let state = tauri::async_runtime::block_on(K8sState::new())
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
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
            ai_test_connection,
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
