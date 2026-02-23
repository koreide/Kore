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
}
