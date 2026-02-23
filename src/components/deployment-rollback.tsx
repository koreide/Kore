import { useEffect, useState, useCallback } from "react";
import { motion, AnimatePresence } from "framer-motion";
import {
  History,
  RotateCcw,
  FileCode2,
  X,
  AlertCircle,
  Clock,
  Loader2,
  Copy,
  ChevronDown,
  ChevronRight,
  Hash,
  Container,
  Users,
  MessageSquare,
} from "lucide-react";
import { listDeploymentRevisions, rollbackDeployment, getRevisionYaml } from "@/lib/api";
import type { DeploymentRevision } from "@/lib/api";
import { formatError } from "@/lib/errors";
import { ConfirmDialog } from "./confirm-dialog";
import { useToast } from "./toast";
import { cn } from "@/lib/utils";

interface DeploymentRollbackProps {
  namespace: string;
  deploymentName: string;
}

function formatTimeAgo(timestamp: string | undefined): string {
  if (!timestamp) return "-";
  try {
    const d = new Date(timestamp);
    const now = new Date();
    const diffS = Math.floor((now.getTime() - d.getTime()) / 1000);
    if (diffS < 60) return `${diffS}s ago`;
    if (diffS < 3600) return `${Math.floor(diffS / 60)}m ago`;
    if (diffS < 86400) return `${Math.floor(diffS / 3600)}h ago`;
    return `${Math.floor(diffS / 86400)}d ago`;
  } catch {
    return "-";
  }
}

function formatTimestamp(timestamp: string | undefined): string {
  if (!timestamp) return "-";
  try {
    return new Date(timestamp).toLocaleString();
  } catch {
    return "-";
  }
}

function highlightYaml(yaml: string): string {
  return yaml
    .split("\n")
    .map((line) => {
      // Comments
      if (/^\s*#/.test(line)) {
        return `<span class="text-slate-500">${escapeHtml(line)}</span>`;
      }
      // Key: value pairs
      const match = line.match(/^(\s*)([\w.-]+)(:)(.*)/);
      if (match) {
        const [, indent, key, colon, value] = match;
        let coloredValue = escapeHtml(value);
        const trimmed = value.trim();
        if (/^["'].*["']$/.test(trimmed)) {
          coloredValue = ` <span class="text-emerald-400">${escapeHtml(trimmed)}</span>`;
        } else if (/^\d+(\.\d+)?$/.test(trimmed)) {
          coloredValue = ` <span class="text-amber-400">${escapeHtml(trimmed)}</span>`;
        } else if (/^(true|false)$/.test(trimmed)) {
          coloredValue = ` <span class="text-indigo-400">${escapeHtml(trimmed)}</span>`;
        } else if (/^(null|~)$/.test(trimmed)) {
          coloredValue = ` <span class="text-slate-500">${escapeHtml(trimmed)}</span>`;
        } else if (trimmed) {
          coloredValue = ` <span class="text-emerald-400">${escapeHtml(trimmed)}</span>`;
        }
        return `${escapeHtml(indent)}<span class="text-accent">${escapeHtml(key)}</span>${escapeHtml(colon)}${coloredValue}`;
      }
      // List items
      const listMatch = line.match(/^(\s*)(- )(.*)/);
      if (listMatch) {
        const [, indent, dash, rest] = listMatch;
        return `${escapeHtml(indent)}<span class="text-slate-500">${escapeHtml(dash)}</span><span class="text-emerald-400">${escapeHtml(rest)}</span>`;
      }
      return escapeHtml(line);
    })
    .join("\n");
}

function escapeHtml(str: string): string {
  return str
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

export function DeploymentRollback({ namespace, deploymentName }: DeploymentRollbackProps) {
  const [revisions, setRevisions] = useState<DeploymentRevision[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [rollbackTarget, setRollbackTarget] = useState<DeploymentRevision | null>(null);
  const [isRollingBack, setIsRollingBack] = useState(false);
  const [yamlTarget, setYamlTarget] = useState<string | null>(null);
  const [yamlContent, setYamlContent] = useState<string>("");
  const [yamlLoading, setYamlLoading] = useState(false);
  const [expandedRevision, setExpandedRevision] = useState<number | null>(null);
  const toast = useToast();

  const fetchRevisions = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await listDeploymentRevisions(namespace, deploymentName);
      // Sort by revision number descending (newest first)
      data.sort((a, b) => b.revision - a.revision);
      setRevisions(data);
    } catch (err) {
      setError(formatError(err));
    } finally {
      setLoading(false);
    }
  }, [namespace, deploymentName]);

  useEffect(() => {
    fetchRevisions();
  }, [fetchRevisions]);

  const handleRollback = async () => {
    if (!rollbackTarget) return;
    setIsRollingBack(true);
    try {
      await rollbackDeployment(namespace, deploymentName, rollbackTarget.name);
      toast(`Rolled back to revision ${rollbackTarget.revision}`, "success");
      setRollbackTarget(null);
      // Refresh the list
      fetchRevisions();
    } catch (err) {
      toast(formatError(err), "error");
    } finally {
      setIsRollingBack(false);
    }
  };

  const handleViewYaml = async (rsName: string) => {
    if (yamlTarget === rsName) {
      setYamlTarget(null);
      setYamlContent("");
      return;
    }
    setYamlTarget(rsName);
    setYamlLoading(true);
    try {
      const yaml = await getRevisionYaml(namespace, rsName);
      setYamlContent(yaml);
    } catch (err) {
      setYamlContent(`Error fetching YAML: ${formatError(err)}`);
    } finally {
      setYamlLoading(false);
    }
  };

  const handleCopyYaml = async () => {
    try {
      await navigator.clipboard.writeText(yamlContent);
      toast("YAML copied to clipboard", "success");
    } catch {
      toast("Failed to copy YAML", "error");
    }
  };

  if (loading) {
    return (
      <div className="p-4 space-y-3">
        {[...Array(3)].map((_, i) => (
          <div key={i} className="skeleton h-20 rounded-lg" style={{ opacity: 1 - i * 0.2 }} />
        ))}
      </div>
    );
  }

  if (error) {
    return (
      <div className="flex items-center gap-3 p-4 text-red-400 text-sm">
        <AlertCircle className="w-4 h-4 shrink-0" />
        <span>Failed to load revisions: {error}</span>
      </div>
    );
  }

  if (revisions.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center h-full text-slate-400 gap-2 py-12">
        <History className="w-8 h-8 text-slate-600" />
        <p className="text-sm">No revisions found</p>
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col">
      {/* Header */}
      <div className="flex items-center justify-between px-4 pt-4 pb-2">
        <div className="flex items-center gap-2">
          <History className="w-4 h-4 text-accent" />
          <h3 className="text-sm font-medium text-slate-200">Revision History</h3>
          <span className="text-[10px] bg-slate-800 text-slate-400 px-1.5 py-0.5 rounded-full">
            {revisions.length}
          </span>
        </div>
      </div>

      {/* Timeline */}
      <div className="flex-1 overflow-auto px-4 pb-4">
        <div className="relative">
          {/* Timeline line */}
          <div className="absolute left-[15px] top-2 bottom-2 w-px bg-slate-800" />

          <div className="space-y-2">
            <AnimatePresence initial={false}>
              {revisions.map((rev, i) => {
                const isCurrent = i === 0;
                const isExpanded = expandedRevision === rev.revision;

                return (
                  <motion.div
                    key={rev.revision}
                    initial={{ opacity: 0, y: 12 }}
                    animate={{ opacity: 1, y: 0 }}
                    exit={{ opacity: 0, y: -12 }}
                    transition={{ duration: 0.2, delay: i * 0.03 }}
                  >
                    <div className={cn("relative pl-9 group")}>
                      {/* Timeline dot */}
                      <div
                        className={cn(
                          "absolute left-[11px] top-3.5 w-[9px] h-[9px] rounded-full border-2 z-10",
                          isCurrent ? "border-accent bg-accent/30" : "border-slate-600 bg-surface",
                        )}
                      />

                      {/* Card */}
                      <div
                        className={cn(
                          "border rounded-lg transition",
                          isCurrent
                            ? "border-accent/30 bg-accent/[0.03]"
                            : "border-slate-800 bg-surface/30 hover:border-slate-700 hover:bg-surface/50",
                        )}
                      >
                        {/* Main row */}
                        <button
                          onClick={() => setExpandedRevision(isExpanded ? null : rev.revision)}
                          className="w-full text-left px-3 py-2.5"
                        >
                          <div className="flex items-center gap-3">
                            {/* Expand indicator */}
                            <div className="text-slate-500">
                              {isExpanded ? (
                                <ChevronDown className="w-3.5 h-3.5" />
                              ) : (
                                <ChevronRight className="w-3.5 h-3.5" />
                              )}
                            </div>

                            {/* Revision number */}
                            <div className="flex items-center gap-1.5 shrink-0">
                              <Hash className="w-3 h-3 text-slate-500" />
                              <span
                                className={cn(
                                  "font-mono text-xs font-medium",
                                  isCurrent ? "text-accent" : "text-slate-300",
                                )}
                              >
                                {rev.revision}
                              </span>
                              {isCurrent && (
                                <span className="text-[9px] uppercase tracking-wider font-medium bg-accent/15 text-accent px-1.5 py-0.5 rounded">
                                  current
                                </span>
                              )}
                            </div>

                            {/* Image (truncated) */}
                            <div className="flex items-center gap-1.5 min-w-0 flex-1">
                              <Container className="w-3 h-3 text-slate-500 shrink-0" />
                              <span className="text-xs text-slate-400 truncate font-mono">
                                {rev.image || "-"}
                              </span>
                            </div>

                            {/* Timestamp */}
                            <span className="text-[10px] text-slate-500 shrink-0">
                              {formatTimeAgo(rev.created)}
                            </span>
                          </div>
                        </button>

                        {/* Expanded details */}
                        <AnimatePresence>
                          {isExpanded && (
                            <motion.div
                              initial={{ height: 0, opacity: 0 }}
                              animate={{ height: "auto", opacity: 1 }}
                              exit={{ height: 0, opacity: 0 }}
                              transition={{ duration: 0.2, ease: "easeInOut" }}
                              className="overflow-hidden"
                            >
                              <div className="px-3 pb-3 pt-1 border-t border-slate-800/50">
                                {/* Detail grid */}
                                <div className="grid grid-cols-2 gap-x-4 gap-y-2 mb-3">
                                  <div>
                                    <div className="text-slate-500 text-[10px] uppercase tracking-wider mb-0.5">
                                      Image
                                    </div>
                                    <div className="flex items-center gap-1.5">
                                      <Container className="w-3 h-3 text-slate-500 shrink-0" />
                                      <span className="text-xs text-slate-200 font-mono break-all">
                                        {rev.image || "-"}
                                      </span>
                                    </div>
                                  </div>
                                  <div>
                                    <div className="text-slate-500 text-[10px] uppercase tracking-wider mb-0.5">
                                      Created
                                    </div>
                                    <div className="flex items-center gap-1.5">
                                      <Clock className="w-3 h-3 text-slate-500 shrink-0" />
                                      <span className="text-xs text-slate-200 font-mono">
                                        {formatTimestamp(rev.created)}
                                      </span>
                                    </div>
                                  </div>
                                  <div>
                                    <div className="text-slate-500 text-[10px] uppercase tracking-wider mb-0.5">
                                      Replicas
                                    </div>
                                    <div className="flex items-center gap-1.5">
                                      <Users className="w-3 h-3 text-slate-500 shrink-0" />
                                      <span className="text-xs text-slate-200 font-mono">
                                        {rev.replicas}
                                      </span>
                                    </div>
                                  </div>
                                  <div>
                                    <div className="text-slate-500 text-[10px] uppercase tracking-wider mb-0.5">
                                      Change Cause
                                    </div>
                                    <div className="flex items-center gap-1.5">
                                      <MessageSquare className="w-3 h-3 text-slate-500 shrink-0" />
                                      <span className="text-xs text-slate-300 break-all">
                                        {rev.change_cause || "No annotation"}
                                      </span>
                                    </div>
                                  </div>
                                </div>

                                {/* Action buttons */}
                                <div className="flex items-center gap-2">
                                  <button
                                    onClick={(e) => {
                                      e.stopPropagation();
                                      handleViewYaml(rev.name);
                                    }}
                                    className={cn(
                                      "flex items-center gap-1.5 px-2.5 py-1.5 rounded-md border text-xs transition",
                                      yamlTarget === rev.name
                                        ? "border-accent/50 bg-accent/10 text-accent"
                                        : "border-slate-800 hover:border-accent/50 hover:bg-muted/30 text-slate-300",
                                    )}
                                  >
                                    <FileCode2 className="w-3.5 h-3.5" />
                                    {yamlTarget === rev.name ? "Hide YAML" : "View YAML"}
                                  </button>

                                  {!isCurrent && (
                                    <button
                                      onClick={(e) => {
                                        e.stopPropagation();
                                        setRollbackTarget(rev);
                                      }}
                                      className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-md border border-amber-800/50 hover:border-amber-600 hover:bg-amber-500/10 text-amber-400 text-xs transition"
                                    >
                                      <RotateCcw className="w-3.5 h-3.5" />
                                      Rollback
                                    </button>
                                  )}
                                </div>

                                {/* Inline YAML viewer */}
                                <AnimatePresence>
                                  {yamlTarget === rev.name && (
                                    <motion.div
                                      initial={{ height: 0, opacity: 0 }}
                                      animate={{
                                        height: "auto",
                                        opacity: 1,
                                      }}
                                      exit={{ height: 0, opacity: 0 }}
                                      transition={{
                                        duration: 0.2,
                                        ease: "easeInOut",
                                      }}
                                      className="overflow-hidden"
                                    >
                                      <div className="mt-2 rounded-md border border-slate-800 bg-black/40 overflow-hidden">
                                        <div className="flex items-center justify-between px-3 py-1.5 border-b border-slate-800 bg-surface/30">
                                          <span className="text-[10px] text-slate-500 uppercase tracking-wider">
                                            ReplicaSet YAML
                                          </span>
                                          <div className="flex items-center gap-1">
                                            <button
                                              onClick={handleCopyYaml}
                                              className="text-slate-500 hover:text-slate-300 transition p-1"
                                              title="Copy YAML"
                                            >
                                              <Copy className="w-3 h-3" />
                                            </button>
                                            <button
                                              onClick={() => {
                                                setYamlTarget(null);
                                                setYamlContent("");
                                              }}
                                              className="text-slate-500 hover:text-slate-300 transition p-1"
                                              title="Close"
                                            >
                                              <X className="w-3 h-3" />
                                            </button>
                                          </div>
                                        </div>
                                        {yamlLoading ? (
                                          <div className="flex items-center justify-center py-8">
                                            <Loader2 className="w-4 h-4 text-accent animate-spin" />
                                          </div>
                                        ) : (
                                          <div className="max-h-64 overflow-auto p-3">
                                            <pre
                                              className="font-mono text-xs leading-relaxed whitespace-pre-wrap"
                                              dangerouslySetInnerHTML={{
                                                __html: highlightYaml(yamlContent),
                                              }}
                                            />
                                          </div>
                                        )}
                                      </div>
                                    </motion.div>
                                  )}
                                </AnimatePresence>
                              </div>
                            </motion.div>
                          )}
                        </AnimatePresence>
                      </div>
                    </div>
                  </motion.div>
                );
              })}
            </AnimatePresence>
          </div>
        </div>
      </div>

      {/* Rollback Confirmation */}
      <ConfirmDialog
        open={rollbackTarget !== null}
        title="Rollback Deployment"
        message={
          rollbackTarget
            ? `Roll back "${deploymentName}" to revision ${rollbackTarget.revision}? This will replace the current pod template with the one from this revision.`
            : ""
        }
        confirmText={isRollingBack ? "Rolling back..." : "Rollback"}
        cancelText="Cancel"
        onConfirm={handleRollback}
        onCancel={() => setRollbackTarget(null)}
        variant="warning"
      />
    </div>
  );
}
