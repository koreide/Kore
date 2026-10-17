//! AI provider configuration (spec 03 §6.1), stored under `kore-ai-config`
//! (without the API key), plus the app-global "config changed" broadcast.
//!
//! Every AI surface (AI Chat, AI Panel, Network Policies, Settings) reads the
//! same [`AiConfigModel`] entity via [`model`] and `cx.observe`s it.
//!
//! Deliberate Fix 11: the model also holds the provider's API key, loaded from
//! the OS keychain (`get_api_key`) whenever the provider needs one, so OpenAI
//! and Anthropic work in every chat surface.

use super::storage;
use crate::backend;
use gpui_kit::*;
use serde::{Deserialize, Serialize};

pub const STORAGE_KEY: &str = "kore-ai-config";

/// One entry of the provider table (segmented-control order).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProviderOption {
    pub id: &'static str,
    pub label: &'static str,
    pub default_model: &'static str,
    pub needs_api_key: bool,
    pub needs_base_url: bool,
    pub default_base_url: Option<&'static str>,
}

pub const PROVIDERS: [ProviderOption; 5] = [
    ProviderOption { id: "openai", label: "OpenAI", default_model: "gpt-4o", needs_api_key: true, needs_base_url: false, default_base_url: None },
    ProviderOption {
        id: "anthropic",
        label: "Anthropic",
        default_model: "claude-sonnet-4-20250514",
        needs_api_key: true,
        needs_base_url: false,
        default_base_url: None,
    },
    ProviderOption {
        id: "ollama",
        label: "Ollama",
        default_model: "llama3.1",
        needs_api_key: false,
        needs_base_url: true,
        default_base_url: Some("http://localhost:11434"),
    },
    ProviderOption { id: "claude_cli", label: "Claude Code", default_model: "sonnet", needs_api_key: false, needs_base_url: false, default_base_url: None },
    ProviderOption { id: "cursor_agent", label: "Cursor Agent", default_model: "auto", needs_api_key: false, needs_base_url: false, default_base_url: None },
];

pub fn provider_option(id: &str) -> Option<&'static ProviderOption> {
    PROVIDERS.iter().find(|p| p.id == id)
}

/// AI provider configuration. `api_key` is never persisted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiConfig {
    pub provider: String,
    #[serde(skip_serializing, default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

impl Default for AiConfig {
    fn default() -> Self {
        AiConfig { provider: "openai".into(), api_key: None, model: "gpt-4o".into(), base_url: None }
    }
}

impl AiConfig {
    /// `loadAIConfig()`: stored config (key stripped) or the OpenAI default.
    pub fn load() -> AiConfig {
        let mut c: AiConfig = storage::get(STORAGE_KEY).unwrap_or_default();
        c.api_key = None;
        c
    }

    pub fn save(&self) {
        storage::set(STORAGE_KEY, self);
    }

    pub fn provider(&self) -> Option<&'static ProviderOption> {
        provider_option(&self.provider)
    }

    /// The backend config (`AIConfig` of kore-core).
    pub fn to_core(&self) -> kore_core::AIConfig {
        use kore_core::AIProvider as P;
        let provider = match self.provider.as_str() {
            "anthropic" => P::Anthropic,
            "ollama" => P::Ollama,
            "claude_cli" => P::ClaudeCli,
            "cursor_agent" => P::CursorAgent,
            _ => P::OpenAI,
        };
        kore_core::AIConfig {
            provider,
            api_key: self.api_key.clone().filter(|k| !k.is_empty()),
            model: self.model.clone(),
            base_url: self.base_url.clone().filter(|b| !b.is_empty()),
        }
    }
}

/// `isProviderConfigured(config)`.
pub fn is_provider_configured(config: &AiConfig) -> bool {
    match config.provider() {
        None => false,
        Some(p) => !p.needs_api_key || config.api_key.as_deref().is_some_and(|k| !k.is_empty()),
    }
}

/// App-global AI config (observe it to react to changes).
pub struct AiConfigModel {
    pub config: AiConfig,
    key_task: Option<Task<()>>,
}

struct GlobalAiConfig(Entity<AiConfigModel>);
impl Global for GlobalAiConfig {}

/// The shared config entity (created on first use; loads the API key — Fix 11).
pub fn model(cx: &mut App) -> Entity<AiConfigModel> {
    if let Some(g) = cx.try_global::<GlobalAiConfig>() {
        return g.0.clone();
    }
    let m = cx.new(|cx| {
        let mut m = AiConfigModel { config: AiConfig::load(), key_task: None };
        m.load_key(cx);
        m
    });
    cx.set_global(GlobalAiConfig(m.clone()));
    m
}

/// Current config snapshot (with the API key once loaded).
pub fn current(cx: &mut App) -> AiConfig {
    model(cx).read(cx).config.clone()
}

impl AiConfigModel {
    pub fn is_configured(&self) -> bool {
        is_provider_configured(&self.config)
    }

    /// Replace the config: persists (without key) and notifies observers.
    /// Loads the keychain key when the provider changed to a key-needing one.
    pub fn set(&mut self, config: AiConfig, cx: &mut Context<Self>) {
        let provider_changed = config.provider != self.config.provider;
        if config == self.config {
            return;
        }
        self.config = config;
        self.config.save();
        if provider_changed && self.config.api_key.is_none() {
            self.load_key(cx);
        }
        cx.notify();
    }

    fn load_key(&mut self, cx: &mut Context<Self>) {
        let Some(p) = self.config.provider() else { return };
        if !p.needs_api_key {
            self.key_task = None;
            return;
        }
        let provider = p.id.to_string();
        let prov = provider.clone();
        self.key_task = Some(backend::run(cx, move |core| async move { core.get_api_key(prov).await }, move |this: &mut Self, res, cx| {
            if let Ok(Some(key)) = res {
                if this.config.provider == provider && this.config.api_key.is_none() && !key.is_empty() {
                    this.config.api_key = Some(key);
                    cx.notify();
                }
            }
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::{is_provider_configured, AiConfig};

    #[test]
    fn configured_rules() {
        let mut c = AiConfig::default();
        assert!(!is_provider_configured(&c));
        c.api_key = Some("k".into());
        assert!(is_provider_configured(&c));
        c = AiConfig { provider: "ollama".into(), api_key: None, model: "m".into(), base_url: None };
        assert!(is_provider_configured(&c));
        c.provider = "nope".into();
        assert!(!is_provider_configured(&c));
    }

    #[test]
    fn key_is_not_serialized() {
        let c = AiConfig { provider: "openai".into(), api_key: Some("secret".into()), model: "gpt-4o".into(), base_url: None };
        let s = serde_json::to_string(&c).unwrap();
        assert!(!s.contains("secret"));
        assert_eq!(s, r#"{"provider":"openai","model":"gpt-4o"}"#);
        let back: AiConfig = serde_json::from_str(r#"{"provider":"ollama","model":"x","base_url":"http://h"}"#).unwrap();
        assert_eq!(back.base_url.as_deref(), Some("http://h"));
    }
}
