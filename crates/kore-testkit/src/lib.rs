//! Test doubles for Kore (dev-dependency only).
//!
//! - [`FakeCluster`]: an in-memory Kubernetes API server, served to kube-rs.
//! - [`FakeConnector`]: maps kubeconfig context names to fake clusters (or to
//!   connection errors and hanging clusters).
//! - [`FakeEnv`]: a temporary kubeconfig and data directories, producing a
//!   [`kore_core::CoreConfig`] that never touches the real machine's state.

pub mod fake;
pub mod fixtures;
pub mod mock_ai;

pub use fake::{FakeCluster, RecordedRequest};
pub use mock_ai::MockAi;

use futures::future::BoxFuture;
use kore_core::{ClusterConnector, Connector, CoreConfig, K8sError};
use kube::config::Kubeconfig;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
enum Target {
    Cluster(FakeCluster),
    /// `connect` fails with this message (bad credentials, missing context...).
    Error(String),
}

/// Context name → fake cluster. Unknown contexts fail like kube-rs does.
#[derive(Clone, Default)]
pub struct FakeConnector {
    targets: Arc<Mutex<BTreeMap<String, Target>>>,
}

impl FakeConnector {
    pub fn set_cluster(&self, context: &str, cluster: FakeCluster) {
        self.targets.lock().unwrap().insert(context.into(), Target::Cluster(cluster));
    }

    pub fn set_error(&self, context: &str, message: &str) {
        self.targets.lock().unwrap().insert(context.into(), Target::Error(message.into()));
    }
}

impl ClusterConnector for FakeConnector {
    fn connect<'a>(&'a self, kubeconfig: &'a Kubeconfig, context: Option<String>) -> BoxFuture<'a, kore_core::error::Result<kube::Client>> {
        Box::pin(async move {
            let name = context
                .or_else(|| kubeconfig.current_context.clone())
                .ok_or_else(|| K8sError::Other("no context".into()))?;
            let ns = kubeconfig
                .contexts
                .iter()
                .find(|c| c.name == name)
                .and_then(|c| c.context.as_ref())
                .and_then(|c| c.namespace.clone())
                .unwrap_or_else(|| "default".into());
            match self.targets.lock().unwrap().get(&name).cloned() {
                Some(Target::Cluster(c)) => Ok(c.client(&ns)),
                Some(Target::Error(msg)) => Err(K8sError::Other(msg)),
                None => Err(K8sError::Other(format!("context \"{name}\" not found in kubeconfig"))),
            }
        })
    }
}

/// A sandbox: temp dir holding a kubeconfig (one entry per context) and the
/// Core / app data directories. Dropping it deletes everything.
pub struct FakeEnv {
    pub dir: tempfile::TempDir,
    pub connector: FakeConnector,
    contexts: Vec<(String, Option<String>)>,
    current: Option<String>,
    update_url: Option<String>,
    ai_url: Option<String>,
}

/// A local port nothing listens on: "offline" default for every outbound URL.
pub const CLOSED: &str = "http://127.0.0.1:9";

impl Default for FakeEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeEnv {
    pub fn new() -> Self {
        FakeEnv { dir: tempfile::tempdir().expect("tempdir"), connector: FakeConnector::default(), contexts: Vec::new(), current: None, update_url: None, ai_url: None }
    }

    /// Add a context backed by `cluster`. The first context added is current unless `current` is called.
    pub fn context(mut self, name: &str, cluster: &FakeCluster) -> Self {
        self.connector.set_cluster(name, cluster.clone());
        self.contexts.push((name.into(), None));
        self
    }

    /// Add a context whose connection fails with `message`.
    pub fn broken_context(mut self, name: &str, message: &str) -> Self {
        self.connector.set_error(name, message);
        self.contexts.push((name.into(), None));
        self
    }

    pub fn current(mut self, name: &str) -> Self {
        self.current = Some(name.into());
        self
    }

    /// Serve the update check from `url` (e.g. a local test server).
    pub fn update_url(mut self, url: &str) -> Self {
        self.update_url = Some(url.into());
        self
    }

    /// Default AI endpoint for every provider (e.g. the in-process mock AI server).
    pub fn ai_url(mut self, url: &str) -> Self {
        self.ai_url = Some(url.into());
        self
    }

    /// Install an executable `name` (e.g. `helm`, `kubectl`) in the sandbox's tool
    /// dir. `script` is a POSIX shell script body; `$KORE_TOOL_LOG` is appended
    /// with the argv of each call (see [`FakeEnv::tool_calls`]).
    pub fn tool(self, name: &str, script: &str) -> Self {
        let dir = self.bin_dir();
        std::fs::create_dir_all(&dir).expect("bin dir");
        let log = self.dir.path().join("tool-calls.log");
        let body = format!(
            "#!/bin/sh\nprintf '%s' \"{name}\" >> '{log}'; for a in \"$@\"; do printf ' %s' \"$a\" >> '{log}'; done; echo >> '{log}'\n{script}\n",
            log = log.display()
        );
        let path = dir.join(name);
        std::fs::write(&path, body).expect("write tool");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        }
        self
    }

    /// Every recorded tool invocation, as `"<tool> <args...>"`.
    pub fn tool_calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.path().join("tool-calls.log"))
            .map(|s| s.lines().map(String::from).collect())
            .unwrap_or_default()
    }

    pub fn bin_dir(&self) -> PathBuf {
        self.dir.path().join("bin")
    }

    pub fn kubeconfig_path(&self) -> PathBuf {
        self.dir.path().join("kubeconfig")
    }

    pub fn data_dir(&self) -> PathBuf {
        self.dir.path().join("data")
    }

    fn write_kubeconfig(&self) {
        let mut clusters = String::new();
        let mut contexts = String::new();
        let mut users = String::new();
        for (i, (name, ns)) in self.contexts.iter().enumerate() {
            clusters.push_str(&format!("- name: {name}\n  cluster:\n    server: https://fake-{i}.invalid\n"));
            let ns_line = ns.as_ref().map(|n| format!("    namespace: {n}\n")).unwrap_or_default();
            contexts.push_str(&format!("- name: {name}\n  context:\n    cluster: {name}\n    user: {name}\n{ns_line}"));
            users.push_str(&format!("- name: {name}\n  user:\n    token: fake\n"));
        }
        let current = self.current.clone().or_else(|| self.contexts.first().map(|c| c.0.clone())).unwrap_or_default();
        let yaml = format!(
            "apiVersion: v1\nkind: Config\ncurrent-context: {current}\nclusters:\n{clusters}contexts:\n{contexts}users:\n{users}"
        );
        std::fs::write(self.kubeconfig_path(), yaml).expect("write kubeconfig");
    }

    /// CoreConfig pointing at this sandbox and the fake connector.
    pub fn core_config(&self) -> CoreConfig {
        self.write_kubeconfig();
        std::fs::create_dir_all(self.data_dir()).ok();
        std::fs::create_dir_all(self.bin_dir()).ok();
        CoreConfig {
            kubeconfig_paths: vec![self.kubeconfig_path()],
            initial_context: None,
            data_dir: Some(self.data_dir()),
            favorites_dir: Some(self.data_dir()),
            event_retention_days: 7,
            worker_threads: 2,
            connector: Connector::new(self.connector.clone()),
            secrets: kore_core::Secrets::memory(),
            // Offline: a closed local port (tests that need an answer serve one).
            update_url: self.update_url.clone().unwrap_or_else(|| format!("{CLOSED}/releases/latest")),
            // Only scripts installed with `tool` exist: no real kubectl/helm/claude.
            tool_dir: Some(self.bin_dir()),
            ai_endpoints: kore_core::AiEndpoints {
                openai: Some(self.ai_url.clone().unwrap_or_else(|| CLOSED.into())),
                anthropic: Some(self.ai_url.clone().unwrap_or_else(|| CLOSED.into())),
                ollama_default: Some(self.ai_url.clone().unwrap_or_else(|| CLOSED.into())),
            },
        }
    }
}
