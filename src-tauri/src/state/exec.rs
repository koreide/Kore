use crate::error::{K8sError, Result};
use crate::state::K8sState;
use kube::api::{Api, AttachParams};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

/// Guard that removes an exec session from the map when dropped.
struct SessionGuard {
    sessions: Arc<RwLock<HashMap<String, ExecSession>>>,
    session_id: String,
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        // Use blocking to handle async cleanup in Drop
        let sessions = self.sessions.clone();
        let sid = self.session_id.clone();
        // Spawn a task to do async cleanup since Drop can't be async
        tauri::async_runtime::spawn(async move {
            let mut sessions = sessions.write().await;
            sessions.remove(&sid);
        });
    }
}

struct ExecSession {
    cancel_token: CancellationToken,
    stdin_tx: mpsc::Sender<Vec<u8>>,
    resize_tx: mpsc::Sender<(u16, u16)>,
}

/// Manages exec sessions with cancellation.
#[derive(Clone)]
pub struct ExecManager {
    sessions: Arc<RwLock<HashMap<String, ExecSession>>>,
}

impl ExecManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn cancel_all(&self) {
        let mut sessions = self.sessions.write().await;
        for (id, session) in sessions.drain() {
            session.cancel_token.cancel();
            info!(session_id = %id, "Cancelled exec session");
        }
    }
}

impl K8sState {
    pub async fn exec_into_pod(
        &self,
        app: AppHandle,
        namespace: String,
        pod_name: String,
        container: Option<String>,
        shell: Option<String>,
    ) -> Result<String> {
        let session_id = format!("{}/{}/{}", namespace, pod_name, generate_session_id());
        let cancel_token = CancellationToken::new();
        let (stdin_tx, mut stdin_rx) = mpsc::channel::<Vec<u8>>(256);
        let (resize_tx, mut resize_rx) = mpsc::channel::<(u16, u16)>(16);

        // Store session
        {
            let mut sessions = self.exec_sessions.sessions.write().await;
            sessions.insert(
                session_id.clone(),
                ExecSession {
                    cancel_token: cancel_token.clone(),
                    stdin_tx: stdin_tx.clone(),
                    resize_tx: resize_tx.clone(),
                },
            );
        }

        let client = self.current_client().await?;
        let handle = app.clone();
        let sid = session_id.clone();
        let ns = namespace.clone();
        let pn = pod_name.clone();
        let shell_cmd = shell.unwrap_or_else(|| "/bin/sh".to_string());
        let exec_sessions = self.exec_sessions.clone();

        tauri::async_runtime::spawn(async move {
            let _guard = SessionGuard {
                sessions: exec_sessions.sessions.clone(),
                session_id: sid.clone(),
            };

            let api: Api<k8s_openapi::api::core::v1::Pod> = Api::namespaced(client, &ns);

            let mut ap = AttachParams::interactive_tty();
            if let Some(ref c) = container {
                ap = ap.container(c);
            }

            let event_stdout = format!("exec-stdout://{sid}");
            let event_exit = format!("exec-exit://{sid}");

            match api.exec(&pn, vec![&shell_cmd], &ap).await {
                Ok(mut attached) => {
                    let mut stdout = match attached.stdout() {
                        Some(s) => s,
                        None => {
                            let _ = handle.emit(
                                &event_exit,
                                &json!({ "error": "No stdout available from exec session" }),
                            );
                            return;
                        }
                    };
                    let mut stdin = match attached.stdin() {
                        Some(s) => s,
                        None => {
                            let _ = handle.emit(
                                &event_exit,
                                &json!({ "error": "No stdin available from exec session" }),
                            );
                            return;
                        }
                    };

                    // Forward stdout to frontend
                    let ct1 = cancel_token.clone();
                    let handle1 = handle.clone();
                    let event_stdout1 = event_stdout.clone();
                    let stdout_task = tauri::async_runtime::spawn(async move {
                        let mut buf = vec![0u8; 4096];
                        loop {
                            tokio::select! {
                                _ = ct1.cancelled() => break,
                                result = stdout.read(&mut buf) => {
                                    match result {
                                        Ok(0) => break,
                                        Ok(n) => {
                                            use base64::Engine;
                                            let encoded = base64::engine::general_purpose::STANDARD.encode(&buf[..n]);
                                            if let Err(e) = handle1.emit(&event_stdout1, &json!({ "data": encoded })) {
                                                error!(error = %e, "Failed to emit exec stdout");
                                                break;
                                            }
                                        }
                                        Err(e) => {
                                            warn!(error = %e, "Exec stdout error");
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    });

                    // Forward stdin from frontend
                    let ct2 = cancel_token.clone();
                    let stdin_task = tauri::async_runtime::spawn(async move {
                        loop {
                            tokio::select! {
                                _ = ct2.cancelled() => break,
                                data = stdin_rx.recv() => {
                                    match data {
                                        Some(bytes) => {
                                            if stdin.write_all(&bytes).await.is_err() {
                                                break;
                                            }
                                        }
                                        None => break,
                                    }
                                }
                            }
                        }
                    });

                    // Handle resize events (best-effort)
                    let ct3 = cancel_token.clone();
                    let _resize_task = tauri::async_runtime::spawn(async move {
                        loop {
                            tokio::select! {
                                _ = ct3.cancelled() => break,
                                size = resize_rx.recv() => {
                                    match size {
                                        Some((_cols, _rows)) => {
                                            // Terminal resize — kube-rs doesn't directly support resize
                                            // via the exec API without raw websocket manipulation.
                                        }
                                        None => break,
                                    }
                                }
                            }
                        }
                    });

                    // Wait for completion
                    tokio::select! {
                        _ = cancel_token.cancelled() => {},
                        _ = stdout_task => {},
                        _ = stdin_task => {},
                    }

                    // Notify exit
                    let _ = handle.emit(&event_exit, &json!({ "reason": "completed" }));
                }
                Err(err) => {
                    error!(error = %err, "Failed to exec into pod");
                    let _ = handle.emit(&event_exit, &json!({ "error": err.to_string() }));
                }
            }

            // Session cleanup is handled by SessionGuard on drop
        });

        Ok(session_id)
    }

    pub async fn send_exec_input(&self, session_id: String, data: String) -> Result<()> {
        let sessions = self.exec_sessions.sessions.read().await;
        if let Some(session) = sessions.get(&session_id) {
            session
                .stdin_tx
                .send(data.into_bytes())
                .await
                .map_err(|_| K8sError::Validation("Exec session stdin closed".to_string()))?;
            Ok(())
        } else {
            Err(K8sError::Validation(format!(
                "Exec session {session_id} not found"
            )))
        }
    }

    pub async fn resize_exec(&self, session_id: String, cols: u16, rows: u16) -> Result<()> {
        let sessions = self.exec_sessions.sessions.read().await;
        if let Some(session) = sessions.get(&session_id) {
            let _ = session.resize_tx.send((cols, rows)).await;
            Ok(())
        } else {
            Err(K8sError::Validation(format!(
                "Exec session {session_id} not found"
            )))
        }
    }

    pub async fn stop_exec(&self, session_id: String) -> Result<()> {
        let mut sessions = self.exec_sessions.sessions.write().await;
        if let Some(session) = sessions.remove(&session_id) {
            session.cancel_token.cancel();
            info!(session_id = %session_id, "Stopped exec session");
            Ok(())
        } else {
            Err(K8sError::Validation(format!(
                "Exec session {session_id} not found"
            )))
        }
    }
}

/// Generate a random session ID. Uses thread_rng (not cryptographic)
/// which is sufficient for internal session identification.
fn generate_session_id() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 16] = rng.gen();
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        u16::from_be_bytes([bytes[4], bytes[5]]),
        (u16::from_be_bytes([bytes[6], bytes[7]]) & 0x0fff) | 0x4000,
        (u16::from_be_bytes([bytes[8], bytes[9]]) & 0x3fff) | 0x8000,
        u64::from_be_bytes([
            0, 0, bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
        ])
    )
}
