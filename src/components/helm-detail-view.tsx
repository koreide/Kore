import { useEffect, useState, useCallback } from "react";
import { motion, AnimatePresence } from "framer-motion";
import {
  ArrowLeft,
  Copy,
  RotateCcw,
  FileText,
  ScrollText,
  History,
  StickyNote,
} from "lucide-react";
import { getHelmValues, getHelmManifest, getHelmHistory, rollbackHelmRelease } from "@/lib/api";
import type { HelmRevision } from "@/lib/api";
import { formatError } from "@/lib/errors";
import { ConfirmDialog } from "./confirm-dialog";
import { useToast } from "./toast";
import { cn } from "@/lib/utils";

interface HelmDetailViewProps {
  release: { name: string; namespace: string };
  onBack: () => void;
}

type TabId = "values" | "manifest" | "history" | "notes";

function Kbd({ children }: { children: React.ReactNode }) {
  return (
    <kbd className="px-1.5 py-0.5 bg-slate-800/80 rounded text-[10px] text-slate-400 font-mono border border-slate-700/50 ml-1.5">
      {children}
    </kbd>
  );
}

function ContentSkeleton() {
  const widths = ["80%", "65%", "90%", "45%", "75%", "55%", "85%", "40%", "70%", "60%"];
  return (
    <div className="p-4 space-y-2.5">
      {widths.map((w, i) => (
        <div key={i} className="skeleton h-3" style={{ width: w, opacity: 1 - i * 0.06 }} />
      ))}
    </div>
  );
}

function HistorySkeleton() {
  return (
    <div className="p-4 space-y-3">
      <div className="skeleton h-4 w-full" />
      {Array.from({ length: 5 }).map((_, i) => (
        <div key={i} className="flex gap-4">
          <div className="skeleton h-4 w-12" style={{ opacity: 1 - i * 0.1 }} />
          <div className="skeleton h-4 w-32" style={{ opacity: 1 - i * 0.1 }} />
          <div className="skeleton h-4 w-20" style={{ opacity: 1 - i * 0.1 }} />
          <div className="skeleton h-4 w-24" style={{ opacity: 1 - i * 0.1 }} />
          <div className="skeleton h-4 flex-1" style={{ opacity: 1 - i * 0.1 }} />
        </div>
      ))}
    </div>
  );
}

/** Simple YAML syntax highlighting */
function highlightYaml(yaml: string): string {
  return yaml
    .split("\n")
    .map((line) => {
      // Comments
      if (/^\s*#/.test(line)) {
        return `<span class="text-slate-500">${escapeHtml(line)}</span>`;
      }
      // Key: value lines
      const keyMatch = line.match(/^(\s*)([\w.-]+)(\s*:\s*)(.*)/);
      if (keyMatch) {
        const [, indent, key, colon, value] = keyMatch;
        const highlightedValue = highlightYamlValue(value);
        return `${escapeHtml(indent)}<span class="text-accent">${escapeHtml(key)}</span>${escapeHtml(colon)}${highlightedValue}`;
      }
      // List items
      const listMatch = line.match(/^(\s*-\s+)(.*)/);
      if (listMatch) {
        const [, prefix, value] = listMatch;
        return `<span class="text-slate-500">${escapeHtml(prefix)}</span>${highlightYamlValue(value)}`;
      }
      return escapeHtml(line);
    })
    .join("\n");
}

function highlightYamlValue(value: string): string {
  if (!value || value.trim() === "") return escapeHtml(value);
  const trimmed = value.trim();
  // Booleans
  if (/^(true|false)$/i.test(trimmed)) {
    return `<span class="text-indigo-400">${escapeHtml(value)}</span>`;
  }
  // Numbers
  if (/^-?\d+(\.\d+)?$/.test(trimmed)) {
    return `<span class="text-amber-400">${escapeHtml(value)}</span>`;
  }
  // Null
  if (/^(null|~)$/i.test(trimmed)) {
    return `<span class="text-slate-500">${escapeHtml(value)}</span>`;
  }
  // Quoted strings
  if (/^["'].*["']$/.test(trimmed)) {
    return `<span class="text-emerald-400">${escapeHtml(value)}</span>`;
  }
  // Unquoted string values
  return `<span class="text-emerald-400">${escapeHtml(value)}</span>`;
}

function escapeHtml(str: string): string {
  return str
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

type HelmHistoryStatusVariant = "deployed" | "failed" | "superseded" | "pending" | "default";

function getHistoryStatusVariant(status: string): HelmHistoryStatusVariant {
  const s = status.toLowerCase();
  if (s === "deployed") return "deployed";
  if (s === "failed") return "failed";
  if (s === "superseded") return "superseded";
  if (s.startsWith("pending")) return "pending";
  return "default";
}

const historyStatusStyles: Record<
  HelmHistoryStatusVariant,
  { dot: string; bg: string; text: string; pulse?: boolean }
> = {
  deployed: {
    dot: "bg-emerald-400",
    bg: "bg-emerald-500/10",
    text: "text-emerald-400",
    pulse: true,
  },
  failed: { dot: "bg-red-400", bg: "bg-red-500/10", text: "text-red-400" },
  superseded: { dot: "bg-slate-400", bg: "bg-slate-500/10", text: "text-slate-400" },
  pending: { dot: "bg-amber-400", bg: "bg-amber-500/10", text: "text-amber-400" },
  default: { dot: "bg-slate-400", bg: "bg-slate-500/10", text: "text-slate-300" },
};

function HistoryStatusBadge({ status }: { status: string }) {
  const variant = getHistoryStatusVariant(status);
  const style = historyStatusStyles[variant];
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1.5 px-2 py-0.5 rounded-full text-xs font-medium",
        style.bg,
        style.text,
      )}
    >
      <span className="relative flex h-1.5 w-1.5">
        {style.pulse && (
          <span
            className={cn(
              "absolute inline-flex h-full w-full rounded-full opacity-75 animate-ping",
              style.dot,
            )}
          />
        )}
        <span className={cn("relative inline-flex rounded-full h-1.5 w-1.5", style.dot)} />
      </span>
      {status}
    </span>
  );
}

export function HelmDetailView({ release, onBack }: HelmDetailViewProps) {
  const [activeTab, setActiveTab] = useState<TabId>("values");
  const [values, setValues] = useState<string>("");
  const [manifest, setManifest] = useState<string>("");
  const [history, setHistory] = useState<HelmRevision[]>([]);
  const [notes, setNotes] = useState<string>("");
  const [loading, setLoading] = useState(true);
  const [showRollbackConfirm, setShowRollbackConfirm] = useState(false);
  const [rollbackTarget, setRollbackTarget] = useState<string | null>(null);
  const [isRollingBack, setIsRollingBack] = useState(false);
  const toast = useToast();

  const { name, namespace } = release;

  // Fetch data based on active tab
  useEffect(() => {
    setLoading(true);

    if (activeTab === "values") {
      getHelmValues(name, namespace)
        .then((data) => {
          setValues(data);
          setLoading(false);
        })
        .catch((err) => {
          setValues(`Error fetching values: ${formatError(err)}`);
          setLoading(false);
        });
    } else if (activeTab === "manifest") {
      getHelmManifest(name, namespace)
        .then((data) => {
          setManifest(data);
          setLoading(false);
        })
        .catch((err) => {
          setManifest(`Error fetching manifest: ${formatError(err)}`);
          setLoading(false);
        });
    } else if (activeTab === "history") {
      getHelmHistory(name, namespace)
        .then((data) => {
          setHistory(data);
          setLoading(false);
        })
        .catch((err) => {
          setHistory([]);
          toast(formatError(err), "error");
          setLoading(false);
        });
    } else if (activeTab === "notes") {
      // Notes are typically part of the manifest output or values;
      // We reuse values as a fallback. If a dedicated endpoint exists, swap it here.
      getHelmValues(name, namespace)
        .then((data) => {
          setNotes(data || "No notes available for this release.");
          setLoading(false);
        })
        .catch(() => {
          setNotes("No notes available for this release.");
          setLoading(false);
        });
    }
  }, [activeTab, name, namespace, toast]);

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

  const handleCopy = useCallback(
    async (content: string) => {
      try {
        await navigator.clipboard.writeText(content);
        toast("Copied to clipboard", "success");
      } catch {
        toast("Failed to copy to clipboard", "error");
      }
    },
    [toast],
  );

  const handleRollbackRequest = (revision: string) => {
    setRollbackTarget(revision);
    setShowRollbackConfirm(true);
  };

  const handleRollback = async () => {
    if (!rollbackTarget) return;
    setIsRollingBack(true);
    try {
      const result = await rollbackHelmRelease(name, namespace, rollbackTarget);
      toast(result || `Rolled back to revision ${rollbackTarget}`, "success");
      setShowRollbackConfirm(false);
      setRollbackTarget(null);
      // Refresh history
      setLoading(true);
      const freshHistory = await getHelmHistory(name, namespace);
      setHistory(freshHistory);
      setLoading(false);
    } catch (err) {
      toast(formatError(err), "error");
    } finally {
      setIsRollingBack(false);
    }
  };

  const tabs: { id: TabId; label: string; icon: typeof FileText }[] = [
    { id: "values", label: "Values", icon: FileText },
    { id: "manifest", label: "Manifest", icon: ScrollText },
    { id: "history", label: "History", icon: History },
    { id: "notes", label: "Notes", icon: StickyNote },
  ];

  return (
    <motion.div
      initial={{ opacity: 0, x: 20 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: 20 }}
      className="h-full w-full flex flex-col bg-background"
    >
      {/* Header */}
      <div className="border-b border-slate-800 p-4 bg-surface/50">
        <div className="flex items-center justify-between mb-4">
          <button
            onClick={onBack}
            aria-label="Go back to Helm releases list"
            className="flex items-center gap-2 px-3 py-1.5 rounded-md border border-slate-800 hover:border-accent/50 hover:bg-muted/30 transition text-sm text-slate-300"
          >
            <ArrowLeft className="w-4 h-4" />
            Back
            <Kbd>Esc</Kbd>
          </button>

          <div className="flex items-center gap-2">
            {(activeTab === "values" || activeTab === "manifest") && (
              <button
                onClick={() => handleCopy(activeTab === "values" ? values : manifest)}
                aria-label="Copy content to clipboard"
                className="flex items-center gap-2 px-3 py-1.5 rounded-md border border-slate-800 hover:border-accent/50 hover:bg-muted/30 transition text-sm text-slate-300"
              >
                <Copy className="w-4 h-4" />
                Copy
              </button>
            )}
          </div>
        </div>

        {/* Release Info */}
        <div className="grid grid-cols-2 md:grid-cols-4 gap-4 text-sm">
          <div>
            <div className="text-slate-500 text-[10px] uppercase tracking-wider mb-1">Release</div>
            <div className="text-slate-100 font-mono text-xs">{name}</div>
          </div>
          <div>
            <div className="text-slate-500 text-[10px] uppercase tracking-wider mb-1">
              Namespace
            </div>
            <div className="text-slate-100 font-mono text-xs">{namespace}</div>
          </div>
        </div>
      </div>

      {/* Tabs */}
      <div className="flex border-b border-slate-800 relative" role="tablist">
        {tabs.map((tab) => {
          const Icon = tab.icon;
          return (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              role="tab"
              aria-selected={activeTab === tab.id}
              className={cn(
                "relative px-4 py-2.5 text-sm transition flex items-center gap-1.5",
                activeTab === tab.id ? "text-accent" : "text-slate-400 hover:text-slate-200",
              )}
            >
              <Icon className="w-3.5 h-3.5" />
              {tab.label}
              {activeTab === tab.id && (
                <motion.div
                  layoutId="helm-detail-tab-indicator"
                  className="absolute bottom-0 left-0 right-0 h-0.5 bg-accent rounded-full"
                  transition={{ type: "spring", stiffness: 500, damping: 35 }}
                />
              )}
            </button>
          );
        })}
      </div>

      {/* Content */}
      <div className="flex-1 overflow-hidden">
        <AnimatePresence mode="wait">
          {activeTab === "values" ? (
            <motion.div
              key="values"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              className="h-full overflow-auto bg-surface/30 border border-slate-800 rounded-lg m-4"
            >
              {loading ? (
                <ContentSkeleton />
              ) : (
                <div className="p-4 font-mono text-xs leading-relaxed">
                  <pre
                    className="whitespace-pre-wrap"
                    dangerouslySetInnerHTML={{
                      __html: highlightYaml(values || "# No values configured"),
                    }}
                  />
                </div>
              )}
            </motion.div>
          ) : activeTab === "manifest" ? (
            <motion.div
              key="manifest"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              className="h-full overflow-auto bg-surface/30 border border-slate-800 rounded-lg m-4"
            >
              {loading ? (
                <ContentSkeleton />
              ) : (
                <div className="p-4 font-mono text-xs leading-relaxed">
                  <pre
                    className="whitespace-pre-wrap"
                    dangerouslySetInnerHTML={{
                      __html: highlightYaml(manifest || "# No manifest available"),
                    }}
                  />
                </div>
              )}
            </motion.div>
          ) : activeTab === "history" ? (
            <motion.div
              key="history"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              className="h-full overflow-auto m-4"
            >
              {loading ? (
                <div className="border border-slate-800 rounded-lg glass">
                  <HistorySkeleton />
                </div>
              ) : history.length === 0 ? (
                <div className="flex flex-col items-center justify-center h-full text-slate-400 gap-3">
                  <History className="w-12 h-12 text-slate-600" />
                  <p className="text-sm font-medium">No revision history found</p>
                </div>
              ) : (
                <div className="border border-slate-800 rounded-lg glass overflow-hidden">
                  <table className="min-w-full text-sm">
                    <thead className="bg-surface/80 sticky top-0 z-10">
                      <tr className="border-b border-slate-800">
                        <th className="px-3 py-2 text-left text-xs font-medium uppercase tracking-wider text-slate-400">
                          Revision
                        </th>
                        <th className="px-3 py-2 text-left text-xs font-medium uppercase tracking-wider text-slate-400">
                          Updated
                        </th>
                        <th className="px-3 py-2 text-left text-xs font-medium uppercase tracking-wider text-slate-400">
                          Status
                        </th>
                        <th className="px-3 py-2 text-left text-xs font-medium uppercase tracking-wider text-slate-400">
                          Chart
                        </th>
                        <th className="px-3 py-2 text-left text-xs font-medium uppercase tracking-wider text-slate-400">
                          App Version
                        </th>
                        <th className="px-3 py-2 text-left text-xs font-medium uppercase tracking-wider text-slate-400">
                          Description
                        </th>
                        <th className="px-3 py-2 text-right text-xs font-medium uppercase tracking-wider text-slate-400">
                          Actions
                        </th>
                      </tr>
                    </thead>
                    <tbody className="divide-y divide-slate-800/50">
                      {history.map((rev, index) => {
                        const isLatest = index === history.length - 1;
                        const isDeployed = rev.status.toLowerCase() === "deployed";
                        return (
                          <tr
                            key={rev.revision}
                            className={cn(
                              "transition",
                              isDeployed ? "bg-emerald-500/5" : "hover:bg-muted/30",
                            )}
                          >
                            <td className="px-3 py-2 text-slate-100 font-mono text-xs">
                              <span className="inline-flex items-center gap-1.5">
                                {rev.revision}
                                {isLatest && (
                                  <span className="px-1.5 py-0.5 rounded text-[10px] bg-accent/10 text-accent font-medium">
                                    latest
                                  </span>
                                )}
                              </span>
                            </td>
                            <td className="px-3 py-2 text-slate-300 text-xs">
                              {(() => {
                                try {
                                  const d = new Date(rev.updated);
                                  if (isNaN(d.getTime())) return rev.updated;
                                  return d.toLocaleString(undefined, {
                                    month: "short",
                                    day: "numeric",
                                    hour: "2-digit",
                                    minute: "2-digit",
                                  });
                                } catch {
                                  return rev.updated;
                                }
                              })()}
                            </td>
                            <td className="px-3 py-2 text-xs">
                              <HistoryStatusBadge status={rev.status} />
                            </td>
                            <td className="px-3 py-2 text-slate-300 font-mono text-xs">
                              {rev.chart}
                            </td>
                            <td className="px-3 py-2 text-slate-400 font-mono text-xs">
                              {rev.app_version || "-"}
                            </td>
                            <td className="px-3 py-2 text-slate-400 text-xs max-w-[200px] truncate">
                              {rev.description || "-"}
                            </td>
                            <td className="px-3 py-2 text-right">
                              {!isDeployed && (
                                <button
                                  onClick={() => handleRollbackRequest(rev.revision)}
                                  disabled={isRollingBack}
                                  className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-md border border-slate-700 hover:border-amber-500/50 hover:bg-amber-500/10 transition text-xs text-slate-300 hover:text-amber-400 disabled:opacity-50 disabled:cursor-not-allowed"
                                >
                                  <RotateCcw className="w-3 h-3" />
                                  Rollback
                                </button>
                              )}
                            </td>
                          </tr>
                        );
                      })}
                    </tbody>
                  </table>
                </div>
              )}
            </motion.div>
          ) : activeTab === "notes" ? (
            <motion.div
              key="notes"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              className="h-full overflow-auto bg-surface/30 border border-slate-800 rounded-lg m-4"
            >
              {loading ? (
                <ContentSkeleton />
              ) : (
                <div className="p-4 font-mono text-xs leading-relaxed text-slate-300">
                  <pre className="whitespace-pre-wrap">{notes}</pre>
                </div>
              )}
            </motion.div>
          ) : null}
        </AnimatePresence>
      </div>

      {/* Rollback Confirmation Dialog */}
      <ConfirmDialog
        open={showRollbackConfirm}
        title="Rollback Helm Release"
        message={`Are you sure you want to rollback "${name}" in namespace "${namespace}" to revision ${rollbackTarget}? This will create a new revision.`}
        confirmText={isRollingBack ? "Rolling back..." : "Rollback"}
        cancelText="Cancel"
        onConfirm={handleRollback}
        onCancel={() => {
          setShowRollbackConfirm(false);
          setRollbackTarget(null);
        }}
        variant="warning"
      />
    </motion.div>
  );
}
