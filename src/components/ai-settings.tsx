import { useCallback, useEffect, useState } from "react";
import { motion } from "framer-motion";
import { Settings2, Loader2, CheckCircle2, AlertCircle } from "lucide-react";
import { cn } from "@/lib/utils";
import { useToast } from "./toast";

// ── Types ────────────────────────────────────────────────────────────────

export interface AIConfig {
  provider: "openai" | "anthropic" | "ollama";
  api_key?: string;
  model: string;
  base_url?: string;
}

interface AISettingsProps {
  config: AIConfig;
  onConfigChange: (config: AIConfig) => void;
}

// ── Constants ────────────────────────────────────────────────────────────

type ProviderOption = {
  id: AIConfig["provider"];
  label: string;
  defaultModel: string;
  needsApiKey: boolean;
  needsBaseUrl: boolean;
  defaultBaseUrl?: string;
};

const providers: ProviderOption[] = [
  { id: "openai", label: "OpenAI", defaultModel: "gpt-4o", needsApiKey: true, needsBaseUrl: false },
  { id: "anthropic", label: "Anthropic", defaultModel: "claude-sonnet-4-20250514", needsApiKey: true, needsBaseUrl: false },
  { id: "ollama", label: "Ollama", defaultModel: "llama3.1", needsApiKey: false, needsBaseUrl: true, defaultBaseUrl: "http://localhost:11434" },
];

const STORAGE_KEY = "kore-ai-config";

// ── Component ────────────────────────────────────────────────────────────

export function AISettings({ config, onConfigChange }: AISettingsProps) {
  const [isTesting, setIsTesting] = useState(false);
  const [testResult, setTestResult] = useState<"success" | "error" | null>(null);
  const toast = useToast();

  const currentProvider = providers.find((p) => p.id === config.provider) || providers[0];

  // Persist config to localStorage whenever it changes
  useEffect(() => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(config));
    } catch {
      // ignore write errors
    }
  }, [config]);

  const handleProviderChange = useCallback(
    (providerId: AIConfig["provider"]) => {
      const provider = providers.find((p) => p.id === providerId)!;
      setTestResult(null);
      onConfigChange({
        ...config,
        provider: providerId,
        model: provider.defaultModel,
        base_url: provider.defaultBaseUrl || undefined,
        api_key: providerId === config.provider ? config.api_key : undefined,
      });
    },
    [config, onConfigChange],
  );

  const handleTestConnection = useCallback(async () => {
    setIsTesting(true);
    setTestResult(null);

    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("ai_test_connection", { config });
      setTestResult("success");
      toast("Connection successful", "success");
    } catch (err) {
      setTestResult("error");
      const msg = err instanceof Error ? err.message : String(err);
      toast(`Connection failed: ${msg}`, "error");
    } finally {
      setIsTesting(false);
    }
  }, [config, toast]);

  return (
    <motion.div
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.2 }}
      className="space-y-5"
    >
      {/* Section Header */}
      <div className="flex items-center gap-2">
        <Settings2 className="w-4 h-4 text-accent" />
        <h3 className="text-sm font-semibold text-slate-100">AI Provider</h3>
      </div>

      {/* Provider Segmented Control */}
      <div className="flex rounded-lg border border-slate-800 overflow-hidden">
        {providers.map((provider) => (
          <button
            key={provider.id}
            onClick={() => handleProviderChange(provider.id)}
            className={cn(
              "flex-1 px-3 py-2 text-xs font-medium transition",
              config.provider === provider.id
                ? "bg-accent/15 text-accent border-accent/30"
                : "bg-surface/60 text-slate-400 hover:text-slate-200 hover:bg-muted/40",
              provider.id !== providers[providers.length - 1].id && "border-r border-slate-800",
            )}
          >
            {provider.label}
          </button>
        ))}
      </div>

      {/* API Key (OpenAI / Anthropic) */}
      {currentProvider.needsApiKey && (
        <div>
          <label className="block text-xs text-slate-400 mb-1.5">API Key</label>
          <input
            type="password"
            value={config.api_key || ""}
            onChange={(e) => {
              setTestResult(null);
              onConfigChange({ ...config, api_key: e.target.value || undefined });
            }}
            placeholder={`Enter your ${currentProvider.label} API key`}
            className="w-full px-3 py-2 bg-surface/60 border border-slate-800 rounded-lg text-sm text-slate-100 placeholder-slate-600 focus:outline-none focus:border-accent/50 transition"
          />
        </div>
      )}

      {/* Model */}
      <div>
        <label className="block text-xs text-slate-400 mb-1.5">Model</label>
        <input
          type="text"
          value={config.model}
          onChange={(e) => {
            setTestResult(null);
            onConfigChange({ ...config, model: e.target.value });
          }}
          placeholder={currentProvider.defaultModel}
          className="w-full px-3 py-2 bg-surface/60 border border-slate-800 rounded-lg text-sm text-slate-100 placeholder-slate-600 focus:outline-none focus:border-accent/50 transition font-mono"
        />
      </div>

      {/* Base URL (Ollama) */}
      {currentProvider.needsBaseUrl && (
        <div>
          <label className="block text-xs text-slate-400 mb-1.5">Base URL</label>
          <input
            type="text"
            value={config.base_url || ""}
            onChange={(e) => {
              setTestResult(null);
              onConfigChange({ ...config, base_url: e.target.value || undefined });
            }}
            placeholder={currentProvider.defaultBaseUrl || "http://localhost:11434"}
            className="w-full px-3 py-2 bg-surface/60 border border-slate-800 rounded-lg text-sm text-slate-100 placeholder-slate-600 focus:outline-none focus:border-accent/50 transition font-mono"
          />
        </div>
      )}

      {/* Test Connection */}
      <button
        onClick={handleTestConnection}
        disabled={isTesting || (currentProvider.needsApiKey && !config.api_key)}
        className={cn(
          "w-full px-4 py-2 rounded-lg border text-sm font-medium transition flex items-center justify-center gap-2",
          testResult === "success"
            ? "bg-emerald-500/10 border-emerald-500/50 text-emerald-400"
            : testResult === "error"
              ? "bg-red-500/10 border-red-500/50 text-red-400"
              : "bg-accent/10 border-accent/40 text-accent hover:bg-accent/20",
          "disabled:opacity-40 disabled:cursor-not-allowed",
        )}
      >
        {isTesting ? (
          <>
            <Loader2 className="w-4 h-4 animate-spin" />
            Testing...
          </>
        ) : testResult === "success" ? (
          <>
            <CheckCircle2 className="w-4 h-4" />
            Connected
          </>
        ) : testResult === "error" ? (
          <>
            <AlertCircle className="w-4 h-4" />
            Failed - Retry
          </>
        ) : (
          "Test Connection"
        )}
      </button>
    </motion.div>
  );
}
