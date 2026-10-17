use crate::error::{K8sError, Result};
use crate::events::{channel, ExecExit, ExecSession};
use crate::state::K8sState;
use futures::SinkExt;
use kube::api::{Api, AttachParams, TerminalSize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

/// Guard that removes an exec session from the map when dropped. The map uses
/// a std mutex so removal is synchronous and works from any thread.
struct SessionGuard {
    sessions: Arc<Mutex<HashMap<String, SessionEntry>>>,
    session_id: String,
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.remove(&self.session_id);
        }
    }
}

struct SessionEntry {
    cancel_token: CancellationToken,
    stdin_tx: mpsc::Sender<Vec<u8>>,
    resize_tx: mpsc::Sender<(u16, u16)>,
}

/// Manages exec sessions with cancellation.
#[derive(Clone)]
pub struct ExecManager {
    sessions: Arc<Mutex<HashMap<String, SessionEntry>>>,
}

impl ExecManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn cancel_all(&self) {
        if let Ok(mut sessions) = self.sessions.lock() {
            for (id, session) in sessions.drain() {
                session.cancel_token.cancel();
                info!(session_id = %id, "Cancelled exec session");
            }
        }
    }

    pub fn len(&self) -> usize {
        self.sessions.lock().map(|s| s.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for ExecManager {
    fn default() -> Self {
        Self::new()
    }
}

impl K8sState {
    /// Start an interactive TTY exec session. The returned [`ExecSession`]
    /// carries raw stdout bytes and a one-shot exit payload; dropping its
    /// `stdout` subscription ends the session (as does `stop_exec`).
    pub async fn exec_into_pod(
        &self,
        namespace: String,
        pod_name: String,
        container: Option<String>,
        shell: Option<String>,
    ) -> Result<ExecSession> {
        // Fetch the client first so a ClientMissing error doesn't leak a session.
        let client = self.current_client().await?;

        let session_id = format!("{}/{}/{}", namespace, pod_name, generate_session_id());
        let (stdout_sink, stdout_sub) = channel::<Vec<u8>>(&self.stream_root());
        let cancel_token = stdout_sink.token().clone();
        let (exit_tx, exit_rx) = futures::channel::oneshot::channel::<ExecExit>();
        let (stdin_tx, mut stdin_rx) = mpsc::channel::<Vec<u8>>(256);
        let (resize_tx, mut resize_rx) = mpsc::channel::<(u16, u16)>(16);

        if let Ok(mut sessions) = self.exec_sessions.sessions.lock() {
            sessions.insert(
                session_id.clone(),
                SessionEntry {
                    cancel_token: cancel_token.clone(),
                    stdin_tx,
                    resize_tx,
                },
            );
        }

        let sid = session_id.clone();
        let ns = namespace.clone();
        let pn = pod_name.clone();
        let shell_cmd = shell.unwrap_or_else(|| "/bin/sh".to_string());
        let exec_sessions = self.exec_sessions.clone();

        tokio::spawn(async move {
            let _guard = SessionGuard {
                sessions: exec_sessions.sessions.clone(),
                session_id: sid.clone(),
            };

            let api: Api<k8s_openapi::api::core::v1::Pod> = Api::namespaced(client, &ns);

            let mut ap = AttachParams::interactive_tty();
            if let Some(ref c) = container {
                ap = ap.container(c);
            }

            let exit = match api.exec(&pn, vec![&shell_cmd], &ap).await {
                Ok(mut attached) => {
                    let Some(mut stdout) = attached.stdout() else {
                        let _ =
                            exit_tx.send(ExecExit::error("No stdout available from exec session"));
                        return;
                    };
                    let Some(mut stdin) = attached.stdin() else {
                        let _ =
                            exit_tx.send(ExecExit::error("No stdin available from exec session"));
                        return;
                    };
                    let mut term_size = attached.terminal_size();

                    // Forward stdout to the subscriber
                    let ct1 = cancel_token.clone();
                    let out = stdout_sink.clone();
                    let mut stdout_task = tokio::spawn(async move {
                        let mut buf = vec![0u8; 4096];
                        loop {
                            tokio::select! {
                                _ = ct1.cancelled() => break,
                                result = stdout.read(&mut buf) => {
                                    match result {
                                        Ok(0) => break,
                                        Ok(n) => {
                                            if out.send(buf[..n].to_vec()).is_err() {
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

                    // Forward stdin and resize requests (Deliberate Fix 7: resize
                    // is sent to the API server as a TerminalSize message).
                    let ct2 = cancel_token.clone();
                    let mut input_task = tokio::spawn(async move {
                        loop {
                            tokio::select! {
                                _ = ct2.cancelled() => break,
                                data = stdin_rx.recv() => {
                                    match data {
                                        Some(bytes) => {
                                            if stdin.write_all(&bytes).await.is_err() {
                                                break;
                                            }
                                            let _ = stdin.flush().await;
                                        }
                                        None => break,
                                    }
                                }
                                size = resize_rx.recv() => {
                                    if let Some((cols, rows)) = size {
                                        if let Some(tx) = term_size.as_mut() {
                                            if let Err(e) = tx.send(TerminalSize { width: cols, height: rows }).await {
                                                warn!(error = %e, "Failed to send terminal size");
                                                term_size = None;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    });

                    // Wait for completion
                    tokio::select! {
                        _ = cancel_token.cancelled() => {},
                        _ = &mut stdout_task => {},
                        _ = &mut input_task => {},
                    }
                    cancel_token.cancel();
                    stdout_task.abort();
                    input_task.abort();
                    attached.abort();
                    ExecExit::completed()
                }
                Err(err) => {
                    error!(error = %err, "Failed to exec into pod");
                    ExecExit::error(err.to_string())
                }
            };
            let _ = exit_tx.send(exit);
            drop(stdout_sink);
            // Session cleanup is handled by SessionGuard on drop
        });

        Ok(ExecSession {
            session_id,
            stdout: stdout_sub,
            exit: exit_rx,
        })
    }

    fn exec_entry<R>(&self, session_id: &str, f: impl FnOnce(&SessionEntry) -> R) -> Result<R> {
        let sessions = self
            .exec_sessions
            .sessions
            .lock()
            .map_err(|_| K8sError::other("exec session map poisoned"))?;
        match sessions.get(session_id) {
            Some(s) => Ok(f(s)),
            None => Err(K8sError::Validation(format!(
                "Exec session {session_id} not found"
            ))),
        }
    }

    pub async fn send_exec_input(&self, session_id: String, data: String) -> Result<()> {
        self.send_exec_bytes(session_id, data.into_bytes()).await
    }

    /// Raw-bytes variant of `send_exec_input` (terminal input need not be UTF-8).
    pub async fn send_exec_bytes(&self, session_id: String, data: Vec<u8>) -> Result<()> {
        let tx = self.exec_entry(&session_id, |s| s.stdin_tx.clone())?;
        tx.send(data)
            .await
            .map_err(|_| K8sError::Validation("Exec session stdin closed".to_string()))
    }

    pub async fn resize_exec(&self, session_id: String, cols: u16, rows: u16) -> Result<()> {
        let tx = self.exec_entry(&session_id, |s| s.resize_tx.clone())?;
        let _ = tx.send((cols, rows)).await;
        Ok(())
    }

    pub async fn stop_exec(&self, session_id: String) -> Result<()> {
        let removed = self
            .exec_sessions
            .sessions
            .lock()
            .map_err(|_| K8sError::other("exec session map poisoned"))?
            .remove(&session_id);
        if let Some(session) = removed {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_id_is_uuid_v4_shaped() {
        let id = generate_session_id();
        let parts: Vec<&str> = id.split('-').collect();
        assert_eq!(parts.len(), 5);
        assert_eq!(parts[2].chars().next(), Some('4'));
        assert_eq!(id.len(), 36);
    }
}
