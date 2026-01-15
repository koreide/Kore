#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod error;
mod kube_state;

use commands::*;
use kube_state::K8sState;
use tauri::Manager;
use anyhow::anyhow;

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let state = tauri::async_runtime::block_on(K8sState::new())
                .map_err(|e| anyhow!(e.to_string()))?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_contexts,
            list_namespaces,
            switch_context,
            list_resources,
            start_watch,
            fetch_logs,
            delete_resource,
            describe_pod,
            start_pod_logs_stream,
            get_pod_metrics,
            start_port_forward,
            stop_port_forward
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

