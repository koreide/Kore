import { useEffect, useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { ArrowLeft, Trash2, Eye, EyeOff } from "lucide-react";
import { deleteResource, describeResource } from "@/lib/api";
import { formatError } from "@/lib/errors";
import type { ResourceItem, ResourceKind } from "@/lib/types";
import { EventsTimeline } from "./events-timeline";
import { YamlEditor } from "./yaml-editor";
import { DescribeContent } from "./describe-content";
import { ConfirmDialog } from "./confirm-dialog";
import { useToast } from "./toast";
import { cn } from "@/lib/utils";

interface ResourceDetailsViewProps {
  resource: ResourceItem;
  kind: ResourceKind;
  onBack: () => void;
}

function Kbd({ children }: { children: React.ReactNode }) {
  return (
    <kbd className="px-1.5 py-0.5 bg-slate-800/80 rounded text-[10px] text-slate-400 font-mono border border-slate-700/50 ml-1.5">
      {children}
    </kbd>
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
      </div>
    </div>
  );
}

/** Decode base64 secret values in describe data */
function decodeSecretValues(
  data: Record<string, unknown>,
  revealedKeys: Set<string>,
): Record<string, unknown> {
  const result = { ...data };
  if (result.data && typeof result.data === "object") {
    const secretData = result.data as Record<string, string>;
    const decoded: Record<string, string> = {};
    for (const [key, value] of Object.entries(secretData)) {
      if (revealedKeys.has(key)) {
        try {
          decoded[key] = atob(value);
        } catch {
          decoded[key] = value;
        }
      } else {
        decoded[key] = "***";
      }
    }
    result.data = decoded;
  }
  return result;
}

export function ResourceDetailsView({ resource, kind, onBack }: ResourceDetailsViewProps) {
  const [activeTab, setActiveTab] = useState<"describe" | "yaml" | "events">("describe");
  const [describe, setDescribe] = useState<string>("");
  const [rawDescribe, setRawDescribe] = useState<Record<string, unknown> | null>(null);
  const [loading, setLoading] = useState(true);
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false);
  const [isDeleting, setIsDeleting] = useState(false);
  const [revealedSecrets, setRevealedSecrets] = useState<Set<string>>(new Set());
  const toast = useToast();

  const resourceName = resource.name || "";
  const namespace = resource.namespace || "default";

  useEffect(() => {
    if (activeTab === "describe") {
      setLoading(true);
      describeResource(kind, namespace, resourceName)
        .then((data) => {
          setRawDescribe(data);
          if (kind === "secrets") {
            const visible = decodeSecretValues(data, revealedSecrets);
            setDescribe(JSON.stringify(visible, null, 2));
          } else {
            setDescribe(JSON.stringify(data, null, 2));
          }
          setLoading(false);
        })
        .catch((err) => {
          setDescribe(`Error: ${err}`);
          setLoading(false);
        });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeTab, kind, namespace, resourceName]);

  // Update describe view when revealed secrets change
  useEffect(() => {
    if (kind === "secrets" && rawDescribe) {
      const visible = decodeSecretValues(rawDescribe, revealedSecrets);
      setDescribe(JSON.stringify(visible, null, 2));
    }
  }, [revealedSecrets, rawDescribe, kind]);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") onBack();
      if (e.key.toLowerCase() === "d" && !showDeleteConfirm) {
        const t = e.target as HTMLElement;
        if (t.tagName !== "INPUT" && t.tagName !== "TEXTAREA") {
          e.preventDefault();
          setShowDeleteConfirm(true);
        }
      }
      // Number key tab switching (1-3)
      const tabKeys = ["1", "2", "3"];
      const tabIds = ["describe", "yaml", "events"] as const;
      const idx = tabKeys.indexOf(e.key);
      if (idx >= 0 && idx < tabIds.length) {
        const t = e.target as HTMLElement;
        if (t.tagName !== "INPUT" && t.tagName !== "TEXTAREA") {
          e.preventDefault();
          setActiveTab(tabIds[idx]);
        }
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onBack, showDeleteConfirm]);

  const handleDelete = async () => {
    setIsDeleting(true);
    try {
      await deleteResource({ kind, namespace, name: resourceName });
      setShowDeleteConfirm(false);
      toast("Resource deleted", "success");
      onBack();
    } catch (err) {
      toast(formatError(err), "error");
    } finally {
      setIsDeleting(false);
    }
  };

  const toggleSecretKey = (key: string) => {
    setRevealedSecrets((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  const secretKeys =
    kind === "secrets" && rawDescribe?.data
      ? Object.keys(rawDescribe.data as Record<string, string>)
      : [];

  const tabs = [
    { id: "describe" as const, label: "Describe" },
    { id: "yaml" as const, label: "YAML" },
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
        <div className="flex items-center justify-between mb-3">
          <button
            onClick={onBack}
            className="flex items-center gap-2 px-3 py-1.5 rounded-md border border-slate-800 hover:border-accent/50 hover:bg-muted/30 transition text-sm text-slate-300"
          >
            <ArrowLeft className="w-4 h-4" />
            Back
            <Kbd>Esc</Kbd>
          </button>
          <button
            onClick={() => setShowDeleteConfirm(true)}
            disabled={isDeleting}
            className="flex items-center gap-2 px-3 py-1.5 rounded-md border border-red-800/50 hover:border-red-600 hover:bg-red-500/15 transition text-sm text-red-400 disabled:opacity-50"
          >
            <Trash2 className="w-4 h-4" />
            Delete
            <Kbd>D</Kbd>
          </button>
        </div>

        <div className="grid grid-cols-3 gap-4 text-sm">
          <div>
            <div className="text-slate-500 text-[10px] uppercase tracking-wider mb-1">Name</div>
            <div className="text-slate-100 font-mono text-xs">{resourceName}</div>
          </div>
          <div>
            <div className="text-slate-500 text-[10px] uppercase tracking-wider mb-1">
              Namespace
            </div>
            <div className="text-slate-100 font-mono text-xs">{namespace}</div>
          </div>
          <div>
            <div className="text-slate-500 text-[10px] uppercase tracking-wider mb-1">Kind</div>
            <div className="text-slate-100 font-mono text-xs capitalize">{kind}</div>
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
                layoutId="resource-detail-tab-indicator"
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
              className="h-full flex flex-col bg-surface/30 border border-slate-800 rounded-lg m-4 overflow-hidden"
            >
              {/* Secret key reveal toggles */}
              {kind === "secrets" && secretKeys.length > 0 && (
                <div className="px-4 pt-3 flex flex-wrap gap-2 border-b border-slate-800/50 pb-3 shrink-0">
                  <span className="text-[10px] uppercase text-slate-500 self-center mr-1">
                    Reveal:
                  </span>
                  {secretKeys.map((key) => (
                    <button
                      key={key}
                      onClick={() => toggleSecretKey(key)}
                      className={cn(
                        "flex items-center gap-1 px-2 py-1 rounded text-xs border transition",
                        revealedSecrets.has(key)
                          ? "border-accent/50 text-accent bg-accent/10"
                          : "border-slate-700 text-slate-400 hover:border-slate-600",
                      )}
                    >
                      {revealedSecrets.has(key) ? (
                        <EyeOff className="w-3 h-3" />
                      ) : (
                        <Eye className="w-3 h-3" />
                      )}
                      {key}
                    </button>
                  ))}
                </div>
              )}

              {loading ? (
                <DescribeSkeleton />
              ) : (
                <div className="flex-1 min-h-0">
                  <DescribeContent content={describe} />
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
              <YamlEditor kind={kind} namespace={namespace} name={resourceName} />
            </motion.div>
          ) : (
            <motion.div
              key="events"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              className="h-full overflow-auto"
            >
              <EventsTimeline kind={kind} namespace={namespace} name={resourceName} />
            </motion.div>
          )}
        </AnimatePresence>
      </div>

      <ConfirmDialog
        open={showDeleteConfirm}
        title={`Delete ${kind.slice(0, 1).toUpperCase() + kind.slice(1).replace(/s$/, "")}`}
        message={`Are you sure you want to delete "${resourceName}" in namespace "${namespace}"? This action cannot be undone.`}
        confirmText="Delete"
        cancelText="Cancel"
        onConfirm={handleDelete}
        onCancel={() => setShowDeleteConfirm(false)}
        variant="danger"
      />
    </motion.div>
  );
}
