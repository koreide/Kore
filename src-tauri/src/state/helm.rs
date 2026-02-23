use crate::error::{K8sError, Result};
use crate::state::K8sState;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelmRelease {
    pub name: String,
    pub namespace: String,
    pub revision: String,
    pub updated: String,
    pub status: String,
    pub chart: String,
    pub app_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelmRevision {
    pub revision: String,
    pub updated: String,
    pub status: String,
    pub chart: String,
    pub app_version: String,
    pub description: String,
}

impl K8sState {
    /// Check if helm is available in PATH.
    pub async fn helm_available(&self) -> bool {
        Command::new("helm")
            .arg("version")
            .arg("--short")
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// List all helm releases (optionally in a namespace).
    pub async fn list_helm_releases(
        &self,
        namespace: Option<String>,
    ) -> Result<Vec<HelmRelease>> {
        let mut cmd = Command::new("helm");
        cmd.arg("list").arg("--output").arg("json");

        if let Some(ns) = namespace {
            cmd.arg("--namespace").arg(ns);
        } else {
            cmd.arg("--all-namespaces");
        }

        // Add kubecontext if set
        let inner = self.inner.read().await;
        if let Some(ctx) = &inner.current_context {
            cmd.arg("--kube-context").arg(ctx);
        }
        drop(inner);

        let output = cmd.output().await.map_err(K8sError::Io)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(K8sError::Validation(format!("helm list failed: {stderr}")));
        }

        let releases: Vec<HelmRelease> =
            serde_json::from_slice(&output.stdout).map_err(K8sError::Serde)?;

        Ok(releases)
    }

    /// Get helm release values.
    pub async fn get_helm_values(
        &self,
        release: String,
        namespace: String,
    ) -> Result<String> {
        let mut cmd = Command::new("helm");
        cmd.arg("get")
            .arg("values")
            .arg(&release)
            .arg("--namespace")
            .arg(&namespace)
            .arg("--all");

        let inner = self.inner.read().await;
        if let Some(ctx) = &inner.current_context {
            cmd.arg("--kube-context").arg(ctx);
        }
        drop(inner);

        let output = cmd.output().await.map_err(K8sError::Io)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(K8sError::Validation(format!(
                "helm get values failed: {stderr}"
            )));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Get helm release manifest.
    pub async fn get_helm_manifest(
        &self,
        release: String,
        namespace: String,
    ) -> Result<String> {
        let mut cmd = Command::new("helm");
        cmd.arg("get")
            .arg("manifest")
            .arg(&release)
            .arg("--namespace")
            .arg(&namespace);

        let inner = self.inner.read().await;
        if let Some(ctx) = &inner.current_context {
            cmd.arg("--kube-context").arg(ctx);
        }
        drop(inner);

        let output = cmd.output().await.map_err(K8sError::Io)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(K8sError::Validation(format!(
                "helm get manifest failed: {stderr}"
            )));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Get helm release history.
    pub async fn get_helm_history(
        &self,
        release: String,
        namespace: String,
    ) -> Result<Vec<HelmRevision>> {
        let mut cmd = Command::new("helm");
        cmd.arg("history")
            .arg(&release)
            .arg("--namespace")
            .arg(&namespace)
            .arg("--output")
            .arg("json");

        let inner = self.inner.read().await;
        if let Some(ctx) = &inner.current_context {
            cmd.arg("--kube-context").arg(ctx);
        }
        drop(inner);

        let output = cmd.output().await.map_err(K8sError::Io)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(K8sError::Validation(format!(
                "helm history failed: {stderr}"
            )));
        }

        let revisions: Vec<HelmRevision> =
            serde_json::from_slice(&output.stdout).map_err(K8sError::Serde)?;

        Ok(revisions)
    }

    /// Rollback a helm release to a specific revision.
    pub async fn rollback_helm_release(
        &self,
        release: String,
        namespace: String,
        revision: String,
    ) -> Result<String> {
        let mut cmd = Command::new("helm");
        cmd.arg("rollback")
            .arg(&release)
            .arg(&revision)
            .arg("--namespace")
            .arg(&namespace);

        let inner = self.inner.read().await;
        if let Some(ctx) = &inner.current_context {
            cmd.arg("--kube-context").arg(ctx);
        }
        drop(inner);

        let output = cmd.output().await.map_err(K8sError::Io)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(K8sError::Validation(format!(
                "helm rollback failed: {stderr}"
            )));
        }

        info!(release = %release, revision = %revision, "Rolled back helm release");
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}
