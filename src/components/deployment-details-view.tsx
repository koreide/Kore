import { useEffect, useState, useMemo } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { ArrowLeft, Minus, Plus, RefreshCw, Trash2 } from "lucide-react";
import { deleteResource, describeResource, scaleDeployment, restartDeployment } from "@/lib/api";
import { formatError } from "@/lib/errors";
import type { ResourceItem } from "@/lib/types";
import { EventsTimeline } from "./events-timeline";
import { ConfirmDialog } from "./confirm-dialog";
import { YamlEditor } from "./yaml-editor";
import { DeploymentRollback } from "./deployment-rollback";
import { MultiPodLogs } from "./multi-pod-logs";
import { useToast } from "./toast";
import { cn } from "@/lib/utils";

interface DeploymentDetailsViewProps {
  deployment: ResourceItem;
  onBack: () => void;
}

function Kbd({ children }: { children: React.ReactNode }) {
  return (
    <kbd className="px-1.5 py-0.5 bg-slate-800/80 rounded text-[10px] text-slate-400 font-mono border border-slate-700/50 ml-1.5">
      {children}
    </kbd>
  );
}

function highlightJson(json: string): string {
  return json
    .replace(/("(?:[^"\\]|\\.)*")\s*:/g, '<span class="text-accent">$1</span>:')
    .replace(
      /:\s*("(?:[^"\\]|\\.)*")/g,
      (_match, value) => `: <span class="text-emerald-400">${value}</span>`,
    )
    .replace(/:\s*(\d+(?:\.\d+)?)\b/g, ': <span class="text-amber-400">$1</span>')
    .replace(/:\s*(true|false)\b/g, ': <span class="text-indigo-400">$1</span>')
    .replace(/:\s*(null)\b/g, ': <span class="text-slate-500">$1</span>');
}

export function DeploymentDetailsView({ deployment, onBack }: DeploymentDetailsViewProps) {
  const [activeTab, setActiveTab] = useState<"describe" | "yaml" | "rollback" | "logs" | "events">(
    "describe",
  );
  const [describe, setDescribe] = useState<string>("");
  const [loading, setLoading] = useState(true);
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false);
  const [showRestartConfirm, setShowRestartConfirm] = useState(false);
  const [isDeleting, setIsDeleting] = useState(false);
  const [replicaInput, setReplicaInput] = useState<number>(0);
  const [isScaling, setIsScaling] = useState(false);
  const [isRestarting, setIsRestarting] = useState(false);
  const toast = useToast();

  const name = deployment.name || "";
  const namespace = deployment.namespace || "default";

  // Parse current replicas from the ready field (e.g. "3/3")
  useEffect(() => {
    const match = deployment.ready?.match(/(\d+)\/(\d+)/);
    if (match) {
      setReplicaInput(parseInt(match[2], 10));
    }
  }, [deployment.ready]);

  useEffect(() => {
    if (activeTab === "describe") {
      setLoading(true);
      describeResource("deployments", namespace, name)
        .then((data) => {
          setDescribe(JSON.stringify(data, null, 2));
          // Update replica count from fresh data
          const specReplicas = (data?.spec as Record<string, unknown>)?.replicas;
          if (typeof specReplicas === "number") {
            setReplicaInput(specReplicas);
          }
          setLoading(false);
        })
        .catch((err) => {
          setDescribe(`Error: ${err}`);
          setLoading(false);
        });
    }
  }, [activeTab, namespace, name]);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") onBack();
      // Number key tab switching (1-5)
      const tabKeys = ["1", "2", "3", "4", "5"];
      const tabIds = ["describe", "yaml", "rollback", "logs", "events"] as const;
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
  }, [onBack]);

  const handleDelete = async () => {
    setIsDeleting(true);
    try {
      await deleteResource({ kind: "deployments", namespace, name });
      setShowDeleteConfirm(false);
      toast("Deployment deleted", "success");
      onBack();
    } catch (err) {
      toast(formatError(err), "error");
    } finally {
      setIsDeleting(false);
    }
  };

  const handleScale = async () => {
    setIsScaling(true);
    try {
      await scaleDeployment(namespace, name, replicaInput);
      toast(`Scaled to ${replicaInput} replicas`, "success");
    } catch (err) {
      toast(formatError(err), "error");
    } finally {
      setIsScaling(false);
    }
  };

  const handleRestart = async () => {
    setIsRestarting(true);
    try {
      await restartDeployment(namespace, name);
      setShowRestartConfirm(false);
      toast("Rolling restart initiated", "success");
    } catch (err) {
      toast(formatError(err), "error");
    } finally {
      setIsRestarting(false);
    }
  };

  // Get labels from deployment for multi-pod log streaming
  const deploymentLabels = useMemo(() => {
    // Try to extract matchLabels from deployment ready field or name
    return `app=${name}`;
  }, [name]);

  const tabs = [
    { id: "describe" as const, label: "Describe" },
    { id: "yaml" as const, label: "YAML" },
    { id: "rollback" as const, label: "Rollback" },
    { id: "logs" as const, label: "Logs" },
    { id: "events" as const, label: "Events" },
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
            className="flex items-center gap-2 px-3 py-1.5 rounded-md border border-slate-800 hover:border-accent/50 hover:bg-muted/30 transition text-sm text-slate-300"
          >
            <ArrowLeft className="w-4 h-4" />
            Back
            <Kbd>Esc</Kbd>
          </button>

          <div className="flex items-center gap-2">
            {/* Scale control */}
            <div className="flex items-center gap-1 border border-slate-800 rounded-md overflow-hidden">
              <button
                onClick={() => setReplicaInput((v) => Math.max(0, v - 1))}
                className="px-2 py-1.5 hover:bg-muted/30 transition text-slate-300"
                disabled={isScaling}
              >
                <Minus className="w-3.5 h-3.5" />
              </button>
              <input
                type="number"
                min={0}
                value={replicaInput}
                onChange={(e) => setReplicaInput(Math.max(0, parseInt(e.target.value) || 0))}
                className="w-12 bg-transparent text-center text-sm text-slate-100 outline-none border-x border-slate-800 py-1.5 font-mono"
              />
              <button
                onClick={() => setReplicaInput((v) => v + 1)}
                className="px-2 py-1.5 hover:bg-muted/30 transition text-slate-300"
                disabled={isScaling}
              >
                <Plus className="w-3.5 h-3.5" />
              </button>
            </div>
            <button
              onClick={handleScale}
              disabled={isScaling}
              className="px-3 py-1.5 rounded-md border border-accent/50 bg-accent/10 text-accent text-sm hover:bg-accent/20 transition disabled:opacity-50"
            >
              {isScaling ? "Scaling..." : "Scale"}
            </button>

            {/* Restart */}
            <button
              onClick={() => setShowRestartConfirm(true)}
              disabled={isRestarting}
              className="flex items-center gap-2 px-3 py-1.5 rounded-md border border-slate-800 hover:border-amber-500/50 hover:bg-amber-500/10 transition text-sm text-slate-300 disabled:opacity-50"
            >
              <RefreshCw className={cn("w-4 h-4", isRestarting && "animate-spin")} />
              Restart
            </button>

            {/* Delete */}
            <button
              onClick={() => setShowDeleteConfirm(true)}
              disabled={isDeleting}
              className="flex items-center gap-2 px-3 py-1.5 rounded-md border border-red-800/50 hover:border-red-600 hover:bg-red-500/15 transition text-sm text-red-400 disabled:opacity-50"
            >
              <Trash2 className="w-4 h-4" />
              Delete
            </button>
          </div>
        </div>

        <div className="grid grid-cols-2 md:grid-cols-5 gap-4 text-sm">
          <div>
            <div className="text-slate-500 text-[10px] uppercase tracking-wider mb-1">Name</div>
            <div className="text-slate-100 font-mono text-xs">{name}</div>
          </div>
          <div>
            <div className="text-slate-500 text-[10px] uppercase tracking-wider mb-1">
              Namespace
            </div>
            <div className="text-slate-100 font-mono text-xs">{namespace}</div>
          </div>
          <div>
            <div className="text-slate-500 text-[10px] uppercase tracking-wider mb-1">Ready</div>
            <div className="text-slate-100 font-mono text-xs">{deployment.ready || "-"}</div>
          </div>
          <div>
            <div className="text-slate-500 text-[10px] uppercase tracking-wider mb-1">
              Up-to-date
            </div>
            <div className="text-slate-100 font-mono text-xs">{deployment.upToDate ?? "-"}</div>
          </div>
          <div>
            <div className="text-slate-500 text-[10px] uppercase tracking-wider mb-1">
              Available
            </div>
            <div className="text-slate-100 font-mono text-xs">{deployment.available ?? "-"}</div>
          </div>
        </div>
      </div>

      {/* Tabs */}
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
                layoutId="deploy-detail-tab-indicator"
                className="absolute bottom-0 left-0 right-0 h-0.5 bg-accent rounded-full"
                transition={{ type: "spring", stiffness: 500, damping: 35 }}
              />
            )}
          </button>
        ))}
      </div>

      {/* Content */}
      <div className="flex-1 overflow-hidden">
        <AnimatePresence mode="wait">
          {activeTab === "describe" ? (
            <motion.div
              key="describe"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              className="h-full overflow-auto bg-surface/30 border border-slate-800 rounded-lg m-4"
            >
              {loading ? (
                <div className="p-4 space-y-3">
                  <div className="skeleton h-4 w-32" />
                  <div className="space-y-2 ml-4">
                    <div className="skeleton h-3 w-48" />
                    <div className="skeleton h-3 w-56" />
                  </div>
                </div>
              ) : (
                <div className="p-4 font-mono text-xs leading-relaxed">
                  <pre
                    className="whitespace-pre-wrap"
                    dangerouslySetInnerHTML={{
                      __html: highlightJson(describe || "No details available"),
                    }}
                  />
                </div>
              )}
            </motion.div>
          ) : activeTab === "yaml" ? (
            <motion.div
              key="yaml"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              className="h-full overflow-auto"
            >
              <YamlEditor kind="deployments" namespace={namespace} name={name} />
            </motion.div>
          ) : activeTab === "rollback" ? (
            <motion.div
              key="rollback"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              className="h-full overflow-auto"
            >
              <DeploymentRollback namespace={namespace} deploymentName={name} />
            </motion.div>
          ) : activeTab === "logs" ? (
            <motion.div
              key="logs"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              className="h-full overflow-hidden"
            >
              <MultiPodLogs namespace={namespace} labelSelector={deploymentLabels} />
            </motion.div>
          ) : (
            <motion.div
              key="events"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              className="h-full overflow-auto"
            >
              <EventsTimeline kind="deployments" namespace={namespace} name={name} />
            </motion.div>
          )}
        </AnimatePresence>
      </div>

      <ConfirmDialog
        open={showDeleteConfirm}
        title="Delete Deployment"
        message={`Are you sure you want to delete deployment "${name}" in namespace "${namespace}"? This action cannot be undone.`}
        confirmText="Delete"
        cancelText="Cancel"
        onConfirm={handleDelete}
        onCancel={() => setShowDeleteConfirm(false)}
        variant="danger"
      />

      <ConfirmDialog
        open={showRestartConfirm}
        title="Restart Deployment"
        message={`This will initiate a rolling restart of deployment "${name}". Pods will be recreated.`}
        confirmText="Restart"
        cancelText="Cancel"
        onConfirm={handleRestart}
        onCancel={() => setShowRestartConfirm(false)}
        variant="warning"
      />
    </motion.div>
  );
}
