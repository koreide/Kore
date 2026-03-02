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
  ChevronRight,
  X,
  Bug,
  Plug,
} from "lucide-react";
import {
  deleteResource,
  describePod,
  listDebugContainers,
  startPodLogsStream,
  stopDebugContainer,
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
import { DebugContainerModal } from "./debug-container-modal";
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
  const isSearchMatch = logSearch && line.toLowerCase().includes(logSearch.toLowerCase());
  const isCurrentMatch = logSearchMatches.length > 0 && logSearchMatches[logSearchIndex] === index;

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
  // Debug container
  const [showDebugModal, setShowDebugModal] = useState(false);
  const [debugContainer, setDebugContainer] = useState<{ name: string; image: string } | undefined>(undefined);
  const [stoppingDebug, setStoppingDebug] = useState(false);
  const [isStaticPod, setIsStaticPod] = useState(false);
  const [showPodInfo, setShowPodInfo] = useState(false);
  const [showPortForward, setShowPortForward] = useState(false);
  const [activePortForwards, setActivePortForwards] = useState(0);

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
          // Detect static pods (mirror pods created by kubelet)
          const metadata = podData.metadata as Record<string, unknown> | undefined;
          const annotations = metadata?.annotations as Record<string, string> | undefined;
          if (annotations?.["kubernetes.io/config.mirror"]) {
            setIsStaticPod(true);
          }
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
          // Restore debug container state if one is running
          listDebugContainers(namespace, podName)
            .then((debugContainers) => {
              const running = debugContainers.filter((dc) => dc.running);
              if (running.length > 0) {
                const last = running[running.length - 1];
                setDebugContainer({ name: last.name, image: last.image });
              }
            })
            .catch(() => {});
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

  const handleStopDebug = useCallback(async () => {
    const dc = debugContainer;
    if (!dc) return;
    setStoppingDebug(true);
    try {
      await stopDebugContainer(namespace, podName, dc.name);
      setDebugContainer(undefined);
      toast("Debug container stopped", "success");
    } catch (err) {
      toast(`Failed to stop debug container: ${formatError(err)}`, "error");
    } finally {
      setStoppingDebug(false);
    }
  }, [debugContainer, namespace, podName, toast]);

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
      if (e.key.toLowerCase() === "d" && !isDeleted && !showDeleteConfirm && !showDebugModal) {
        const target = e.target as HTMLElement;
        if (target.tagName !== "INPUT" && target.tagName !== "TEXTAREA") {
          e.preventDefault();
          setShowDeleteConfirm(true);
        }
      }
      if (e.key.toLowerCase() === "b" && !isDeleted && !showDeleteConfirm && !showDebugModal) {
        const target = e.target as HTMLElement;
        if (target.tagName !== "INPUT" && target.tagName !== "TEXTAREA") {
          e.preventDefault();
          if (debugContainer) {
            handleStopDebug();
          } else if (!isStaticPod) {
            setShowDebugModal(true);
          }
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
  }, [onBack, isDeleted, isStaticPod, showDeleteConfirm, showDebugModal, logSearchVisible, activeTab, debugContainer, handleStopDebug]);

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

  const handleDebugReady = (containerName: string, image: string) => {
    setDebugContainer({ name: containerName, image });
    setShowDebugModal(false);
    setActiveTab("shell");
    toast(`Debug container "${containerName}" is ready`, "success");
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
        <div className="border-b border-slate-800 bg-surface/50">
          <div className="flex items-center gap-3 px-4 py-2.5">
            {/* Left: Back button */}
            <button
              onClick={onBack}
              aria-label="Go back to resource list"
              className="flex items-center gap-1.5 px-2.5 py-1 rounded-md border border-slate-800 hover:border-accent/50 hover:bg-muted/30 transition text-sm text-slate-300 shrink-0"
            >
              <ArrowLeft className="w-3.5 h-3.5" />
              <Kbd>Esc</Kbd>
            </button>

            {/* Center: Pod name + status */}
            <div className="flex items-center gap-2.5 min-w-0 flex-1">
              <span className="text-slate-100 font-mono text-sm truncate">{podName}</span>
              <StatusBadge status={status} />
              <button
                onClick={() => setShowPodInfo(!showPodInfo)}
                className="flex items-center gap-1 text-xs text-slate-500 hover:text-slate-300 transition shrink-0"
                aria-label={showPodInfo ? "Hide pod details" : "Show pod details"}
              >
                <motion.span
                  animate={{ rotate: showPodInfo ? 90 : 0 }}
                  transition={{ duration: 0.15 }}
                  className="inline-flex"
                >
                  <ChevronRight className="w-3.5 h-3.5" />
                </motion.span>
                <span className="hidden sm:inline">details</span>
              </button>
            </div>

            {/* Right: Pod-level actions */}
            {!isDeleted && (
              <div className="flex items-center gap-1.5 shrink-0">
                <button
                  onClick={() => {
                    if (debugContainer) {
                      handleStopDebug();
                    } else if (!isStaticPod) {
                      setShowDebugModal(true);
                    }
                  }}
                  disabled={(isStaticPod && !debugContainer) || stoppingDebug}
                  title={
                    debugContainer
                      ? `Stop debug container "${debugContainer.name}"`
                      : isStaticPod
                        ? "Static pods do not support ephemeral debug containers"
                        : "Debug container"
                  }
                  aria-label={debugContainer ? "Stop debug container" : "Debug container"}
                  className={cn(
                    "flex items-center gap-1.5 px-2.5 py-1 rounded-md border transition text-sm",
                    debugContainer
                      ? "border-emerald-500/50 bg-emerald-500/15 text-emerald-400"
                      : isStaticPod
                        ? "border-slate-800 text-slate-600 cursor-not-allowed"
                        : "border-accent/30 hover:border-accent/50 hover:bg-accent/15 text-accent",
                  )}
                >
                  <Bug className="w-3.5 h-3.5" />
                  <span className="hidden sm:inline">{stoppingDebug ? "Stopping…" : debugContainer ? "Stop Debug" : "Debug"}</span>
                  {debugContainer ? (
                    <span className="relative flex h-2 w-2">
                      <span className="absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75 animate-ping" />
                      <span className="relative inline-flex rounded-full h-2 w-2 bg-emerald-400" />
                    </span>
                  ) : (
                    <Kbd>B</Kbd>
                  )}
                </button>
                <button
                  onClick={() => setShowDeleteConfirm(true)}
                  disabled={isDeleting}
                  aria-label="Delete pod"
                  className="flex items-center gap-1.5 px-2.5 py-1 rounded-md border border-red-800/50 hover:border-red-600 hover:bg-red-500/15 transition text-sm text-red-400 disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  <Trash2 className="w-3.5 h-3.5" />
                  <span className="hidden sm:inline">Delete</span>
                  <Kbd>D</Kbd>
                </button>
                <button
                  onClick={() => setShowPortForward(!showPortForward)}
                  aria-label={showPortForward ? "Hide port forwarding" : "Show port forwarding"}
                  className={cn(
                    "flex items-center gap-1.5 px-2.5 py-1 rounded-md border transition text-sm relative",
                    activePortForwards > 0
                      ? "border-emerald-500/50 bg-emerald-500/15 text-emerald-400"
                      : showPortForward
                        ? "border-accent/50 bg-accent/15 text-accent"
                        : "border-slate-800 hover:border-accent/50 hover:bg-muted/30 text-slate-300",
                  )}
                >
                  <Plug className="w-3.5 h-3.5" />
                  <span className="hidden sm:inline">Ports</span>
                  {activePortForwards > 0 && (
                    <span className="relative flex h-2 w-2">
                      <span className="absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75 animate-ping" />
                      <span className="relative inline-flex rounded-full h-2 w-2 bg-emerald-400" />
                    </span>
                  )}
                </button>
              </div>
            )}
          </div>

          {/* Collapsible Pod Info */}
          <AnimatePresence>
            {showPodInfo && (
              <motion.div
                initial={{ height: 0, opacity: 0 }}
                animate={{ height: "auto", opacity: 1 }}
                exit={{ height: 0, opacity: 0 }}
                transition={{ duration: 0.15 }}
                className="overflow-hidden"
              >
                <div className="flex items-center gap-6 px-4 py-2 border-t border-slate-800/50 text-xs">
                  <div className="flex items-center gap-1.5">
                    <span className="text-slate-500">Namespace:</span>
                    <span className="text-slate-200 font-mono">{namespace}</span>
                  </div>
                  <div className="h-3 w-px bg-slate-800" />
                  <div className="flex items-center gap-1.5">
                    <span className="text-slate-500">Node:</span>
                    <span className="text-slate-200 font-mono">{node}</span>
                  </div>
                  <div className="h-3 w-px bg-slate-800" />
                  <div className="flex items-center gap-1.5">
                    <span className="text-slate-500">IP:</span>
                    <span className="text-slate-200 font-mono">{ip}</span>
                  </div>
                </div>
              </motion.div>
            )}
          </AnimatePresence>

          {/* Debug container banner */}
          {debugContainer && (
            <div className="flex items-center gap-2 px-4 py-2 border-t border-slate-800/50 bg-emerald-500/5">
              <Bug className="w-3.5 h-3.5 text-emerald-400 shrink-0" />
              <span className="text-xs text-emerald-400 font-medium">Debug:</span>
              <span className="text-xs text-slate-200">{debugContainer.name}</span>
              <span className="text-[10px] font-mono text-emerald-400/70 bg-emerald-500/10 px-1.5 py-0.5 rounded">
                {debugContainer.image}
              </span>
            </div>
          )}
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
                <span className="flex items-center gap-1.5">
                  {tab.label}
                  {tab.id === "shell" && debugContainer && (
                    <span className="relative flex h-2 w-2">
                      <span className="absolute inline-flex h-full w-full rounded-full bg-accent opacity-75 animate-ping" />
                      <span className="relative inline-flex rounded-full h-2 w-2 bg-accent" />
                    </span>
                  )}
                </span>
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

        {/* Contextual Toolbar — Logs tab */}
        {!isDeleted && activeTab === "logs" && (
          <div className="flex items-center gap-2 px-4 py-1.5 border-b border-slate-800/50 bg-surface/30">
            {/* Container selector */}
            {containers.length > 1 && (
              <select
                value={selectedContainer || ""}
                onChange={(e) => setSelectedContainer(e.target.value || undefined)}
                className="px-2 py-1 rounded-md border border-slate-800 bg-surface/60 text-xs text-slate-300 outline-none"
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
            <label className="flex items-center gap-1.5 px-2 py-1 rounded-md border border-slate-800 hover:border-accent/50 hover:bg-muted/30 transition text-xs cursor-pointer text-slate-300">
              <input
                type="checkbox"
                checked={showPrevious}
                onChange={(e) => setShowPrevious(e.target.checked)}
                className="w-3 h-3"
              />
              Previous
            </label>
            {/* Auto-scroll toggle */}
            <label className="flex items-center gap-1.5 px-2 py-1 rounded-md border border-slate-800 hover:border-accent/50 hover:bg-muted/30 transition text-xs cursor-pointer text-slate-300">
              <input
                type="checkbox"
                checked={autoScroll}
                onChange={(e) => setAutoScroll(e.target.checked)}
                className="w-3 h-3"
              />
              Auto-scroll
            </label>

            <div className="flex-1" />

            {/* Right side: Copy, Download, Search */}
            <button
              onClick={handleCopyLogs}
              aria-label="Copy logs to clipboard"
              className="flex items-center gap-1.5 px-2 py-1 rounded-md border border-slate-800 hover:border-accent/50 hover:bg-muted/30 transition text-xs text-slate-300"
            >
              <Copy className="w-3.5 h-3.5" />
              Copy
            </button>
            <button
              onClick={handleDownloadLogs}
              aria-label="Download logs as file"
              className="flex items-center gap-1.5 px-2 py-1 rounded-md border border-slate-800 hover:border-accent/50 hover:bg-muted/30 transition text-xs text-slate-300"
            >
              <Download className="w-3.5 h-3.5" />
              Download
            </button>
            <button
              onClick={() => {
                setLogSearchVisible(true);
                if (focusTimeoutRef.current) clearTimeout(focusTimeoutRef.current);
                focusTimeoutRef.current = setTimeout(() => logSearchInputRef.current?.focus(), 50);
              }}
              aria-label="Search logs"
              className={cn(
                "flex items-center gap-1.5 px-2 py-1 rounded-md border transition text-xs",
                logSearchVisible
                  ? "border-accent/50 bg-accent/15 text-accent"
                  : "border-slate-800 hover:border-accent/50 hover:bg-muted/30 text-slate-300",
              )}
            >
              <Search className="w-3.5 h-3.5" />
              <Kbd>⌘F</Kbd>
            </button>
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
                      debugContainer?.name ||
                      (selectedContainer ? selectedContainer.replace(/^init:/, "") : undefined)
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

        {/* Debug Container Modal */}
        <DebugContainerModal
          open={showDebugModal}
          onClose={() => setShowDebugModal(false)}
          onDebugReady={handleDebugReady}
          namespace={namespace}
          podName={podName}
          containers={containers.filter((c) => !c.startsWith("init:"))}
        />
      </div>

      {/* Port Forwarding Sidebar — always mounted, animated visibility */}
      {!isDeleted && activeTab !== "shell" && (
        <motion.div
          animate={{ width: showPortForward ? 320 : 0, opacity: showPortForward ? 1 : 0 }}
          initial={false}
          transition={{ duration: 0.2, ease: "easeInOut" }}
          className="overflow-hidden shrink-0"
        >
          <PortForwarding
            namespace={namespace}
            podName={podName}
            onActiveCountChange={setActivePortForwards}
          />
        </motion.div>
      )}
    </motion.div>
  );
}
