use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum K8sError {
    #[error("Kubeconfig not found or unreadable")]
    Kubeconfig(#[from] kube::config::KubeconfigError),
    #[error("Client not initialized")]
    ClientMissing,
    #[error("Kubernetes error: {0}")]
    Kube(#[from] kube::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("Validation error: {0}")]
    Validation(String),
    #[error("Port forward error: {0}")]
    PortForward(String),
    #[error("Operation cancelled")]
    #[allow(dead_code)]
    Cancelled,
}

pub type Result<T> = std::result::Result<T, K8sError>;

#[derive(Debug, Clone, Serialize)]
pub struct ConnectionStatus {
    pub connected: bool,
    pub error: Option<String>,
    pub error_kind: Option<String>,
    pub kubeconfig_path: Option<String>,
    pub contexts_available: Vec<String>,
    pub current_context: Option<String>,
}

/// Classify an error string into a category for the frontend.
pub fn classify_connection_error(err: &K8sError) -> &'static str {
    match err {
        K8sError::Kubeconfig(_) => "no_kubeconfig",
        K8sError::Kube(e) => {
            let msg = e.to_string().to_lowercase();
            if msg.contains("unauthorized")
                || msg.contains("forbidden")
                || msg.contains("401")
                || msg.contains("403")
            {
                "auth_failed"
            } else {
                "cluster_unreachable"
            }
        }
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = K8sError::ClientMissing;
        assert_eq!(err.to_string(), "Client not initialized");

        let err = K8sError::Validation("name is empty".to_string());
        assert_eq!(err.to_string(), "Validation error: name is empty");

        let err = K8sError::PortForward("port in use".to_string());
        assert_eq!(err.to_string(), "Port forward error: port in use");

        let err = K8sError::Cancelled;
        assert_eq!(err.to_string(), "Operation cancelled");
    }

    #[test]
    fn test_classify_validation_error() {
        let err = K8sError::Validation("bad input".to_string());
        assert_eq!(classify_connection_error(&err), "unknown");
    }

    #[test]
    fn test_classify_client_missing() {
        let err = K8sError::ClientMissing;
        assert_eq!(classify_connection_error(&err), "unknown");
    }

    #[test]
    fn test_classify_cancelled() {
        let err = K8sError::Cancelled;
        assert_eq!(classify_connection_error(&err), "unknown");
    }

    #[test]
    fn test_classify_port_forward() {
        let err = K8sError::PortForward("port in use".to_string());
        assert_eq!(classify_connection_error(&err), "unknown");
    }
}
