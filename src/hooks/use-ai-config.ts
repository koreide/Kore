import { useEffect, useState } from "react";
import { providers, type AIConfig } from "@/components/ai-settings";

const STORAGE_KEY = "kore-ai-config";
const EVENT_NAME = "kore-ai-config-change";

export function loadAIConfig(): AIConfig {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored) {
      const { api_key: _, ...config } = JSON.parse(stored);
      return config as AIConfig;
    }
  } catch {
    // ignore parse errors
  }
  return { provider: "openai", model: "gpt-4o" };
}

export function isProviderConfigured(config: AIConfig): boolean {
  const provider = providers.find((p) => p.id === config.provider);
  if (!provider) return false;
  if (provider.needsApiKey && !config.api_key) return false;
  return true;
}

export function useAIConfig() {
  const [aiConfig, setAiConfig] = useState<AIConfig>(() => loadAIConfig());

  useEffect(() => {
    const reload = () => setAiConfig(loadAIConfig());
    window.addEventListener(EVENT_NAME, reload);
    return () => window.removeEventListener(EVENT_NAME, reload);
  }, []);

  const isConfigured = isProviderConfigured(aiConfig);

  return { aiConfig, isConfigured };
}
