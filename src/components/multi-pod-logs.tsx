import { useEffect, useRef, useState, useMemo, useCallback } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { Search, Copy, X, Filter, ChevronUp, ChevronDown } from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { streamMultiPodLogs } from "@/lib/api";
import { cn } from "@/lib/utils";
import { useToast } from "@/components/toast";

interface MultiPodLogsProps {
  namespace: string;
  labelSelector: string;
  onStop?: () => void;
}

interface LogEntry {
  pod: string;
  color: string;
  line: string;
}

/** Detect log level and return color class */
function getLogLevelColor(line: string): string | null {
  const upper = line.toUpperCase();
  if (/\bERROR\b|\bFATAL\b|\bPANIC\b/.test(upper)) return "text-red-400";
  if (/\bWARN(ING)?\b/.test(upper)) return "text-amber-400";
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

export function MultiPodLogs({ namespace, labelSelector, onStop }: MultiPodLogsProps) {
  const [logEntries, setLogEntries] = useState<LogEntry[]>([]);
  const [autoScroll, setAutoScroll] = useState(true);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Per-pod filter
  const [hiddenPods, setHiddenPods] = useState<Set<string>>(new Set());
  const [showFilterPanel, setShowFilterPanel] = useState(false);

  // Log search
  const [logSearch, setLogSearch] = useState("");
  const [logSearchVisible, setLogSearchVisible] = useState(false);
  const [logSearchIndex, setLogSearchIndex] = useState(0);
  const logSearchInputRef = useRef<HTMLInputElement>(null);

  const logsEndRef = useRef<HTMLDivElement>(null);
  const logsContainerRef = useRef<HTMLDivElement>(null);
  const toast = useToast();

  // Collect unique pod names from entries
  const podNames = useMemo(() => {
    const seen = new Set<string>();
    for (const entry of logEntries) {
      seen.add(entry.pod);
    }
    return Array.from(seen).sort();
  }, [logEntries]);

  // Pod color map for consistent coloring
  const podColorMap = useMemo(() => {
    const map = new Map<string, string>();
    for (const entry of logEntries) {
      if (!map.has(entry.pod)) {
        map.set(entry.pod, entry.color);
      }
    }
    return map;
  }, [logEntries]);

  // Visible log entries after filtering
  const visibleEntries = useMemo(() => {
    return logEntries.filter((entry) => !hiddenPods.has(entry.pod));
  }, [logEntries, hiddenPods]);

  // Search matches (indices into visibleEntries)
  const logSearchMatches = useMemo(() => {
    if (!logSearch) return [];
    const matches: number[] = [];
    const query = logSearch.toLowerCase();
    visibleEntries.forEach((entry, i) => {
      if (entry.line.toLowerCase().includes(query) || entry.pod.toLowerCase().includes(query)) {
        matches.push(i);
      }
    });
    return matches;
  }, [visibleEntries, logSearch]);

  // Clean the label selector for event channel name (replace non-alphanumeric with underscores)
  const cleanedSelector = useMemo(
    () => labelSelector.replace(/[^a-zA-Z0-9_.-]/g, "_"),
    [labelSelector],
  );

  // Start streaming and listen for events
  useEffect(() => {
    let isMounted = true;
    let unlistenFn: (() => void) | null = null;

    const eventName = `multi-pod-logs://${namespace}/${cleanedSelector}`;

    const setup = async () => {
      try {
        const unlisten = await listen<LogEntry>(eventName, (event) => {
          if (!isMounted) return;
          setLoading(false);
          setLogEntries((prev) => [...prev, event.payload]);
        });

        if (!isMounted) {
          unlisten();
          return;
        }
        unlistenFn = unlisten;

        await streamMultiPodLogs(namespace, labelSelector);
      } catch (err) {
        if (!isMounted) return;
        const msg = typeof err === "string" ? err : String(err);
        setError(msg);
        setLoading(false);
      }
    };

    setup();

    return () => {
      isMounted = false;
      if (unlistenFn) {
        unlistenFn();
      }
    };
  }, [namespace, labelSelector, cleanedSelector]);

  // Auto-scroll
  useEffect(() => {
    if (autoScroll && logsEndRef.current) {
      logsEndRef.current.scrollIntoView({ behavior: "smooth" });
    }
  }, [visibleEntries, autoScroll]);

  // Keyboard shortcuts
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        if (logSearchVisible) {
          setLogSearchVisible(false);
          setLogSearch("");
          return;
        }
        onStop?.();
      }
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "f") {
        e.preventDefault();
        setLogSearchVisible(true);
        setTimeout(() => logSearchInputRef.current?.focus(), 50);
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onStop, logSearchVisible]);

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
      const lineIndex = logSearchMatches[idx];
      const el = logsContainerRef.current?.querySelector(`[data-line="${lineIndex}"]`);
      el?.scrollIntoView({ behavior: "smooth", block: "center" });
    },
    [logSearchMatches, logSearchIndex],
  );

  const handleCopyLogs = async () => {
    try {
      const text = visibleEntries
        .map((entry, i) => `${i + 1} [${entry.pod}] ${entry.line}`)
        .join("\n");
      await navigator.clipboard.writeText(text);
      toast("Copied to clipboard", "success");
    } catch (err) {
      console.error("Failed to copy logs", err);
      toast("Failed to copy logs", "error");
    }
  };

  const togglePod = (pod: string) => {
    setHiddenPods((prev) => {
      const next = new Set(prev);
      if (next.has(pod)) {
        next.delete(pod);
      } else {
        next.add(pod);
      }
      return next;
    });
  };

  const showAllPods = () => setHiddenPods(new Set());
  const hideAllPods = () => setHiddenPods(new Set(podNames));

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      className="h-full flex flex-col"
    >
      {/* Toolbar */}
      <div className="flex items-center justify-between gap-2 px-4 py-2 border-b border-slate-800 bg-surface/50">
        <div className="flex items-center gap-2">
          <span className="text-xs text-slate-400 font-mono">
            {namespace}/{labelSelector}
          </span>
          <span className="text-[10px] text-slate-600">
            {podNames.length} pod{podNames.length !== 1 ? "s" : ""}
            {hiddenPods.size > 0 && ` (${hiddenPods.size} hidden)`}
          </span>
        </div>
        <div className="flex items-center gap-2">
          {/* Filter button */}
          <button
            onClick={() => setShowFilterPanel((v) => !v)}
            className={cn(
              "flex items-center gap-1.5 px-3 py-1.5 rounded-md border transition text-sm",
              showFilterPanel
                ? "border-accent/50 bg-accent/10 text-accent"
                : "border-slate-800 hover:border-accent/50 hover:bg-muted/30 text-slate-300",
            )}
          >
            <Filter className="w-3.5 h-3.5" />
            Filter
          </button>
          {/* Copy button */}
          <button
            onClick={handleCopyLogs}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-md border border-slate-800 hover:border-accent/50 hover:bg-muted/30 transition text-sm text-slate-300"
          >
            <Copy className="w-3.5 h-3.5" />
            Copy
          </button>
          {/* Auto-scroll toggle */}
          <label className="flex items-center gap-2 px-3 py-1.5 rounded-md border border-slate-800 hover:border-accent/50 hover:bg-muted/30 transition text-sm cursor-pointer text-slate-300">
            <input
              type="checkbox"
              checked={autoScroll}
              onChange={(e) => setAutoScroll(e.target.checked)}
              className="w-3.5 h-3.5"
            />
            Auto-scroll
          </label>
          {/* Stop button */}
          {onStop && (
            <button
              onClick={onStop}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-md border border-red-800/50 hover:border-red-600 hover:bg-red-500/15 transition text-sm text-red-400"
            >
              <X className="w-3.5 h-3.5" />
              Stop
            </button>
          )}
        </div>
      </div>

      {/* Filter panel */}
      <AnimatePresence>
        {showFilterPanel && podNames.length > 0 && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ duration: 0.15 }}
            className="overflow-hidden border-b border-slate-800 bg-surface/30"
          >
            <div className="px-4 py-2 flex flex-wrap items-center gap-2">
              <span className="text-[10px] text-slate-500 uppercase tracking-wider mr-1">Pods</span>
              <button
                onClick={showAllPods}
                className="text-[10px] text-accent hover:text-accent/80 transition mr-1"
              >
                All
              </button>
              <button
                onClick={hideAllPods}
                className="text-[10px] text-accent hover:text-accent/80 transition mr-2"
              >
                None
              </button>
              {podNames.map((pod) => {
                const color = podColorMap.get(pod) || "#94a3b8";
                const isVisible = !hiddenPods.has(pod);
                return (
                  <label
                    key={pod}
                    className={cn(
                      "flex items-center gap-1.5 px-2 py-1 rounded border text-xs font-mono cursor-pointer transition",
                      isVisible
                        ? "border-slate-700 bg-slate-800/50 text-slate-200"
                        : "border-slate-800 bg-transparent text-slate-600",
                    )}
                  >
                    <input
                      type="checkbox"
                      checked={isVisible}
                      onChange={() => togglePod(pod)}
                      className="w-3 h-3"
                    />
                    <span
                      className="w-2 h-2 rounded-full shrink-0"
                      style={{ backgroundColor: color }}
                    />
                    <span className="truncate max-w-[160px]">{pod}</span>
                  </label>
                );
              })}
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      {/* Log content */}
      <div className="flex-1 overflow-hidden relative">
        {/* Search overlay */}
        {logSearchVisible && (
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

        <div className="h-full overflow-auto bg-black/40 border border-slate-800 rounded-lg m-4">
          {loading && logEntries.length === 0 ? (
            <LogSkeleton />
          ) : error ? (
            <div className="p-4 text-red-400 text-xs font-mono">{error}</div>
          ) : (
            <div ref={logsContainerRef} className="font-mono text-xs leading-relaxed">
              {visibleEntries.length > 0 ? (
                visibleEntries.map((entry, i) => {
                  const levelColor = getLogLevelColor(entry.line);
                  const isSearchMatch =
                    logSearch &&
                    (entry.line.toLowerCase().includes(logSearch.toLowerCase()) ||
                      entry.pod.toLowerCase().includes(logSearch.toLowerCase()));
                  const isCurrentMatch =
                    logSearchMatches.length > 0 && logSearchMatches[logSearchIndex] === i;

                  return (
                    <div
                      key={i}
                      data-line={i}
                      className={cn(
                        "flex hover:bg-white/[0.02] group",
                        isCurrentMatch && "bg-amber-400/10",
                        isSearchMatch && !isCurrentMatch && "bg-amber-400/5",
                      )}
                    >
                      {/* Line number */}
                      <span className="select-none text-right text-slate-600 w-12 shrink-0 pr-3 py-px border-r border-slate-800/50 group-hover:text-slate-500">
                        {i + 1}
                      </span>
                      {/* Pod name */}
                      <span
                        className="select-none shrink-0 px-2 py-px font-semibold truncate max-w-[180px]"
                        style={{ color: entry.color }}
                        title={entry.pod}
                      >
                        {logSearch ? highlightMatches(entry.pod, logSearch) : entry.pod}
                      </span>
                      {/* Separator */}
                      <span className="text-slate-700 py-px select-none">|</span>
                      {/* Log line */}
                      <span
                        className={cn(
                          "pl-2 py-px whitespace-pre-wrap break-all flex-1",
                          levelColor || "text-green-400",
                        )}
                      >
                        {logSearch ? highlightMatches(entry.line, logSearch) : entry.line}
                      </span>
                    </div>
                  );
                })
              ) : (
                <div className="p-4 text-slate-500">
                  {logEntries.length > 0
                    ? "All pods are hidden. Adjust filters to see logs."
                    : "No logs available"}
                </div>
              )}
              <div ref={logsEndRef} />
            </div>
          )}
        </div>
      </div>
    </motion.div>
  );
}
