import { useState, useEffect, useCallback, useMemo } from "react";
import { motion } from "framer-motion";
import {
  ArrowLeft,
  Settings as SettingsIcon,
  Monitor,
  Keyboard,
  Clock,
  Palette,
  Globe,
  Layout,
  Info,
  RefreshCw,
  ExternalLink,
  Download,
  CheckCircle,
  AlertTriangle,
} from "lucide-react";
import { cn } from "@/lib/utils";
import type { AppView } from "@/lib/types";

const SETTINGS_KEY = "kore-settings";

interface KoreSettings {
  eventRetention: string;
  defaultNamespaces: Record<string, string>;
  accentColor: string;
  defaultView: AppView;
}

const DEFAULT_SETTINGS: KoreSettings = {
  eventRetention: "7d",
  defaultNamespaces: {},
  accentColor: "#58d0ff",
  defaultView: "chat",
};

const DEFAULT_VIEW_OPTIONS: { value: AppView; label: string }[] = [
  { value: "chat", label: "AI Chat" },
  { value: "table", label: "Resource Table" },
  { value: "dashboard", label: "Dashboard" },
];

const RETENTION_OPTIONS = [
  { value: "1d", label: "1 day" },
  { value: "3d", label: "3 days" },
  { value: "7d", label: "7 days" },
  { value: "14d", label: "14 days" },
  { value: "30d", label: "30 days" },
];

const ACCENT_PRESETS = [
  { value: "#58d0ff", label: "Cyan", ring: "ring-cyan-400" },
  { value: "#a78bfa", label: "Violet", ring: "ring-violet-400" },
  { value: "#34d399", label: "Emerald", ring: "ring-emerald-400" },
  { value: "#fb923c", label: "Orange", ring: "ring-orange-400" },
  { value: "#f87171", label: "Red", ring: "ring-red-400" },
];

const SHORTCUTS = [
  {
    category: "Navigation",
    items: [
      { keys: "j / k", description: "Navigate up / down" },
      { keys: "l", description: "Enter detail view" },
      { keys: "h", description: "Go back" },
      { keys: "1 - 5", description: "Switch tabs" },
    ],
  },
  {
    category: "Actions",
    items: [
      { keys: "\u2318K", description: "Command palette" },
      { keys: "\u2318R", description: "Refresh resources" },
      { keys: "/", description: "Search / filter" },
      { keys: "\u2318F", description: "Search logs" },
      { keys: "D", description: "Delete resource" },
    ],
  },
  {
    category: "General",
    items: [
      { keys: "Esc", description: "Close / go back" },
      { keys: "Enter", description: "Select / confirm" },
      { keys: "?", description: "Show shortcut overlay" },
    ],
  },
];

function loadSettings(): KoreSettings {
  try {
    const stored = localStorage.getItem(SETTINGS_KEY);
    if (!stored) return { ...DEFAULT_SETTINGS };
    return { ...DEFAULT_SETTINGS, ...JSON.parse(stored) };
  } catch {
    return { ...DEFAULT_SETTINGS };
  }
}

function saveSettings(settings: KoreSettings) {
  localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
  // StorageEvent only fires in other windows; dispatch a custom event for same-window listeners
  window.dispatchEvent(new CustomEvent("kore-settings-change"));
}

/** Hook to read settings from localStorage */
export function useSettings(): KoreSettings {
  const [settings, setSettings] = useState<KoreSettings>(() => loadSettings());

  useEffect(() => {
    const reload = () => setSettings(loadSettings());
    const handleStorage = (e: StorageEvent) => {
      if (e.key === SETTINGS_KEY) reload();
    };
    window.addEventListener("storage", handleStorage);
    window.addEventListener("kore-settings-change", reload);
    return () => {
      window.removeEventListener("storage", handleStorage);
      window.removeEventListener("kore-settings-change", reload);
    };
  }, []);

  return settings;
}

function Kbd({ children }: { children: React.ReactNode }) {
  return (
    <kbd className="inline-flex items-center justify-center min-w-[22px] px-1.5 py-0.5 bg-slate-800/80 rounded text-[10px] text-slate-400 font-mono border border-slate-700/50">
      {children}
    </kbd>
  );
}

interface SettingsProps {
  onBack: () => void;
  updateAvailable?: boolean;
  latestVersion?: string | null;
  currentVersion?: string | null;
  releaseUrl?: string | null;
  releaseNotes?: string | null;
  onCheckForUpdates?: () => void;
  updateChecking?: boolean;
  onPerformUpdate?: () => void;
  updating?: boolean;
  updateError?: string | null;
  updateSuccess?: string | null;
}

export function Settings({
  onBack,
  updateAvailable,
  latestVersion,
  currentVersion,
  releaseUrl,
  releaseNotes,
  onCheckForUpdates,
  updateChecking,
  onPerformUpdate,
  updating,
  updateError,
  updateSuccess,
}: SettingsProps) {
  const [settings, setSettings] = useState<KoreSettings>(() => loadSettings());
  const [contextInput, setContextInput] = useState("");
  const [namespaceInput, setNamespaceInput] = useState("");

  // Esc to go back
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onBack();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onBack]);

  const updateSetting = useCallback(
    <K extends keyof KoreSettings>(key: K, value: KoreSettings[K]) => {
      setSettings((prev) => {
        const next = { ...prev, [key]: value };
        saveSettings(next);
        return next;
      });
    },
    [],
  );

  const handleAddDefaultNamespace = useCallback(() => {
    const ctx = contextInput.trim();
    const ns = namespaceInput.trim();
    if (!ctx || !ns) return;

    const updated = { ...settings.defaultNamespaces, [ctx]: ns };
    updateSetting("defaultNamespaces", updated);
    setContextInput("");
    setNamespaceInput("");
  }, [contextInput, namespaceInput, settings.defaultNamespaces, updateSetting]);

  const handleRemoveDefaultNamespace = useCallback(
    (ctx: string) => {
      const updated = { ...settings.defaultNamespaces };
      delete updated[ctx];
      updateSetting("defaultNamespaces", updated);
    },
    [settings.defaultNamespaces, updateSetting],
  );

  const namespaceEntries = useMemo(
    () => Object.entries(settings.defaultNamespaces),
    [settings.defaultNamespaces],
  );

  return (
    <motion.div
      initial={{ opacity: 0, x: 20 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: 20 }}
      className="h-full w-full flex flex-col bg-background"
    >
      {/* Header */}
      <div className="border-b border-slate-800 p-4 bg-surface/50">
        <div className="flex items-center gap-4">
          <button
            onClick={onBack}
            className="flex items-center gap-2 px-3 py-1.5 rounded-md border border-slate-800 hover:border-accent/50 hover:bg-muted/30 transition text-sm text-slate-300"
          >
            <ArrowLeft className="w-4 h-4" />
            Back
            <Kbd>Esc</Kbd>
          </button>
          <div className="flex items-center gap-2">
            <SettingsIcon className="w-5 h-5 text-accent" />
            <h1 className="text-lg font-semibold text-slate-100">Settings</h1>
          </div>
        </div>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-auto p-6 space-y-6 max-w-3xl">
        {/* General Section */}
        <motion.div
          initial={{ opacity: 0, y: 10 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.05 }}
          className="glass rounded-xl p-5"
        >
          <div className="flex items-center gap-2 mb-4">
            <Clock className="w-4 h-4 text-accent" />
            <h2 className="text-sm font-semibold text-slate-100">General</h2>
          </div>

          <div className="space-y-4">
            <div>
              <label className="block text-xs text-slate-400 mb-1.5">
                <div className="flex items-center gap-1.5">
                  <Layout className="w-3 h-3" />
                  Default View
                </div>
              </label>
              <select
                value={settings.defaultView}
                onChange={(e) => updateSetting("defaultView", e.target.value as AppView)}
                className="w-full max-w-xs px-3 py-2 bg-background border border-slate-800 rounded-md text-sm text-slate-100 outline-none focus:border-accent transition appearance-none cursor-pointer"
              >
                {DEFAULT_VIEW_OPTIONS.map((opt) => (
                  <option key={opt.value} value={opt.value}>
                    {opt.label}
                  </option>
                ))}
              </select>
              <p className="text-[10px] text-slate-500 mt-1">
                Which view to show when the app starts.
              </p>
            </div>

            <div>
              <label className="block text-xs text-slate-400 mb-1.5">Event Retention Period</label>
              <select
                value={settings.eventRetention}
                onChange={(e) => updateSetting("eventRetention", e.target.value)}
                className="w-full max-w-xs px-3 py-2 bg-background border border-slate-800 rounded-md text-sm text-slate-100 outline-none focus:border-accent transition appearance-none cursor-pointer"
              >
                {RETENTION_OPTIONS.map((opt) => (
                  <option key={opt.value} value={opt.value}>
                    {opt.label}
                  </option>
                ))}
              </select>
              <p className="text-[10px] text-slate-500 mt-1">
                How long to keep event history in the timeline view.
              </p>
            </div>
          </div>
        </motion.div>

        {/* Display Section */}
        <motion.div
          initial={{ opacity: 0, y: 10 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.1 }}
          className="glass rounded-xl p-5"
        >
          <div className="flex items-center gap-2 mb-4">
            <Monitor className="w-4 h-4 text-accent" />
            <h2 className="text-sm font-semibold text-slate-100">Display</h2>
          </div>

          <div className="space-y-5">
            {/* Default namespace per context */}
            <div>
              <label className="block text-xs text-slate-400 mb-1.5">
                <div className="flex items-center gap-1.5">
                  <Globe className="w-3 h-3" />
                  Default Namespace per Context
                </div>
              </label>

              {namespaceEntries.length > 0 && (
                <div className="mb-3 space-y-1.5">
                  {namespaceEntries.map(([ctx, ns]) => (
                    <div
                      key={ctx}
                      className="flex items-center gap-2 px-3 py-1.5 bg-background/50 border border-slate-800/50 rounded text-xs"
                    >
                      <span className="font-mono text-slate-300 flex-1 truncate">{ctx}</span>
                      <span className="text-slate-600">&rarr;</span>
                      <span className="font-mono text-accent truncate">{ns}</span>
                      <button
                        onClick={() => handleRemoveDefaultNamespace(ctx)}
                        className="text-slate-500 hover:text-red-400 transition ml-1"
                      >
                        &times;
                      </button>
                    </div>
                  ))}
                </div>
              )}

              <div className="flex gap-2">
                <input
                  type="text"
                  value={contextInput}
                  onChange={(e) => setContextInput(e.target.value)}
                  placeholder="Context name"
                  className="flex-1 px-3 py-2 bg-background border border-slate-800 rounded-md text-sm text-slate-100 placeholder-slate-600 outline-none focus:border-accent transition"
                />
                <input
                  type="text"
                  value={namespaceInput}
                  onChange={(e) => setNamespaceInput(e.target.value)}
                  placeholder="Namespace"
                  onKeyDown={(e) => {
                    if (e.key === "Enter") handleAddDefaultNamespace();
                  }}
                  className="flex-1 px-3 py-2 bg-background border border-slate-800 rounded-md text-sm text-slate-100 placeholder-slate-600 outline-none focus:border-accent transition"
                />
                <button
                  onClick={handleAddDefaultNamespace}
                  disabled={!contextInput.trim() || !namespaceInput.trim()}
                  className="px-4 py-2 bg-accent/15 border border-accent/40 rounded-md text-xs text-accent hover:bg-accent/25 transition disabled:opacity-40 disabled:cursor-not-allowed"
                >
                  Add
                </button>
              </div>
              <p className="text-[10px] text-slate-500 mt-1">
                Automatically select a namespace when switching to a context.
              </p>
            </div>

            {/* Theme accent color */}
            <div>
              <label className="block text-xs text-slate-400 mb-2">
                <div className="flex items-center gap-1.5">
                  <Palette className="w-3 h-3" />
                  Theme Accent Color
                </div>
              </label>
              <div className="flex gap-3">
                {ACCENT_PRESETS.map((preset) => (
                  <button
                    key={preset.value}
                    onClick={() => updateSetting("accentColor", preset.value)}
                    className={cn(
                      "flex flex-col items-center gap-1.5 p-2 rounded-lg border transition",
                      settings.accentColor === preset.value
                        ? "border-slate-600 bg-slate-800/50"
                        : "border-transparent hover:border-slate-700 hover:bg-slate-800/30",
                    )}
                  >
                    <div
                      className={cn(
                        "w-8 h-8 rounded-full ring-2 ring-offset-2 ring-offset-background transition",
                        settings.accentColor === preset.value
                          ? "ring-slate-400 scale-110"
                          : "ring-transparent",
                      )}
                      style={{ backgroundColor: preset.value }}
                    />
                    <span className="text-[10px] text-slate-500">{preset.label}</span>
                  </button>
                ))}
              </div>
            </div>
          </div>
        </motion.div>

        {/* Keyboard Shortcuts Section */}
        <motion.div
          initial={{ opacity: 0, y: 10 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.15 }}
          className="glass rounded-xl p-5"
        >
          <div className="flex items-center gap-2 mb-4">
            <Keyboard className="w-4 h-4 text-accent" />
            <h2 className="text-sm font-semibold text-slate-100">Keyboard Shortcuts</h2>
          </div>

          <div className="space-y-4">
            {SHORTCUTS.map((group) => (
              <div key={group.category}>
                <p className="text-[10px] uppercase tracking-wider text-slate-500 mb-2 font-medium">
                  {group.category}
                </p>
                <div className="space-y-0.5">
                  {group.items.map((shortcut) => (
                    <div
                      key={shortcut.keys}
                      className="flex items-center justify-between px-3 py-2 rounded hover:bg-white/[0.02] transition"
                    >
                      <span className="text-xs text-slate-300">{shortcut.description}</span>
                      <div className="flex items-center gap-1">
                        {shortcut.keys.split(" / ").map((key, i) => (
                          <span key={i} className="flex items-center gap-1">
                            {i > 0 && <span className="text-slate-600 text-[10px] mx-0.5">/</span>}
                            <Kbd>{key.trim()}</Kbd>
                          </span>
                        ))}
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            ))}
          </div>
        </motion.div>

        {/* About Section */}
        <motion.div
          initial={{ opacity: 0, y: 10 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.2 }}
          className="glass rounded-xl p-5"
        >
          <div className="flex items-center gap-2 mb-4">
            <Info className="w-4 h-4 text-accent" />
            <h2 className="text-sm font-semibold text-slate-100">About</h2>
          </div>

          <div className="space-y-4">
            <div className="flex items-center justify-between">
              <span className="text-xs text-slate-400">Current Version</span>
              <span className="text-xs font-mono text-slate-200">
                {currentVersion ?? "unknown"}
              </span>
            </div>

            {updateSuccess && (
              <div className="px-3 py-3 bg-emerald-500/10 border border-emerald-500/30 rounded-lg flex items-center gap-2">
                <CheckCircle className="w-4 h-4 text-emerald-400 shrink-0" />
                <span className="text-xs text-emerald-300">
                  Updated to {updateSuccess} — restart Kore to apply.
                </span>
              </div>
            )}

            {updateError && (
              <div className="px-3 py-3 bg-red-500/10 border border-red-500/30 rounded-lg space-y-2">
                <div className="flex items-center gap-2">
                  <AlertTriangle className="w-4 h-4 text-red-400 shrink-0" />
                  <span className="text-xs text-red-300">Update failed: {updateError}</span>
                </div>
                {releaseUrl && (
                  <button
                    onClick={async () => {
                      try {
                        const { open } = await import("@tauri-apps/plugin-shell");
                        await open(releaseUrl);
                      } catch {
                        window.open(releaseUrl, "_blank");
                      }
                    }}
                    className="flex items-center gap-1 text-[11px] text-slate-400 hover:text-slate-200 transition"
                  >
                    <ExternalLink className="w-3 h-3" />
                    Download manually instead
                  </button>
                )}
              </div>
            )}

            {updateAvailable && !updateSuccess && (
              <div className="px-3 py-3 bg-accent/10 border border-accent/30 rounded-lg space-y-2">
                <div className="flex items-center justify-between">
                  <span className="text-xs font-medium text-accent">
                    Update available: {latestVersion}
                  </span>
                  <button
                    onClick={onPerformUpdate}
                    disabled={updating}
                    className="flex items-center gap-1.5 px-2.5 py-1 bg-accent/20 hover:bg-accent/30 rounded text-[11px] text-accent transition disabled:opacity-50"
                  >
                    {updating ? (
                      <>
                        <span className="w-3 h-3 border-[1.5px] border-accent/30 border-t-accent rounded-full animate-spin" />
                        Installing...
                      </>
                    ) : (
                      <>
                        <Download className="w-3 h-3" />
                        Update Now
                      </>
                    )}
                  </button>
                </div>
                {releaseNotes && (
                  <p className="text-[11px] text-slate-400 leading-relaxed line-clamp-3">
                    {releaseNotes}
                  </p>
                )}
              </div>
            )}

            <button
              onClick={onCheckForUpdates}
              disabled={updateChecking}
              className="flex items-center gap-2 px-3 py-2 bg-background border border-slate-800 rounded-md text-xs text-slate-300 hover:border-accent/50 hover:text-slate-100 transition disabled:opacity-50"
            >
              <RefreshCw className={cn("w-3.5 h-3.5", updateChecking && "animate-spin")} />
              {updateChecking ? "Checking..." : "Check for Updates"}
            </button>
          </div>
        </motion.div>
      </div>
    </motion.div>
  );
}
