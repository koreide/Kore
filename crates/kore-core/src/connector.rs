//! How [`crate::Core`] turns a kubeconfig context into a `kube::Client`.
//!
//! The default, [`KubeconfigConnector`], builds a real client from the kubeconfig.
//! Tests inject their own connector through [`crate::CoreConfig::connector`]
//! (e.g. `kore-testkit`'s in-memory API server), so no real cluster is touched.

use crate::error::{K8sError, Result};
use futures::future::BoxFuture;
use kube::config::{KubeConfigOptions, Kubeconfig};
use kube::{Client, Config};
use std::fmt;
use std::sync::Arc;

pub trait ClusterConnector: Send + Sync + 'static {
    /// Build a client for `context` (`None` = the kubeconfig's current-context).
    /// Always called on Core's tokio runtime.
    fn connect<'a>(&'a self, kubeconfig: &'a Kubeconfig, context: Option<String>) -> BoxFuture<'a, Result<Client>>;
}

/// Real clients from the kubeconfig (TLS, auth plugins, proxies).
pub struct KubeconfigConnector;

impl ClusterConnector for KubeconfigConnector {
    fn connect<'a>(&'a self, kubeconfig: &'a Kubeconfig, context: Option<String>) -> BoxFuture<'a, Result<Client>> {
        Box::pin(async move {
            let opts = KubeConfigOptions { context, ..Default::default() };
            let config = Config::from_custom_kubeconfig(kubeconfig.clone(), &opts).await?;
            Client::try_from(config).map_err(K8sError::Kube)
        })
    }
}

/// Cloneable handle stored in [`crate::CoreConfig`].
#[derive(Clone)]
pub struct Connector(pub Arc<dyn ClusterConnector>);

impl Connector {
    pub fn new(c: impl ClusterConnector) -> Self {
        Connector(Arc::new(c))
    }
}

impl Default for Connector {
    fn default() -> Self {
        Connector::new(KubeconfigConnector)
    }
}

impl fmt::Debug for Connector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Connector")
    }
}

impl std::ops::Deref for Connector {
    type Target = dyn ClusterConnector;
    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}
