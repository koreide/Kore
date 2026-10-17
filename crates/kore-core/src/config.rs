//! Injectable paths and settings for [`crate::Core`].

use crate::connector::Connector;
use crate::secrets::Secrets;
use std::path::PathBuf;

/// Configuration for a [`crate::Core`]. `Default` uses the standard locations:
/// kubeconfig from `KUBECONFIG` / `~/.kube/config`, events DB at
/// `dirs::data_dir()/kore/events.db`, favorites at `~/.kore/favorites.json`.
#[derive(Debug, Clone)]
pub struct CoreConfig {
    /// Explicit kubeconfig files (merged in order, like `KUBECONFIG`). Empty
    /// means "use `KUBECONFIG` env or `~/.kube/config`". When set, the same
    /// files are passed to `kubectl`/`helm` subprocesses via `KUBECONFIG`.
    pub kubeconfig_paths: Vec<PathBuf>,
    /// Context to start in. `None` = kubeconfig `current-context`.
    pub initial_context: Option<String>,
    /// Directory holding `events.db`. `None` = `dirs::data_dir()/kore`.
    pub data_dir: Option<PathBuf>,
    /// Directory holding `favorites.json`. `None` = `~/.kore`.
    pub favorites_dir: Option<PathBuf>,
    /// Event store retention in days (pruned at startup and hourly).
    pub event_retention_days: u32,
    /// Worker threads of the owned tokio runtime.
    pub worker_threads: usize,
    /// Builds kube clients per context. Default: from the kubeconfig.
    pub connector: Connector,
    /// API key storage. Default: the OS keychain.
    pub secrets: Secrets,
    /// GitHub "latest release" API URL used by the update check.
    pub update_url: String,
    /// Run `kubectl` / `helm` / `claude` / `agent` from this directory only (and
    /// give children `PATH=<dir>:/usr/bin:/bin`). A tool missing from it counts as
    /// not installed. `None` = PATH lookup plus the well-known tool directories.
    pub tool_dir: Option<PathBuf>,
    /// AI provider endpoints (defaults: the public APIs and local Ollama).
    pub ai_endpoints: AiEndpoints,
}

/// Default AI provider endpoints. `None` falls back to `KORE_OPENAI_BASE_URL` /
/// `KORE_ANTHROPIC_BASE_URL`, then the public API; Ollama to `http://localhost:11434`
/// when no base URL is configured in the app.
#[derive(Debug, Clone, Default)]
pub struct AiEndpoints {
    pub openai: Option<String>,
    pub anthropic: Option<String>,
    pub ollama_default: Option<String>,
}

impl Default for CoreConfig {
    fn default() -> Self {
        Self {
            kubeconfig_paths: Vec::new(),
            initial_context: None,
            data_dir: None,
            favorites_dir: None,
            event_retention_days: 7,
            worker_threads: 4,
            connector: Connector::default(),
            secrets: Secrets::default(),
            update_url: crate::state::update::GITHUB_RELEASES_URL.to_string(),
            tool_dir: None,
            ai_endpoints: AiEndpoints::default(),
        }
    }
}

impl CoreConfig {
    pub fn data_dir(&self) -> Option<PathBuf> {
        self.data_dir
            .clone()
            .or_else(|| dirs::data_dir().map(|d| d.join("kore")))
    }

    pub fn favorites_dir(&self) -> Option<PathBuf> {
        self.favorites_dir
            .clone()
            .or_else(|| dirs::home_dir().map(|h| h.join(".kore")))
    }
}
