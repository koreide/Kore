use crate::error::{K8sError, Result};
use crate::state::K8sState;
use k8s_openapi::api::core::v1::Pod;
use kube::api::{Api, AttachParams, Patch, PatchParams};
use serde_json::json;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

impl K8sState {
    /// Inject an ephemeral debug container into a running pod and wait for it to start.
    pub async fn add_debug_container(
        &self,
        namespace: String,
        pod_name: String,
        image: String,
        target_container: Option<String>,
        command: Option<Vec<String>>,
    ) -> Result<String> {
        let client = self.current_client().await?;
        let api: Api<Pod> = Api::namespaced(client, &namespace);

        // Generate unique debug container name
        let id = &Uuid::new_v4().to_string()[..8];
        let debug_name = format!("debugger-{id}");

        // Build strategic merge patch JSON
        let mut container = json!({
            "name": debug_name,
            "image": image,
            "stdin": true,
            "tty": true,
        });
        if let Some(target) = &target_container {
            container["targetContainerName"] = json!(target);
        }
        if let Some(cmd) = &command {
            container["command"] = json!(cmd);
        }

        let patch = json!({
            "spec": {
                "ephemeralContainers": [container]
            }
        });

        // Patch the ephemeralcontainers subresource
        let pp = PatchParams::default();
        api.patch_subresource("ephemeralcontainers", &pod_name, &pp, &Patch::Strategic(patch))
            .await
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("404") || msg.contains("not found") {
                    K8sError::Validation(
                        "Ephemeral containers not supported by this cluster (requires K8s 1.25+)"
                            .into(),
                    )
                } else if msg.contains("403") || msg.contains("forbidden") {
                    K8sError::Validation(
                        "RBAC: insufficient permissions to create ephemeral containers".into(),
                    )
                } else if msg.contains("static pods do not support ephemeral containers") {
                    K8sError::Validation(
                        "Static pods do not support ephemeral containers. Try a non-static pod instead.".into(),
                    )
                } else if msg.contains("422") || msg.contains("Invalid") {
                    K8sError::Validation(
                        format!("Pod does not support ephemeral containers: {msg}"),
                    )
                } else {
                    K8sError::Kube(e)
                }
            })?;

        // Poll until container is Running (max 30s, 1s interval)
        for _ in 0..30 {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let pod = api.get(&pod_name).await.map_err(K8sError::Kube)?;
            if let Some(status) = &pod.status {
                if let Some(ecs) = &status.ephemeral_container_statuses {
                    for cs in ecs {
                        if cs.name == debug_name {
                            if cs.state.as_ref().is_some_and(|s| s.running.is_some()) {
                                return Ok(debug_name);
                            }
                            // Check for terminal failure (image pull error, etc.)
                            if let Some(waiting) =
                                cs.state.as_ref().and_then(|s| s.waiting.as_ref())
                            {
                                let reason = waiting.reason.as_deref().unwrap_or("");
                                if reason == "ErrImagePull"
                                    || reason == "ImagePullBackOff"
                                    || reason == "InvalidImageName"
                                {
                                    return Err(K8sError::Validation(format!(
                                        "Debug container failed: {}: {}",
                                        reason,
                                        waiting.message.as_deref().unwrap_or("")
                                    )));
                                }
                            }
                            if cs.state.as_ref().is_some_and(|s| s.terminated.is_some()) {
                                return Err(K8sError::Validation(
                                    "Debug container terminated unexpectedly".into(),
                                ));
                            }
                        }
                    }
                }
            }
        }

        Err(K8sError::Validation(
            "Timed out waiting for debug container to start (30s)".into(),
        ))
    }

    /// List existing ephemeral containers on a pod with their statuses.
    pub async fn list_debug_containers(
        &self,
        namespace: String,
        pod_name: String,
    ) -> Result<Vec<serde_json::Value>> {
        let client = self.current_client().await?;
        let api: Api<Pod> = Api::namespaced(client, &namespace);
        let pod = api.get(&pod_name).await.map_err(K8sError::Kube)?;

        let mut result = vec![];
        if let Some(spec) = &pod.spec {
            if let Some(ecs) = &spec.ephemeral_containers {
                for ec in ecs {
                    let status = pod
                        .status
                        .as_ref()
                        .and_then(|s| s.ephemeral_container_statuses.as_ref())
                        .and_then(|statuses| statuses.iter().find(|s| s.name == ec.name));
                    result.push(json!({
                        "name": ec.name,
                        "image": ec.image,
                        "targetContainer": ec.target_container_name,
                        "running": status.is_some_and(|s| {
                            s.state.as_ref().is_some_and(|st| st.running.is_some())
                        }),
                    }));
                }
            }
        }
        Ok(result)
    }

    /// Stop a debug container by attaching to its stdin and writing `exit`.
    ///
    /// The debug container runs `/bin/sh` as PID 1 with stdin + tty.  Unlike
    /// `exec` (which spawns a *new* process), `attach` connects to PID 1's
    /// own stdin.  Writing `exit\n` causes the interactive shell to terminate,
    /// which terminates the container.
    ///
    /// This avoids the kernel's PID-1 signal protection entirely — no signals
    /// are involved; PID 1 simply reads a command from its stdin and exits.
    pub async fn stop_debug_container(
        &self,
        namespace: String,
        pod_name: String,
        container_name: String,
    ) -> Result<()> {
        let client = self.current_client().await?;
        let api: Api<Pod> = Api::namespaced(client, &namespace);

        let ap = AttachParams {
            container: Some(container_name.clone()),
            stdin: true,
            stdout: false,
            stderr: false,
            tty: true,
            ..Default::default()
        };

        let mut attached = api.attach(&pod_name, &ap).await.map_err(|e| {
            K8sError::Validation(format!(
                "Failed to attach to debug container '{container_name}': {e}"
            ))
        })?;

        // Write "exit\n" to PID 1's stdin.  The leading \n flushes any
        // partial input that may be sitting on the line.
        if let Some(mut stdin) = attached.stdin() {
            let _ = stdin.write_all(b"\nexit\n").await;
            let _ = stdin.shutdown().await;
        }

        // Poll until the container is no longer Running (max 10 s).
        for _ in 0..10 {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let pod = api.get(&pod_name).await.map_err(K8sError::Kube)?;
            let still_running = pod
                .status
                .as_ref()
                .and_then(|s| s.ephemeral_container_statuses.as_ref())
                .and_then(|statuses| statuses.iter().find(|s| s.name == container_name))
                .is_some_and(|cs| {
                    cs.state.as_ref().is_some_and(|s| s.running.is_some())
                });
            if !still_running {
                return Ok(());
            }
        }

        Err(K8sError::Validation(
            "Timed out waiting for debug container to stop (10s)".into(),
        ))
    }
}
