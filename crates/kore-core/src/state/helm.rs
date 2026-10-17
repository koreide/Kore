use crate::error::{K8sError, Result};
use crate::state::K8sState;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

/// Accept a JSON string or integer and keep it as a string (Deliberate Fix 8:
/// `helm history -o json` emits `revision` as an integer).
fn string_or_int<'de, D>(d: D) -> std::result::Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = serde_json::Value::deserialize(d)?;
    match v {
        serde_json::Value::String(s) => Ok(s),
        serde_json::Value::Number(n) => Ok(n.to_string()),
        serde_json::Value::Null => Ok(String::new()),
        other => Err(serde::de::Error::custom(format!(
            "expected string or integer revision, got {other}"
        ))),
    }
}

/// Parses a Helm timestamp. `helm history -o json` emits RFC 3339
/// (`2026-10-16T19:02:42.602509+03:00`), while `helm list -o json` emits Go's
/// `time.Time` string (`2026-10-16 19:02:43.123456 +0300 IDT`, possibly with a
/// trailing monotonic `m=+0.1` reading). Both are accepted.
pub fn parse_helm_time(raw: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(d) = chrono::DateTime::parse_from_rfc3339(raw) {
        return Some(d);
    }
    // Go format: "<date> <time> <±hhmm> [<zone abbreviation>] [m=±…]".
    let mut parts = raw.split_whitespace();
    let (date, time, offset) = (parts.next()?, parts.next()?, parts.next()?);
    let joined = format!("{date} {time} {offset}");
    chrono::DateTime::parse_from_str(&joined, "%Y-%m-%d %H:%M:%S%.f %z").ok()
}

/// Strips the `NOTES:` header `helm get notes` prints before the rendered notes.
pub fn clean_helm_notes(raw: &str) -> String {
    let t = raw.trim_start_matches(['\r', '\n']);
    let t = t.strip_prefix("NOTES:").unwrap_or(t);
    t.trim_start_matches(['\r', '\n']).trim_end().to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelmRelease {
    pub name: String,
    pub namespace: String,
    #[serde(deserialize_with = "string_or_int")]
    pub revision: String,
    pub updated: String,
    pub status: String,
    pub chart: String,
    pub app_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelmRevision {
    #[serde(deserialize_with = "string_or_int")]
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
        crate::tools::command("helm")
            .arg("version")
            .arg("--short")
            .output()
            .await
            .map(|o| o.status.success())
            .map_err(|e| {
                warn!(error = %e, "Failed to check helm availability");
                e
            })
            .unwrap_or(false)
    }

    /// List all helm releases (optionally in a namespace).
    pub async fn list_helm_releases(&self, namespace: Option<String>) -> Result<Vec<HelmRelease>> {
        let mut cmd = crate::tools::command("helm");
        self.apply_kubeconfig_env(&mut cmd);
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
    pub async fn get_helm_values(&self, release: String, namespace: String) -> Result<String> {
        let mut cmd = crate::tools::command("helm");
        self.apply_kubeconfig_env(&mut cmd);
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

    /// Get the rendered NOTES.txt of a release (`helm get notes`). A release
    /// without notes yields an empty string.
    pub async fn get_helm_notes(&self, release: String, namespace: String) -> Result<String> {
        let mut cmd = crate::tools::command("helm");
        self.apply_kubeconfig_env(&mut cmd);
        cmd.arg("get")
            .arg("notes")
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
                "helm get notes failed: {stderr}"
            )));
        }

        Ok(clean_helm_notes(&String::from_utf8_lossy(&output.stdout)))
    }

    /// Get helm release manifest.
    pub async fn get_helm_manifest(&self, release: String, namespace: String) -> Result<String> {
        let mut cmd = crate::tools::command("helm");
        self.apply_kubeconfig_env(&mut cmd);
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
        let mut cmd = crate::tools::command("helm");
        self.apply_kubeconfig_env(&mut cmd);
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
        let mut cmd = crate::tools::command("helm");
        self.apply_kubeconfig_env(&mut cmd);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helm_history_accepts_integer_revision() {
        let json = r#"[{"revision":1,"updated":"2026-10-15T10:00:00Z","status":"superseded","chart":"kore-demo-0.1.0","app_version":"1.0","description":"Install complete"},
                       {"revision":"2","updated":"2026-10-15T10:01:00Z","status":"deployed","chart":"kore-demo-0.1.0","app_version":"1.0","description":"Upgrade complete"}]"#;
        let revs: Vec<HelmRevision> = serde_json::from_str(json).unwrap();
        assert_eq!(revs[0].revision, "1");
        assert_eq!(revs[1].revision, "2");
    }

    #[test]
    fn parses_helm_list_go_timestamp() {
        let d = parse_helm_time("2026-10-16 19:02:43.123456 +0300 IDT").unwrap();
        assert_eq!(d.to_rfc3339(), "2026-10-16T19:02:43.123456+03:00");
        let d = parse_helm_time("2026-10-15 10:00:00.123456789 +0000 UTC").unwrap();
        assert_eq!(d.timestamp(), 1_792_058_400);
        // No fractional seconds, monotonic clock suffix.
        let d = parse_helm_time("2026-10-15 10:00:00 -0700 PDT m=+0.123").unwrap();
        assert_eq!(d.to_rfc3339(), "2026-10-15T10:00:00-07:00");
    }

    #[test]
    fn parses_helm_history_rfc3339() {
        let d = parse_helm_time("2026-10-16T19:02:42.602509+03:00").unwrap();
        assert_eq!(d.offset().local_minus_utc(), 3 * 3600);
        assert!(parse_helm_time("").is_none());
        assert!(parse_helm_time("not a date").is_none());
    }

    #[test]
    fn notes_header_is_stripped() {
        assert_eq!(clean_helm_notes("NOTES:\nThanks for installing x.\nline two\n"), "Thanks for installing x.\nline two");
        assert_eq!(clean_helm_notes(""), "");
        assert_eq!(clean_helm_notes("\n"), "");
        assert_eq!(clean_helm_notes("plain"), "plain");
    }

    #[test]
    fn helm_list_accepts_string_revision() {
        let json = r#"[{"name":"kore-demo","namespace":"kore-demo","revision":"2","updated":"x","status":"deployed","chart":"kore-demo-0.1.0","app_version":"1.0"}]"#;
        let rels: Vec<HelmRelease> = serde_json::from_str(json).unwrap();
        assert_eq!(rels[0].revision, "2");
    }
}
