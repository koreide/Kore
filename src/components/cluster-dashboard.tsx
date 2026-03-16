import { useEffect, useState, useCallback, useRef } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { PieChart, Pie, Cell, ResponsiveContainer, Tooltip } from "recharts";
import {
  Activity,
  AlertTriangle,
  Clock,
  Heart,
  Info,
  RefreshCw,
  Server,
  Skull,
  Loader2,
} from "lucide-react";
import { getClusterHealth, getClusterHealthMultiCluster } from "@/lib/api";
import type { ClusterHealth, ClusterHealthEntry } from "@/lib/api";
import { cn } from "@/lib/utils";
import {
  parseUsagePercent,
  formatMemory,
  scoreColor,
  scoreBorderColor,
  scoreGlowColor,
  scoreLabel,
  scoreBgColor,
} from "@/lib/k8s-utils";

interface ClusterDashboardProps {
  onNavigateToResource?: (kind: string, name: string, namespace: string) => void;
  multiCluster?: boolean;
}

// ── Colour constants ──────────────────────────────────────────────────

const POD_COLORS: Record<string, string> = {
  Running: "#10b981", // emerald-500
  Pending: "#f59e0b", // amber-500
  Failed: "#ef4444", // red-500
  CrashLooping: "#a855f7", // purple-500
  Succeeded: "#64748b", // slate-500
};

const NODE_STATUS_COLORS: Record<string, string> = {
  Ready: "#10b981",
  NotReady: "#ef4444",
  Unknown: "#f59e0b",
};

// ── Card wrapper ──────────────────────────────────────────────────────

function DashboardCard({
  title,
  icon: Icon,
  children,
  className,
  tooltip,
}: {
  title: string;
  icon: React.ElementType;
  children: React.ReactNode;
  className?: string;
  tooltip?: string;
}) {
  const [showTooltip, setShowTooltip] = useState(false);

  return (
    <motion.div
      initial={{ opacity: 0, y: 12 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.3 }}
      className={cn("glass rounded-lg border border-slate-800 p-4 flex flex-col", className)}
    >
      <div className="flex items-center gap-2 mb-3">
        <Icon className="w-4 h-4 text-accent" />
        <h3 className="text-sm font-semibold text-slate-200 tracking-wide uppercase">{title}</h3>
        {tooltip && (
          <div
            className="relative"
            onMouseEnter={() => setShowTooltip(true)}
            onMouseLeave={() => setShowTooltip(false)}
          >
            <Info className="w-3.5 h-3.5 text-slate-500 hover:text-slate-300 transition-colors cursor-help" />
            {showTooltip && (
              <div className="absolute left-1/2 -translate-x-1/2 top-full mt-1.5 z-50 w-56 px-3 py-2 text-[11px] leading-relaxed text-slate-300 bg-surface border border-slate-700 rounded-md shadow-lg pointer-events-none">
                {tooltip}
              </div>
            )}
          </div>
        )}
      </div>
      <div className="flex-1 min-h-0">{children}</div>
    </motion.div>
  );
}

// ── Custom Pie tooltip ────────────────────────────────────────────────

function PieTooltipContent({
  active,
  payload,
}: {
  active?: boolean;
  payload?: Array<{ name: string; value: number; payload: { fill: string } }>;
}) {
  if (!active || !payload?.length) return null;
  const entry = payload[0];
  return (
    <div className="bg-surface border border-slate-700 rounded px-3 py-2 text-xs shadow-lg">
      <span
        className="inline-block w-2 h-2 rounded-full mr-2"
        style={{ background: entry.payload.fill }}
      />
      <span className="text-slate-300">{entry.name}: </span>
      <span className="text-slate-100 font-semibold">{entry.value}</span>
    </div>
  );
}

// ── Kind mapping ────────────────────────────────────────────────────

const KIND_TO_RESOURCE: Record<string, string> = {
  Pod: "pods",
  Deployment: "deployments",
  Service: "services",
  Node: "nodes",
  Ingress: "ingresses",
  Job: "jobs",
  CronJob: "cronjobs",
  ConfigMap: "configmaps",
  Secret: "secrets",
  ReplicaSet: "deployments",
};

// ── Health cards (shared between single and multi-cluster) ───────────

function HealthCards({
  health,
  onNavigateToResource,
}: {
  health: ClusterHealth;
  onNavigateToResource?: (kind: string, name: string, namespace: string) => void;
}) {
  const podData = [
    { name: "Running", value: health.pods.running, fill: POD_COLORS.Running },
    { name: "Pending", value: health.pods.pending, fill: POD_COLORS.Pending },
    { name: "Failed", value: health.pods.failed, fill: POD_COLORS.Failed },
    { name: "CrashLooping", value: health.pods.crash_looping, fill: POD_COLORS.CrashLooping },
    { name: "Succeeded", value: health.pods.succeeded, fill: POD_COLORS.Succeeded },
  ].filter((d) => d.value > 0);

  const hasNoPods = podData.length === 0;

  return (
    <>
      {/* ── Health Score ─────────────────────────────────────────── */}
      <DashboardCard title="Cluster Health" icon={Heart} tooltip="Score based on node readiness, pod health, and recent warning events.">
        <div className="flex flex-col items-center justify-center py-4">
          <div
            className={cn(
              "w-28 h-28 rounded-full border-4 flex items-center justify-center shadow-lg",
              scoreBorderColor(health.score),
              scoreGlowColor(health.score),
            )}
          >
            <span className={cn("text-4xl font-bold font-mono", scoreColor(health.score))}>
              {health.score}
            </span>
          </div>
          <span className={cn("mt-3 text-sm font-medium", scoreColor(health.score))}>
            {scoreLabel(health.score)}
          </span>
          <span className="text-xs text-slate-500 mt-1">
            {health.pods.total} pods across {health.nodes.length} nodes
          </span>
        </div>
      </DashboardCard>

      {/* ── Pod Health Pie ───────────────────────────────────────── */}
      <DashboardCard title="Pod Health" icon={Activity}>
        {hasNoPods ? (
          <div className="flex items-center justify-center h-full text-slate-500 text-sm">
            No pods found
          </div>
        ) : (
          <div className="flex items-center gap-4">
            <div className="w-40 h-40 flex-shrink-0">
              <ResponsiveContainer width="100%" height="100%">
                <PieChart>
                  <Pie
                    data={podData}
                    cx="50%"
                    cy="50%"
                    innerRadius={35}
                    outerRadius={60}
                    paddingAngle={2}
                    dataKey="value"
                    strokeWidth={0}
                  >
                    {podData.map((entry, idx) => (
                      <Cell key={idx} fill={entry.fill} />
                    ))}
                  </Pie>
                  <Tooltip content={<PieTooltipContent />} />
                </PieChart>
              </ResponsiveContainer>
            </div>
            <div className="flex flex-col gap-1.5 text-xs">
              {podData.map((entry) => (
                <div key={entry.name} className="flex items-center gap-2">
                  <span
                    className="w-2.5 h-2.5 rounded-full flex-shrink-0"
                    style={{ background: entry.fill }}
                  />
                  <span className="text-slate-400">{entry.name}</span>
                  <span className="text-slate-200 font-mono ml-auto">{entry.value}</span>
                </div>
              ))}
              <div className="border-t border-slate-800 pt-1 mt-1 flex items-center gap-2">
                <span className="text-slate-500">Total</span>
                <span className="text-slate-300 font-mono ml-auto">{health.pods.total}</span>
              </div>
            </div>
          </div>
        )}
      </DashboardCard>

      {/* ── Node Capacity ────────────────────────────────────────── */}
      <DashboardCard title="Node Status" icon={Server}>
        {health.nodes.length === 0 ? (
          <div className="flex items-center justify-center h-full text-slate-500 text-sm">
            No node data
          </div>
        ) : (
          <div className="space-y-3 overflow-auto max-h-48">
            {health.nodes.map((node) => {
              const statusColor = NODE_STATUS_COLORS[node.status] || NODE_STATUS_COLORS.Unknown;
              return (
                <div key={node.name} className="space-y-1">
                  <div className="flex items-center justify-between text-xs">
                    <span className="text-slate-300 font-mono truncate max-w-[60%]">
                      {node.name}
                    </span>
                    <span className="font-medium" style={{ color: statusColor }}>
                      {node.status}
                    </span>
                  </div>
                  {/* CPU bar */}
                  <div className="flex items-center gap-2 text-[10px] text-slate-500">
                    <span className="w-8">CPU</span>
                    <div className="flex-1 h-1.5 bg-slate-800 rounded-full overflow-hidden">
                      <div
                        className="h-full rounded-full transition-all duration-500"
                        style={{
                          width: `${parseUsagePercent(node.cpu_usage, node.cpu_capacity)}%`,
                          backgroundColor: statusColor,
                        }}
                      />
                    </div>
                    <span className="w-20 text-right tabular-nums text-slate-400">
                      {node.cpu_usage || "?"} / {node.cpu_capacity || "?"}
                    </span>
                  </div>
                  {/* Memory bar */}
                  <div className="flex items-center gap-2 text-[10px] text-slate-500">
                    <span className="w-8">Mem</span>
                    <div className="flex-1 h-1.5 bg-slate-800 rounded-full overflow-hidden">
                      <div
                        className="h-full rounded-full transition-all duration-500"
                        style={{
                          width: `${parseUsagePercent(node.memory_usage, node.memory_capacity)}%`,
                          backgroundColor: statusColor,
                        }}
                      />
                    </div>
                    <span className="w-20 text-right tabular-nums text-slate-400">
                      {formatMemory(node.memory_usage)} / {formatMemory(node.memory_capacity)}
                    </span>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </DashboardCard>

      {/* ── Restart Hotlist ──────────────────────────────────────── */}
      <DashboardCard title="Restart Hotlist" icon={RefreshCw}>
        {health.restart_hotlist.length === 0 ? (
          <div className="flex items-center justify-center h-full text-slate-500 text-sm">
            No restarts detected
          </div>
        ) : (
          <div className="space-y-1 overflow-auto max-h-48">
            {health.restart_hotlist.map((pod, idx) => (
              <button
                key={`${pod.namespace}/${pod.name}`}
                onClick={() => onNavigateToResource?.("pods", pod.name, pod.namespace)}
                className={cn(
                  "w-full flex items-center gap-2 px-2 py-1.5 rounded text-xs text-left",
                  "hover:bg-slate-800/60 transition-colors group",
                )}
              >
                <span className="text-slate-500 font-mono w-4 text-right">{idx + 1}</span>
                <span className="flex-1 truncate font-mono text-slate-300 group-hover:text-accent transition-colors">
                  {pod.name}
                </span>
                <span className="text-slate-500 text-[10px] truncate max-w-[80px]">
                  {pod.namespace}
                </span>
                <span
                  className={cn(
                    "font-mono font-semibold tabular-nums",
                    pod.restarts >= 10
                      ? "text-red-400"
                      : pod.restarts >= 5
                        ? "text-amber-400"
                        : "text-slate-400",
                  )}
                >
                  {pod.restarts}
                </span>
              </button>
            ))}
          </div>
        )}
      </DashboardCard>

      {/* ── Pending Pods ─────────────────────────────────────────── */}
      <DashboardCard title="Pending Pods" icon={Clock}>
        {health.pending_pods.length === 0 ? (
          <div className="flex items-center justify-center h-full text-slate-500 text-sm">
            No pending pods
          </div>
        ) : (
          <div className="space-y-2 overflow-auto max-h-48">
            {health.pending_pods.map((pod) => (
              <button
                key={`${pod.namespace}/${pod.name}`}
                onClick={() => onNavigateToResource?.("pods", pod.name, pod.namespace)}
                className={cn(
                  "w-full text-left px-2 py-1.5 rounded text-xs",
                  "hover:bg-slate-800/60 transition-colors group",
                )}
              >
                <div className="flex items-center justify-between">
                  <span className="font-mono text-slate-300 truncate group-hover:text-accent transition-colors">
                    {pod.name}
                  </span>
                  <span className="text-slate-600 text-[10px] ml-2 flex-shrink-0">{pod.age}</span>
                </div>
                <div className="flex items-center gap-1 mt-0.5">
                  <span className="text-amber-400/70 text-[10px] truncate">
                    {pod.reason || "Unknown"}
                  </span>
                  <span className="text-slate-600 text-[10px] ml-auto truncate max-w-[80px]">
                    {pod.namespace}
                  </span>
                </div>
              </button>
            ))}
          </div>
        )}
      </DashboardCard>

      {/* ── Recent Warnings ──────────────────────────────────────── */}
      <DashboardCard title="Recent Warnings" icon={Skull} className="md:col-span-2 xl:col-span-1">
        {health.recent_warnings.length === 0 ? (
          <div className="flex items-center justify-center h-full text-slate-500 text-sm">
            No recent warnings
          </div>
        ) : (
          <div className="space-y-2 overflow-auto max-h-48">
            {health.recent_warnings.map((evt, idx) => {
              const resourceKind = KIND_TO_RESOURCE[evt.object_kind];
              const canNavigate = !!(resourceKind && evt.object_name && onNavigateToResource);
              return (
                <div
                  key={`${evt.involved_object}-${evt.reason}-${idx}`}
                  className="px-2 py-1.5 rounded border border-amber-500/10 bg-amber-500/5 text-xs"
                >
                  <div className="flex items-center gap-1.5">
                    <AlertTriangle className="w-3 h-3 text-amber-400 flex-shrink-0" />
                    <span className="text-amber-300 font-medium truncate">{evt.reason}</span>
                    {evt.count > 1 && (
                      <span className="text-slate-500 font-mono text-[10px] ml-auto flex-shrink-0">
                        x{evt.count}
                      </span>
                    )}
                  </div>
                  <p className="text-slate-400 mt-0.5 line-clamp-2 leading-relaxed">{evt.message}</p>
                  <div className="flex items-center justify-between mt-1 text-[10px] text-slate-600">
                    {canNavigate ? (
                      <button
                        onClick={() => onNavigateToResource!(resourceKind, evt.object_name, evt.namespace)}
                        className="truncate max-w-[60%] text-accent hover:text-accent/80 transition-colors"
                      >
                        {evt.involved_object}
                      </button>
                    ) : (
                      <span className="truncate max-w-[60%]">{evt.involved_object}</span>
                    )}
                    <span>{evt.last_seen}</span>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </DashboardCard>
    </>
  );
}

// ── Main component ────────────────────────────────────────────────────

export function ClusterDashboard({ onNavigateToResource, multiCluster }: ClusterDashboardProps) {
  const [health, setHealth] = useState<ClusterHealth | null>(null);
  const [multiHealth, setMultiHealth] = useState<ClusterHealthEntry[] | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const fetchHealth = useCallback(async () => {
    try {
      if (multiCluster) {
        const data = await getClusterHealthMultiCluster();
        setMultiHealth(data.clusters);
        setHealth(null);
      } else {
        const data = await getClusterHealth();
        setHealth(data);
        setMultiHealth(null);
      }
      setError(null);
    } catch (err: unknown) {
      setError(err?.toString() || "Failed to fetch cluster health");
    } finally {
      setLoading(false);
    }
  }, [multiCluster]);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);

    const doFetch = async () => {
      await fetchHealth();
    };

    doFetch();

    intervalRef.current = setInterval(() => {
      if (!cancelled) fetchHealth();
    }, 30_000);

    return () => {
      cancelled = true;
      if (intervalRef.current) clearInterval(intervalRef.current);
    };
  }, [fetchHealth]);

  // ── Loading state ───────────────────────────────────────────────────

  if (loading && !health && !multiHealth) {
    return (
      <div className="h-full flex items-center justify-center text-slate-400">
        <div className="flex items-center gap-2">
          <Loader2 className="w-5 h-5 animate-spin" />
          Loading cluster health...
        </div>
      </div>
    );
  }

  // ── Error state ─────────────────────────────────────────────────────

  if (error && !health && !multiHealth) {
    return (
      <div className="h-full flex items-center justify-center">
        <div className="text-center text-slate-400">
          <AlertTriangle className="w-12 h-12 mx-auto mb-4 opacity-50 text-amber-400" />
          <p className="text-lg">{error}</p>
          <button
            onClick={fetchHealth}
            className="mt-4 text-sm text-accent hover:text-accent/80 transition-colors inline-flex items-center gap-1"
          >
            <RefreshCw className="w-3.5 h-3.5" /> Retry
          </button>
        </div>
      </div>
    );
  }

  // ── Multi-cluster view ──────────────────────────────────────────────

  if (multiCluster && multiHealth) {
    return (
      <div className="h-full overflow-auto p-4">
        <AnimatePresence mode="wait">
          <motion.div
            key="multi-dashboard"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            className="space-y-6"
          >
            {multiHealth.map((entry) => (
              <div key={entry.context}>
                <div className="flex items-center gap-3 mb-3">
                  <h2 className="text-base font-semibold text-slate-200 font-mono">
                    {entry.context}
                  </h2>
                  <span
                    className={cn(
                      "text-xs font-mono font-semibold px-2 py-0.5 rounded-full border",
                      scoreBgColor(entry.health.score),
                    )}
                  >
                    {entry.health.score} - {scoreLabel(entry.health.score)}
                  </span>
                </div>
                <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4 auto-rows-min">
                  <HealthCards health={entry.health} onNavigateToResource={onNavigateToResource} />
                </div>
              </div>
            ))}
          </motion.div>
        </AnimatePresence>
      </div>
    );
  }

  // ── Single-cluster view ─────────────────────────────────────────────

  if (!health) return null;

  return (
    <div className="h-full overflow-auto p-4">
      <AnimatePresence mode="wait">
        <motion.div
          key="dashboard"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4 auto-rows-min"
        >
          <HealthCards health={health} onNavigateToResource={onNavigateToResource} />
        </motion.div>
      </AnimatePresence>
    </div>
  );
}
