import { useEffect, useRef, useState, useMemo, useCallback } from "react";
import { List as VirtualList } from "react-window";
import type { ListImperativeAPI, RowComponentProps } from "react-window";
import { motion, AnimatePresence } from "framer-motion";
import {
  ArrowLeft,
  Copy,
  Download,
  AlertCircle,
  Trash2,
  Search,
  ChevronUp,
  ChevronDown,
  X,
} from "lucide-react";
import {
  deleteResource,
  describePod,
  getResourceYaml,
  startPodLogsStream,
  stopPodLogsStream,
} from "@/lib/api";
import { formatError } from "@/lib/errors";
import type { ResourceItem, WatchEventPayload } from "@/lib/types";
import { listen } from "@tauri-apps/api/event";
import { PodMetrics } from "./pod-metrics";
import { ConfirmDialog } from "./confirm-dialog";
import { PortForwarding } from "./port-forwarding";
import { EventsTimeline } from "./events-timeline";
import { ExecTerminal } from "./exec-terminal";
import { YamlEditor } from "./yaml-editor";
import { DescribeContent } from "./describe-content";
import { useToast } from "./toast";
import { cn } from "@/lib/utils";

interface PodDetailsViewProps {
  pod: ResourceItem;
  onBack: () => void;
}

type StatusVariant = "running" | "pending" | "failed" | "terminating" | "default" | "deleted";

function getStatusVariant(status: string): StatusVariant {
  const s = status.toLowerCase();
  if (s === "running" || s === "ready" || s === "succeeded" || s === "completed") return "running";
  if (s === "pending" || s === "containercreating" || s === "init" || s === "waiting")
    return "pending";
  if (s === "failed" || s === "crashloopbackoff" || s === "error" || s === "imagepullbackoff")
    return "failed";
  if (s === "terminating") return "terminating";
  if (s === "deleted") return "deleted";
  return "default";
}

const statusColors: Record<
  StatusVariant,
  { dot: string; text: string; bg: string; pulse?: boolean }
> = {
  running: {
    dot: "bg-emerald-400",
    text: "text-emerald-400",
    bg: "bg-emerald-500/10",
    pulse: true,
  },
  pending: { dot: "bg-amber-400", text: "text-amber-400", bg: "bg-amber-500/10" },
  failed: { dot: "bg-red-400", text: "text-red-400", bg: "bg-red-500/10" },
  terminating: { dot: "bg-orange-400", text: "text-orange-400", bg: "bg-orange-500/10" },
  deleted: { dot: "bg-red-400", text: "text-red-400", bg: "bg-red-500/10" },
  default: { dot: "bg-slate-400", text: "text-slate-300", bg: "bg-slate-500/10" },
};

function StatusBadge({ status }: { status: string }) {
  const variant = getStatusVariant(status);
  const style = statusColors[variant];
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

function Kbd({ children }: { children: React.ReactNode }) {
  return (
    <kbd className="px-1.5 py-0.5 bg-slate-800/80 rounded text-[10px] text-slate-400 font-mono border border-slate-700/50 ml-1.5">
      {children}
    </kbd>
  );
}

function LogSkeleton() {
  const widths = [
    "80%",
    "65%",
    "90%",
    "45%",
    "75%",
    "55%",
    "85%",
    "40%",
    "70%",
    "60%",
    "88%",
    "50%",
  ];
  return (
    <div className="p-4 space-y-2.5">
      {widths.map((w, i) => (
        <div key={i} className="skeleton h-3" style={{ width: w, opacity: 1 - i * 0.06 }} />
      ))}
    </div>
  );
}

function DescribeSkeleton() {
  return (
    <div className="p-4 space-y-3">
      <div className="skeleton h-4 w-32" />
      <div className="space-y-2 ml-4">
        <div className="skeleton h-3 w-48" />
        <div className="skeleton h-3 w-56" />
        <div className="skeleton h-3 w-40" />
      </div>
      <div className="skeleton h-4 w-28 mt-4" />
      <div className="space-y-2 ml-4">
        <div className="skeleton h-3 w-64" />
        <div className="skeleton h-3 w-52" />
        <div className="skeleton h-3 w-44" />
        <div className="skeleton h-3 w-60" />
      </div>
    </div>
  );
}

/** Detect log level and return color class */
function getLogLevelColor(line: string): string | null {
  const upper = line.toUpperCase();
  if (/\bERROR\b|\bFATAL\b|\bPANIC\b/.test(upper)) return "text-red-400";
  if (/\bWARN(ING)?\b/.test(upper)) return "text-amber-400";
  if (/\bINFO\b/.test(upper)) return "text-blue-400";
  if (/\bDEBUG\b|\bTRACE\b/.test(upper)) return "text-slate-500";
  return null;
}

/** Highlight search matches in a string */
function highlightMatches(text: string, query: string): React.ReactNode {
  if (!query) return text;
  const escapedQuery = query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const regex = new RegExp(`(${escapedQuery})`, "gi");
  const parts = text.split(regex);
  return parts.map((part, i) =>
    regex.test(part) ? (
      <mark key={i} className="bg-amber-400/30 text-amber-200 rounded-sm px-0.5">
        {part}
      </mark>
    ) : (
      part
    ),
  );
}

interface LogRowProps {
  logLines: string[];
  logSearch: string;
  logSearchMatches: number[];
  logSearchIndex: number;
}

function LogRow(props: RowComponentProps<LogRowProps>) {
  const { index, style, logLines, logSearch, logSearchMatches, logSearchIndex } = props;
  const line = logLines[index];
  const levelColor = getLogLevelColor(line);
  const isSearchMatch =
    logSearch && line.toLowerCase().includes(logSearch.toLowerCase());
  const isCurrentMatch =
    logSearchMatches.length > 0 && logSearchMatches[logSearchIndex] === index;

  return (
    <div
      style={style}
      data-line={index}
      className={cn(
        "flex hover:bg-white/[0.02] group",
        isCurrentMatch && "bg-amber-400/10",
        isSearchMatch && !isCurrentMatch && "bg-amber-400/5",
      )}
    >
      <span className="select-none text-right text-slate-600 w-12 shrink-0 pr-3 py-px border-r border-slate-800/50 group-hover:text-slate-500">
        {index + 1}
      </span>
      <span
        className={cn(
          "pl-3 py-px whitespace-pre-wrap break-all flex-1",
          levelColor || "text-green-400",
        )}
      >
        {logSearch ? highlightMatches(line, logSearch) : line}
      </span>
    </div>
  );
}

export function PodDetailsView({ pod, onBack }: PodDetailsViewProps) {
  const [activeTab, setActiveTab] = useState<
    "logs" | "describe" | "yaml" | "metrics" | "events" | "shell"
  >("logs");
  const [logs, setLogs] = useState<string>("");
  const [describe, setDescribe] = useState<string>("");
  const [autoScroll, setAutoScroll] = useState(true);
  const [loading, setLoading] = useState(true);
  const [isDeleted, setIsDeleted] = useState(false);
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false);
  const [isDeleting, setIsDeleting] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  // Container selector
  const [containers, setContainers] = useState<string[]>([]);
  const [selectedContainer, setSelectedContainer] = useState<string | undefined>(undefined);
  // Previous logs toggle
  const [showPrevious, setShowPrevious] = useState(false);
  // Log search
  const [logSearch, setLogSearch] = useState("");
  const [logSearchVisible, setLogSearchVisible] = useState(false);
  const [logSearchIndex, setLogSearchIndex] = useState(0);
  const logSearchInputRef = useRef<HTMLInputElement>(null);

  const logsContainerRef = useRef<HTMLDivElement>(null);
  const virtualListRef = useRef<ListImperativeAPI>(null);
  const focusTimeoutRef = useRef<ReturnType<typeof setTimeout>>();
  const toast = useToast();

  const podName = pod.name || "";
  const namespace = pod.namespace || "default";
  const status = isDeleted ? "Deleted" : pod.status || "Unknown";
  const node = pod.node || "Unknown";
  const ip = pod.ip || "Unknown";

  const logLines = useMemo(() => {
    if (!logs) return [];
    return logs.split("\n");
  }, [logs]);

  const logSearchMatches = useMemo(() => {
    if (!logSearch) return [];
    const matches: number[] = [];
    const query = logSearch.toLowerCase();
    logLines.forEach((line, i) => {
      if (line.toLowerCase().includes(query)) matches.push(i);
    });
    return matches;
  }, [logLines, logSearch]);

  // Fetch describe data + extract container names
  useEffect(() => {
    if (!isDeleted) {
      describePod(namespace, podName)
        .then((podData) => {
          setDescribe(JSON.stringify(podData, null, 2));
          // Extract container names
          const spec = podData.spec as Record<string, unknown> | undefined;
          const containerList = spec?.containers as Array<Record<string, unknown>> | undefined;
          const initContainers = spec?.initContainers as Array<Record<string, unknown>> | undefined;
          const names: string[] = [];
          if (initContainers) {
            initContainers.forEach((c) => {
              if (c.name) names.push(`init:${c.name as string}`);
            });
          }
          if (containerList) {
            containerList.forEach((c) => {
              if (c.name) names.push(c.name as string);
            });
          }
          setContainers(names);
          if (activeTab === "describe") setLoading(false);
        })
        .catch((err) => {
          setDescribe(`Error fetching pod details: ${err}`);
          if (activeTab === "describe") setLoading(false);
        });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [namespace, podName, isDeleted]);

  // Setup logs streaming with container + previous support
  useEffect(() => {
    if (activeTab !== "logs" || !podName || !namespace || isDeleted) {
      if (isDeleted && activeTab === "logs") {
        setLogs("Pod has been deleted. Logs are no longer available.");
        setLoading(false);
      }
      return;
    }

    setLoading(true);
    setLogs("");
    let isMounted = true;

    // Resolve container name (strip init: prefix for the API call)
    const containerName = selectedContainer ? selectedContainer.replace(/^init:/, "") : undefined;

    const eventName = `pod-logs://${namespace}/${podName}`;
    let unlistenFn: (() => void) | null = null;

    const setupStream = async () => {
      try {
        await startPodLogsStream(namespace, podName, containerName, showPrevious);

        const unlisten = await listen<{ logs?: string; error?: string; append?: boolean }>(
          eventName,
          (event) => {
            if (!isMounted) return;
            const payload = event.payload;
            if (payload.error) {
              setLogs((prev) => prev + `\n[ERROR] ${payload.error}\n`);
              setLoading(false);
            } else if (payload.logs) {
              setLogs((prev) => {
                if (payload.append) {
                  return prev + payload.logs;
                }
                return payload.logs || "";
              });
              setLoading(false);
            }
          },
        );

        if (!isMounted) {
          unlisten();
          return;
        }
        unlistenFn = unlisten;
      } catch (err) {
        if (!isMounted) return;
        setLogs(`Error starting log stream: ${err}`);
        setLoading(false);
      }
    };

    setupStream();

    return () => {
      isMounted = false;
      if (unlistenFn) {
        unlistenFn();
      }
      stopPodLogsStream().catch(() => {});
    };
  }, [activeTab, namespace, podName, isDeleted, selectedContainer, showPrevious]);

  // Auto-scroll logs (for virtualized list)
  useEffect(() => {
    if (autoScroll && virtualListRef.current && logLines.length > 0) {
      virtualListRef.current.scrollToRow({ index: logLines.length - 1, align: "end" });
    }
  }, [logs, autoScroll, logLines.length]);


  // Listen for pod deletion events
  useEffect(() => {
    let unlistenFn: (() => void) | null = null;
    let isMounted = true;

    const setupListener = async () => {
      const unlisten = await listen<WatchEventPayload>("resource://event", (event) => {
        if (!isMounted) return;
        const payload = event.payload;

        if (payload.kind === "pods" && payload.action === "deleted") {
          const metadata = payload.object.metadata ?? {};
          const eventPodName = (metadata.name as string) ?? "";
          const eventNamespace = (metadata.namespace as string) ?? "";

          if (eventPodName === podName && eventNamespace === namespace) {
            setIsDeleted(true);
          }
        }
      });

      if (!isMounted) {
        unlisten();
        return;
      }
      unlistenFn = unlisten;
    };

    setupListener().catch((err) => {
      console.error("Failed to set up pod deletion listener", err);
    });

    return () => {
      isMounted = false;
      if (unlistenFn) {
        unlistenFn();
      }
    };
  }, [podName, namespace]);

  // Handle keyboard shortcuts
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        if (logSearchVisible) {
          setLogSearchVisible(false);
          setLogSearch("");
          return;
        }
        onBack();
      }
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "f" && activeTab === "logs") {
        e.preventDefault();
        setLogSearchVisible(true);
        if (focusTimeoutRef.current) clearTimeout(focusTimeoutRef.current);
        focusTimeoutRef.current = setTimeout(() => logSearchInputRef.current?.focus(), 50);
      }
      if (e.key.toLowerCase() === "d" && !isDeleted && !showDeleteConfirm) {
        const target = e.target as HTMLElement;
        if (target.tagName !== "INPUT" && target.tagName !== "TEXTAREA") {
          e.preventDefault();
          setShowDeleteConfirm(true);
        }
      }
      // Number key tab switching (1-6)
      const tabKeys = ["1", "2", "3", "4", "5", "6"];
      const tabIds = ["logs", "describe", "yaml", "metrics", "events", "shell"] as const;
      const idx = tabKeys.indexOf(e.key);
      if (idx >= 0 && idx < tabIds.length) {
        const target = e.target as HTMLElement;
        if (target.tagName !== "INPUT" && target.tagName !== "TEXTAREA") {
          e.preventDefault();
          setActiveTab(tabIds[idx]);
        }
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onBack, isDeleted, showDeleteConfirm, logSearchVisible, activeTab]);

  // Clean up focus timeout on unmount
  useEffect(() => {
    return () => {
      if (focusTimeoutRef.current) clearTimeout(focusTimeoutRef.current);
    };
  }, []);

  const handleCopyLogs = async () => {
    try {
      await navigator.clipboard.writeText(logs);
      toast("Copied to clipboard", "success");
    } catch (err) {
      console.error("Failed to copy logs", err);
      toast("Failed to copy logs", "error");
    }
  };

  const handleDownloadLogs = () => {
    const timestamp = new Date().toISOString().replace(/[:.]/g, "-");
    const filename = `${podName}-${timestamp}.log`;
    const blob = new Blob([logs], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = filename;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
    toast("Logs downloaded", "success");
  };

  const handleCopyYaml = async () => {
    try {
      const yaml = await getResourceYaml("pods", namespace, podName);
      await navigator.clipboard.writeText(yaml);
      toast("YAML copied to clipboard", "success");
    } catch (err) {
      console.error("Failed to copy YAML", err);
      toast("Failed to copy YAML", "error");
    }
  };

  const handleDelete = async () => {
    if (!podName || !namespace || isDeleted) return;

    setIsDeleting(true);
    setDeleteError(null);
    try {
      await deleteResource({ kind: "pods", namespace, name: podName });
      setShowDeleteConfirm(false);
    } catch (err) {
      console.error("Failed to delete pod", err);
      const msg = formatError(err);
      setDeleteError(msg);
      toast(msg, "error");
    } finally {
      setIsDeleting(false);
    }
  };

  const navigateLogSearch = useCallback(
    (direction: "next" | "prev") => {
      if (logSearchMatches.length === 0) return;
      let idx = logSearchIndex;
      if (direction === "next") {
        idx = (idx + 1) % logSearchMatches.length;
      } else {
        idx = (idx - 1 + logSearchMatches.length) % logSearchMatches.length;
      }
      setLogSearchIndex(idx);
      // Scroll to match using VirtualList
      const lineIndex = logSearchMatches[idx];
      if (virtualListRef.current) {
        virtualListRef.current.scrollToRow({ index: lineIndex, align: "center" });
      } else {
        // Fallback for non-virtualized rendering
        const el = logsContainerRef.current?.querySelector(`[data-line="${lineIndex}"]`);
        el?.scrollIntoView({ behavior: "smooth", block: "center" });
      }
    },
    [logSearchMatches, logSearchIndex],
  );

  const tabs = [
    { id: "logs" as const, label: "Logs" },
    { id: "describe" as const, label: "Describe" },
    { id: "yaml" as const, label: "YAML" },
    { id: "metrics" as const, label: "Metrics" },
    { id: "events" as const, label: "Events" },
    { id: "shell" as const, label: "Shell" },
  ];

  return (
    <motion.div
      initial={{ opacity: 0, x: 20 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: 20 }}
      className="h-full w-full flex bg-background"
    >
      {/* Main Content */}
      <div className="flex-1 flex flex-col min-w-0">
        {/* Header */}
        <div className="border-b border-slate-800 p-4 bg-surface/50">
          <div className="flex items-center justify-between mb-4">
            <button
              onClick={onBack}
              aria-label="Go back to resource list"
              className="flex items-center gap-2 px-3 py-1.5 rounded-md border border-slate-800 hover:border-accent/50 hover:bg-muted/30 transition text-sm text-slate-300"
            >
              <ArrowLeft className="w-4 h-4" />
              Back
              <Kbd>Esc</Kbd>
            </button>
            <div className="flex items-center gap-2">
              {!isDeleted && (
                <>
                  <button
                    onClick={handleCopyYaml}
                    aria-label="Copy pod YAML to clipboard"
                    className="flex items-center gap-2 px-3 py-1.5 rounded-md border border-slate-800 hover:border-accent/50 hover:bg-muted/30 transition text-sm text-slate-300"
                  >
                    <Copy className="w-4 h-4" />
                    Copy YAML
                  </button>
                  <button
                    onClick={() => setShowDeleteConfirm(true)}
                    disabled={isDeleting}
                    aria-label="Delete pod"
                    className="flex items-center gap-2 px-3 py-1.5 rounded-md border border-red-800/50 hover:border-red-600 hover:bg-red-500/15 transition text-sm text-red-400 disabled:opacity-50 disabled:cursor-not-allowed"
                  >
                    <Trash2 className="w-4 h-4" />
                    Delete
                    <Kbd>D</Kbd>
                  </button>
                </>
              )}
              {activeTab === "logs" && (
                <>
                  {/* Container selector */}
                  {containers.length > 1 && (
                    <select
                      value={selectedContainer || ""}
                      onChange={(e) => setSelectedContainer(e.target.value || undefined)}
                      className="px-2 py-1.5 rounded-md border border-slate-800 bg-surface/60 text-xs text-slate-300 outline-none"
                    >
                      <option value="">All containers</option>
                      {containers.map((c) => (
                        <option key={c} value={c}>
                          {c}
                        </option>
                      ))}
                    </select>
                  )}
                  {/* Previous toggle */}
                  <label className="flex items-center gap-2 px-3 py-1.5 rounded-md border border-slate-800 hover:border-accent/50 hover:bg-muted/30 transition text-sm cursor-pointer text-slate-300">
                    <input
                      type="checkbox"
                      checked={showPrevious}
                      onChange={(e) => setShowPrevious(e.target.checked)}
                      className="w-3.5 h-3.5"
                    />
                    Previous
                  </label>
                  <button
                    onClick={handleCopyLogs}
                    aria-label="Copy logs to clipboard"
                    className="flex items-center gap-2 px-3 py-1.5 rounded-md border border-slate-800 hover:border-accent/50 hover:bg-muted/30 transition text-sm text-slate-300"
                  >
                    <Copy className="w-4 h-4" />
                    Copy
                  </button>
                  <button
                    onClick={handleDownloadLogs}
                    aria-label="Download logs as file"
                    className="flex items-center gap-2 px-3 py-1.5 rounded-md border border-slate-800 hover:border-accent/50 hover:bg-muted/30 transition text-sm text-slate-300"
                  >
                    <Download className="w-4 h-4" />
                    Download
                  </button>
                  <label className="flex items-center gap-2 px-3 py-1.5 rounded-md border border-slate-800 hover:border-accent/50 hover:bg-muted/30 transition text-sm cursor-pointer text-slate-300">
                    <input
                      type="checkbox"
                      checked={autoScroll}
                      onChange={(e) => setAutoScroll(e.target.checked)}
                      className="w-3.5 h-3.5"
                    />
                    Auto-scroll
                  </label>
                </>
              )}
            </div>
          </div>

          {/* Pod Info */}
          <div className="grid grid-cols-2 md:grid-cols-5 gap-4 text-sm">
            <div>
              <div className="text-slate-500 text-[10px] uppercase tracking-wider mb-1">Name</div>
              <div className="text-slate-100 font-mono text-xs">{podName}</div>
            </div>
            <div>
              <div className="text-slate-500 text-[10px] uppercase tracking-wider mb-1">
                Namespace
              </div>
              <div className="text-slate-100 font-mono text-xs">{namespace}</div>
            </div>
            <div>
              <div className="text-slate-500 text-[10px] uppercase tracking-wider mb-1">Status</div>
              <StatusBadge status={status} />
            </div>
            <div>
              <div className="text-slate-500 text-[10px] uppercase tracking-wider mb-1">Node</div>
              <div className="text-slate-100 font-mono text-xs">{node}</div>
            </div>
            <div>
              <div className="text-slate-500 text-[10px] uppercase tracking-wider mb-1">IP</div>
              <div className="text-slate-100 font-mono text-xs">{ip}</div>
            </div>
          </div>
        </div>

        {/* Tabs */}
        {!isDeleted && (
          <div className="flex border-b border-slate-800 relative" role="tablist">
            {tabs.map((tab) => (
              <button
                key={tab.id}
                onClick={() => setActiveTab(tab.id)}
                role="tab"
                aria-selected={activeTab === tab.id}
                className={cn(
                  "relative px-4 py-2.5 text-sm transition",
                  activeTab === tab.id ? "text-accent" : "text-slate-400 hover:text-slate-200",
                )}
              >
                {tab.label}
                {activeTab === tab.id && (
                  <motion.div
                    layoutId="pod-detail-tab-indicator"
                    className="absolute bottom-0 left-0 right-0 h-0.5 bg-accent rounded-full"
                    transition={{ type: "spring", stiffness: 500, damping: 35 }}
                  />
                )}
              </button>
            ))}
          </div>
        )}

        {/* Content */}
        <div className="flex-1 overflow-hidden relative">
          {/* Log search overlay */}
          {logSearchVisible && activeTab === "logs" && (
            <div className="absolute top-2 right-6 z-20 flex items-center gap-1 bg-surface border border-slate-700 rounded-lg px-3 py-1.5 shadow-lg">
              <Search className="w-3.5 h-3.5 text-slate-500" />
              <input
                ref={logSearchInputRef}
                value={logSearch}
                onChange={(e) => {
                  setLogSearch(e.target.value);
                  setLogSearchIndex(0);
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    navigateLogSearch(e.shiftKey ? "prev" : "next");
                  }
                  if (e.key === "Escape") {
                    setLogSearchVisible(false);
                    setLogSearch("");
                  }
                }}
                placeholder="Search logs..."
                className="bg-transparent text-xs text-slate-200 outline-none w-40 placeholder:text-slate-600"
              />
              {logSearchMatches.length > 0 && (
                <span className="text-[10px] text-slate-500 mx-1">
                  {logSearchIndex + 1}/{logSearchMatches.length}
                </span>
              )}
              <button
                onClick={() => navigateLogSearch("prev")}
                className="text-slate-400 hover:text-slate-200"
              >
                <ChevronUp className="w-3.5 h-3.5" />
              </button>
              <button
                onClick={() => navigateLogSearch("next")}
                className="text-slate-400 hover:text-slate-200"
              >
                <ChevronDown className="w-3.5 h-3.5" />
              </button>
              <button
                onClick={() => {
                  setLogSearchVisible(false);
                  setLogSearch("");
                }}
                className="text-slate-400 hover:text-slate-200 ml-1"
              >
                <X className="w-3.5 h-3.5" />
              </button>
            </div>
          )}

          {isDeleted ? (
            <motion.div
              initial={{ opacity: 0, y: 20 }}
              animate={{ opacity: 1, y: 0 }}
              className="h-full flex items-center justify-center"
            >
              <div className="text-center p-8 bg-surface/30 border border-red-500/50 rounded-lg m-4 max-w-md">
                <AlertCircle className="w-12 h-12 text-red-400 mx-auto mb-4" />
                <h3 className="text-xl font-semibold text-red-400 mb-2">Pod No Longer Exists</h3>
                <p className="text-slate-300 mb-4">
                  The pod <span className="font-mono text-accent">{podName}</span> in namespace{" "}
                  <span className="font-mono text-accent">{namespace}</span> has been deleted.
                </p>
                <button
                  onClick={onBack}
                  className="px-4 py-2 rounded-md border border-slate-800 hover:border-accent/50 hover:bg-muted/30 transition text-sm"
                >
                  Go Back
                </button>
              </div>
            </motion.div>
          ) : (
            <AnimatePresence mode="wait">
              {activeTab === "logs" ? (
                <motion.div
                  key="logs"
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0 }}
                  className="h-full bg-black/40 border border-slate-800 rounded-lg m-4"
                >
                  {loading && logs === "" ? (
                    <LogSkeleton />
                  ) : (
                    <div className="h-full font-mono text-xs leading-relaxed">
                      {logLines.length > 0 ? (
                        <VirtualList
                          listRef={virtualListRef}
                          rowCount={logLines.length}
                          rowHeight={20}
                          style={{ fontFamily: "inherit" }}
                          overscanCount={20}
                          rowComponent={LogRow}
                          rowProps={{
                            logLines,
                            logSearch,
                            logSearchMatches,
                            logSearchIndex,
                          }}
                        />
                      ) : (
                        <div className="p-4 text-slate-500">No logs available</div>
                      )}
                    </div>
                  )}
                </motion.div>
              ) : activeTab === "describe" ? (
                <motion.div
                  key="describe"
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0 }}
                  className="h-full bg-surface/30 border border-slate-800 rounded-lg m-4 overflow-hidden"
                >
                  {loading ? <DescribeSkeleton /> : <DescribeContent content={describe} />}
                </motion.div>
              ) : activeTab === "yaml" ? (
                <motion.div
                  key="yaml"
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0 }}
                  className="h-full overflow-auto"
                >
                  <YamlEditor kind="pods" namespace={namespace} name={podName} />
                </motion.div>
              ) : activeTab === "metrics" ? (
                <motion.div
                  key="metrics"
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0 }}
                  className="h-full overflow-hidden"
                >
                  <PodMetrics namespace={namespace} podName={podName} />
                </motion.div>
              ) : activeTab === "events" ? (
                <motion.div
                  key="events"
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0 }}
                  className="h-full overflow-auto"
                >
                  <EventsTimeline kind="pods" namespace={namespace} name={podName} />
                </motion.div>
              ) : activeTab === "shell" ? (
                <motion.div
                  key="shell"
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0 }}
                  className="h-full overflow-hidden"
                >
                  <ExecTerminal
                    namespace={namespace}
                    podName={podName}
                    container={
                      selectedContainer ? selectedContainer.replace(/^init:/, "") : undefined
                    }
                  />
                </motion.div>
              ) : null}
            </AnimatePresence>
          )}
        </div>

        {/* Delete Error */}
        {deleteError && (
          <div className="mx-4 mb-2 p-2 bg-red-500/10 border border-red-500/50 rounded text-xs text-red-400">
            Failed to delete pod: {deleteError}
          </div>
        )}

        {/* Delete Confirmation Dialog */}
        <ConfirmDialog
          open={showDeleteConfirm}
          title="Delete Pod"
          message={`Are you sure you want to delete pod "${podName}" in namespace "${namespace}"? This action cannot be undone.`}
          confirmText="Delete"
          cancelText="Cancel"
          onConfirm={handleDelete}
          onCancel={() => setShowDeleteConfirm(false)}
          variant="danger"
        />
      </div>

      {/* Port Forwarding Sidebar */}
      {!isDeleted && activeTab !== "shell" && (
        <PortForwarding namespace={namespace} podName={podName} />
      )}
    </motion.div>
  );
}
