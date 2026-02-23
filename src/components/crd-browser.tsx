import { useCallback, useEffect, useMemo, useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { Puzzle, ChevronRight, ChevronDown, Trash2, ArrowLeft, Search, Inbox } from "lucide-react";
import { listCrds, listCrdInstances, getCrdInstance, deleteCrdInstance } from "@/lib/api";
import type { CrdInfo } from "@/lib/api";
import { formatError } from "@/lib/errors";
import { cn } from "@/lib/utils";
import { useToast } from "@/components/toast";
import { ConfirmDialog } from "@/components/confirm-dialog";

interface CrdBrowserProps {
  namespace?: string;
  onBack?: () => void;
}

function formatRelativeAge(timestamp: string | undefined): string {
  if (!timestamp) return "-";
  try {
    const created = new Date(timestamp);
    const now = new Date();
    const diffMs = now.getTime() - created.getTime();
    const diffSeconds = Math.floor(diffMs / 1000);
    const diffMinutes = Math.floor(diffSeconds / 60);
    const diffHours = Math.floor(diffMinutes / 60);
    const diffDays = Math.floor(diffHours / 24);

    if (diffDays > 0) return `${diffDays}d`;
    if (diffHours > 0) return `${diffHours}h`;
    if (diffMinutes > 0) return `${diffMinutes}m`;
    return `${diffSeconds}s`;
  } catch {
    return "-";
  }
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

function Kbd({ children }: { children: React.ReactNode }) {
  return (
    <kbd className="px-1.5 py-0.5 bg-slate-800/80 rounded text-[10px] text-slate-400 font-mono border border-slate-700/50 ml-1.5">
      {children}
    </kbd>
  );
}

function CrdListSkeleton() {
  return (
    <div className="p-3 space-y-3">
      {[1, 2, 3].map((g) => (
        <div key={g} className="space-y-2">
          <div className="skeleton h-3 w-32" />
          <div className="space-y-1.5 ml-2">
            <div className="skeleton h-3 w-40" />
            <div className="skeleton h-3 w-36" />
            <div className="skeleton h-3 w-44" />
          </div>
        </div>
      ))}
    </div>
  );
}

function InstanceTableSkeleton() {
  return (
    <div className="p-4 space-y-3">
      <div className="skeleton h-4 w-48" />
      <div className="space-y-2">
        {[1, 2, 3, 4, 5].map((i) => (
          <div key={i} className="flex gap-4">
            <div className="skeleton h-3 w-40" />
            <div className="skeleton h-3 w-24" />
            <div className="skeleton h-3 w-16" />
          </div>
        ))}
      </div>
    </div>
  );
}

export function CrdBrowser({ namespace, onBack }: CrdBrowserProps) {
  // CRD list state
  const [crds, setCrds] = useState<CrdInfo[]>([]);
  const [crdsLoading, setCrdsLoading] = useState(true);
  const [crdsError, setCrdsError] = useState<string | null>(null);
  const [crdSearch, setCrdSearch] = useState("");
  const [collapsedGroups, setCollapsedGroups] = useState<Set<string>>(new Set());

  // Selected CRD
  const [selectedCrd, setSelectedCrd] = useState<CrdInfo | null>(null);

  // Instances state
  const [instances, setInstances] = useState<Record<string, unknown>[]>([]);
  const [instancesLoading, setInstancesLoading] = useState(false);
  const [instancesError, setInstancesError] = useState<string | null>(null);

  // Detail state
  const [selectedInstance, setSelectedInstance] = useState<Record<string, unknown> | null>(null);
  const [detailJson, setDetailJson] = useState<string>("");
  const [detailLoading, setDetailLoading] = useState(false);

  // Delete state
  const [deleteTarget, setDeleteTarget] = useState<{
    name: string;
    namespace: string;
  } | null>(null);
  const [isDeleting, setIsDeleting] = useState(false);

  const toast = useToast();

  // Fetch CRDs
  useEffect(() => {
    setCrdsLoading(true);
    setCrdsError(null);
    listCrds()
      .then((data) => {
        setCrds(data);
        setCrdsLoading(false);
      })
      .catch((err) => {
        setCrdsError(formatError(err));
        setCrdsLoading(false);
      });
  }, []);

  // Group CRDs by API group
  const groupedCrds = useMemo(() => {
    const filtered = crdSearch
      ? crds.filter(
          (c) =>
            c.kind.toLowerCase().includes(crdSearch.toLowerCase()) ||
            c.group.toLowerCase().includes(crdSearch.toLowerCase()) ||
            c.name.toLowerCase().includes(crdSearch.toLowerCase()),
        )
      : crds;

    const groups = new Map<string, CrdInfo[]>();
    for (const crd of filtered) {
      const existing = groups.get(crd.group) || [];
      existing.push(crd);
      groups.set(crd.group, existing);
    }

    // Sort groups alphabetically, sort CRDs within each group
    const sorted = Array.from(groups.entries()).sort(([a], [b]) => a.localeCompare(b));
    for (const [, items] of sorted) {
      items.sort((a, b) => a.kind.localeCompare(b.kind));
    }

    return sorted;
  }, [crds, crdSearch]);

  // Fetch instances when CRD is selected
  useEffect(() => {
    if (!selectedCrd) {
      setInstances([]);
      setSelectedInstance(null);
      setDetailJson("");
      return;
    }

    setInstancesLoading(true);
    setInstancesError(null);
    setSelectedInstance(null);
    setDetailJson("");

    const ns = selectedCrd.scope === "Namespaced" ? namespace : undefined;
    listCrdInstances(selectedCrd.group, selectedCrd.version, selectedCrd.plural, ns)
      .then((data) => {
        setInstances(data);
        setInstancesLoading(false);
      })
      .catch((err) => {
        setInstancesError(formatError(err));
        setInstancesLoading(false);
      });
  }, [selectedCrd, namespace]);

  // Fetch instance detail
  const handleSelectInstance = useCallback(
    async (instance: Record<string, unknown>) => {
      if (!selectedCrd) return;

      setSelectedInstance(instance);
      setDetailLoading(true);

      const metadata = instance.metadata as Record<string, unknown> | undefined;
      const name = (metadata?.name as string) || "";
      const ns = (metadata?.namespace as string) || "default";

      try {
        const detail = await getCrdInstance(
          selectedCrd.group,
          selectedCrd.version,
          selectedCrd.plural,
          ns,
          name,
        );
        setDetailJson(JSON.stringify(detail, null, 2));
      } catch (err) {
        setDetailJson(`Error fetching instance: ${formatError(err)}`);
      } finally {
        setDetailLoading(false);
      }
    },
    [selectedCrd],
  );

  // Delete instance
  const handleDelete = useCallback(async () => {
    if (!selectedCrd || !deleteTarget) return;

    setIsDeleting(true);
    try {
      await deleteCrdInstance(
        selectedCrd.group,
        selectedCrd.version,
        selectedCrd.plural,
        deleteTarget.namespace,
        deleteTarget.name,
      );
      toast("Resource deleted", "success");
      setDeleteTarget(null);

      // Remove from instances list
      setInstances((prev) =>
        prev.filter((inst) => {
          const meta = inst.metadata as Record<string, unknown> | undefined;
          return (
            (meta?.name as string) !== deleteTarget.name ||
            (meta?.namespace as string) !== deleteTarget.namespace
          );
        }),
      );

      // Clear detail if the deleted instance was selected
      if (selectedInstance) {
        const meta = selectedInstance.metadata as Record<string, unknown> | undefined;
        if (
          (meta?.name as string) === deleteTarget.name &&
          (meta?.namespace as string) === deleteTarget.namespace
        ) {
          setSelectedInstance(null);
          setDetailJson("");
        }
      }
    } catch (err) {
      toast(formatError(err), "error");
    } finally {
      setIsDeleting(false);
    }
  }, [selectedCrd, deleteTarget, selectedInstance, toast]);

  // Toggle group collapse
  const toggleGroup = useCallback((group: string) => {
    setCollapsedGroups((prev) => {
      const next = new Set(prev);
      if (next.has(group)) {
        next.delete(group);
      } else {
        next.add(group);
      }
      return next;
    });
  }, []);

  // Keyboard shortcuts
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        if (selectedInstance) {
          setSelectedInstance(null);
          setDetailJson("");
        } else if (selectedCrd) {
          setSelectedCrd(null);
        } else {
          onBack?.();
        }
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onBack, selectedCrd, selectedInstance]);

  return (
    <motion.div
      initial={{ opacity: 0, x: 20 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: 20 }}
      className="h-full w-full flex flex-col bg-background"
    >
      {/* Header */}
      <div className="border-b border-slate-800 p-4 bg-surface/50">
        <div className="flex items-center justify-between">
          {onBack && (
            <button
              onClick={onBack}
              className="flex items-center gap-2 px-3 py-1.5 rounded-md border border-slate-800 hover:border-accent/50 hover:bg-muted/30 transition text-sm text-slate-300"
            >
              <ArrowLeft className="w-4 h-4" />
              Back
              <Kbd>Esc</Kbd>
            </button>
          )}
          <div className="flex items-center gap-2.5">
            <div className="w-7 h-7 rounded-lg bg-accent/15 flex items-center justify-center">
              <Puzzle className="w-4 h-4 text-accent" />
            </div>
            <div>
              <h2 className="text-sm font-semibold text-slate-100">Custom Resources</h2>
              <p className="text-[10px] text-slate-500">
                {crds.length} CRD{crds.length !== 1 ? "s" : ""} discovered
              </p>
            </div>
          </div>
          <div className="w-20" />
        </div>
      </div>

      {/* Two-panel layout */}
      <div className="flex-1 flex min-h-0">
        {/* Left panel: CRD list */}
        <div className="w-72 border-r border-slate-800 flex flex-col bg-surface/30">
          {/* Search */}
          <div className="p-3 border-b border-slate-800/50">
            <div className="flex items-center gap-2 px-2.5 py-1.5 rounded-md border border-slate-800 bg-background/50 focus-within:border-accent/50 transition">
              <Search className="w-3.5 h-3.5 text-slate-500 shrink-0" />
              <input
                value={crdSearch}
                onChange={(e) => setCrdSearch(e.target.value)}
                placeholder="Search CRDs..."
                className="bg-transparent text-xs text-slate-200 outline-none w-full placeholder:text-slate-600"
              />
            </div>
          </div>

          {/* CRD groups */}
          <div className="flex-1 overflow-auto">
            {crdsLoading ? (
              <CrdListSkeleton />
            ) : crdsError ? (
              <div className="p-4 text-sm text-red-400">{crdsError}</div>
            ) : groupedCrds.length === 0 ? (
              <div className="flex flex-col items-center justify-center h-full text-slate-400 gap-2 p-4">
                <Inbox className="w-10 h-10 text-slate-600" />
                <p className="text-xs">
                  {crdSearch ? "No CRDs match your search" : "No CRDs found"}
                </p>
              </div>
            ) : (
              <div className="py-1">
                {groupedCrds.map(([group, items]) => {
                  const isCollapsed = collapsedGroups.has(group);
                  return (
                    <div key={group}>
                      {/* Group header */}
                      <button
                        onClick={() => toggleGroup(group)}
                        className="w-full flex items-center gap-1.5 px-3 py-2 text-left hover:bg-muted/20 transition"
                      >
                        {isCollapsed ? (
                          <ChevronRight className="w-3 h-3 text-slate-500 shrink-0" />
                        ) : (
                          <ChevronDown className="w-3 h-3 text-slate-500 shrink-0" />
                        )}
                        <span className="text-[10px] uppercase tracking-wider text-slate-500 font-medium truncate">
                          {group}
                        </span>
                        <span className="text-[10px] text-slate-600 ml-auto shrink-0">
                          {items.length}
                        </span>
                      </button>

                      {/* CRD items */}
                      <AnimatePresence>
                        {!isCollapsed && (
                          <motion.div
                            initial={{ height: 0, opacity: 0 }}
                            animate={{ height: "auto", opacity: 1 }}
                            exit={{ height: 0, opacity: 0 }}
                            transition={{ duration: 0.15, ease: "easeOut" }}
                            className="overflow-hidden"
                          >
                            {items.map((crd) => {
                              const isSelected =
                                selectedCrd?.name === crd.name && selectedCrd?.group === crd.group;
                              return (
                                <button
                                  key={`${crd.group}/${crd.name}`}
                                  onClick={() => setSelectedCrd(crd)}
                                  className={cn(
                                    "w-full flex items-center gap-2 px-3 py-1.5 pl-7 text-left transition text-xs",
                                    isSelected
                                      ? "bg-accent/10 text-accent border-l-2 border-l-accent"
                                      : "text-slate-300 hover:bg-muted/20 hover:text-slate-100",
                                  )}
                                >
                                  <Puzzle className="w-3 h-3 shrink-0" />
                                  <span className="font-mono truncate">{crd.kind}</span>
                                  {crd.scope === "Cluster" && (
                                    <span className="text-[9px] bg-slate-700/50 text-slate-400 px-1 py-0.5 rounded shrink-0">
                                      cluster
                                    </span>
                                  )}
                                </button>
                              );
                            })}
                          </motion.div>
                        )}
                      </AnimatePresence>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        </div>

        {/* Right panel: Instances / Detail */}
        <div className="flex-1 flex flex-col min-w-0">
          {!selectedCrd ? (
            <div className="flex-1 flex flex-col items-center justify-center text-slate-400 gap-3">
              <Puzzle className="w-12 h-12 text-slate-600" />
              <p className="text-sm font-medium">Select a Custom Resource</p>
              <p className="text-xs text-slate-500">
                Choose a CRD from the left panel to view its instances
              </p>
            </div>
          ) : selectedInstance ? (
            /* Instance detail view */
            <motion.div
              key="detail"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              className="flex-1 flex flex-col min-h-0"
            >
              {/* Detail header */}
              <div className="border-b border-slate-800 p-3 bg-surface/30 flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <button
                    onClick={() => {
                      setSelectedInstance(null);
                      setDetailJson("");
                    }}
                    className="flex items-center gap-1.5 px-2.5 py-1 rounded-md border border-slate-800 hover:border-accent/50 hover:bg-muted/30 transition text-xs text-slate-300"
                  >
                    <ArrowLeft className="w-3.5 h-3.5" />
                    Back to list
                  </button>
                  <span className="text-xs text-slate-500">|</span>
                  <span className="text-xs font-mono text-accent">
                    {((selectedInstance.metadata as Record<string, unknown> | undefined)
                      ?.name as string) || "unknown"}
                  </span>
                </div>
              </div>

              {/* Detail content */}
              <div className="flex-1 overflow-auto bg-surface/20 border border-slate-800 rounded-lg m-4">
                {detailLoading ? (
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
                ) : (
                  <div className="p-4 font-mono text-xs leading-relaxed">
                    <pre
                      className="whitespace-pre-wrap"
                      dangerouslySetInnerHTML={{
                        __html: highlightJson(detailJson || "No details available"),
                      }}
                    />
                  </div>
                )}
              </div>
            </motion.div>
          ) : (
            /* Instance table view */
            <motion.div
              key="table"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              className="flex-1 flex flex-col min-h-0"
            >
              {/* Table header */}
              <div className="border-b border-slate-800 p-3 bg-surface/30 flex items-center gap-3">
                <Puzzle className="w-4 h-4 text-accent shrink-0" />
                <div className="min-w-0">
                  <h3 className="text-sm font-semibold text-slate-100 truncate">
                    {selectedCrd.kind}
                  </h3>
                  <p className="text-[10px] text-slate-500 truncate">
                    {selectedCrd.group}/{selectedCrd.version} - {selectedCrd.scope}
                  </p>
                </div>
                <span className="ml-auto text-[10px] bg-accent/15 text-accent px-1.5 py-0.5 rounded-full font-medium shrink-0">
                  {instances.length}
                </span>
              </div>

              {/* Table content */}
              <div className="flex-1 overflow-auto">
                {instancesLoading ? (
                  <InstanceTableSkeleton />
                ) : instancesError ? (
                  <div className="p-4 text-sm text-red-400">{instancesError}</div>
                ) : instances.length === 0 ? (
                  <div className="flex flex-col items-center justify-center h-full text-slate-400 gap-3">
                    <Inbox className="w-10 h-10 text-slate-600" />
                    <p className="text-xs">No instances found</p>
                    <p className="text-[10px] text-slate-500">
                      {namespace
                        ? `No ${selectedCrd.kind} resources in this namespace`
                        : `No ${selectedCrd.kind} resources found`}
                    </p>
                  </div>
                ) : (
                  <div className="border border-slate-800 rounded-lg m-4 glass overflow-hidden">
                    <table className="min-w-full text-sm">
                      <thead className="bg-surface/80 sticky top-0 z-10">
                        <tr className="border-b border-slate-800">
                          <th className="px-3 py-2 text-left text-xs font-medium uppercase tracking-wider text-slate-400">
                            Name
                          </th>
                          <th className="px-3 py-2 text-left text-xs font-medium uppercase tracking-wider text-slate-400">
                            Namespace
                          </th>
                          <th className="px-3 py-2 text-left text-xs font-medium uppercase tracking-wider text-slate-400">
                            Age
                          </th>
                          <th className="px-3 py-2 text-right text-xs font-medium uppercase tracking-wider text-slate-400 w-16">
                            {/* Actions */}
                          </th>
                        </tr>
                      </thead>
                      <tbody className="divide-y divide-slate-800/50">
                        {instances.map((instance, idx) => {
                          const metadata = instance.metadata as Record<string, unknown> | undefined;
                          const name = (metadata?.name as string) || "-";
                          const ns = (metadata?.namespace as string) || "-";
                          const creationTimestamp = metadata?.creationTimestamp as
                            | string
                            | undefined;

                          return (
                            <tr
                              key={`${ns}/${name}-${idx}`}
                              onClick={() => handleSelectInstance(instance)}
                              className="transition cursor-pointer hover:bg-muted/30"
                            >
                              <td className="px-3 py-2 text-slate-100 font-mono text-xs">{name}</td>
                              <td className="px-3 py-2 text-slate-100 font-mono text-xs">{ns}</td>
                              <td className="px-3 py-2 text-slate-100 font-mono text-xs">
                                {formatRelativeAge(creationTimestamp)}
                              </td>
                              <td className="px-3 py-2 text-right">
                                <button
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    setDeleteTarget({ name, namespace: ns });
                                  }}
                                  disabled={isDeleting}
                                  className="p-1 rounded hover:bg-red-500/10 transition text-slate-500 hover:text-red-400 disabled:opacity-50"
                                  title="Delete"
                                >
                                  <Trash2 className="w-3.5 h-3.5" />
                                </button>
                              </td>
                            </tr>
                          );
                        })}
                      </tbody>
                    </table>
                  </div>
                )}
              </div>
            </motion.div>
          )}
        </div>
      </div>

      {/* Delete Confirmation */}
      <ConfirmDialog
        open={deleteTarget !== null}
        title={`Delete ${selectedCrd?.kind || "Resource"}`}
        message={`Are you sure you want to delete "${deleteTarget?.name}" in namespace "${deleteTarget?.namespace}"? This action cannot be undone.`}
        confirmText={isDeleting ? "Deleting..." : "Delete"}
        cancelText="Cancel"
        onConfirm={handleDelete}
        onCancel={() => setDeleteTarget(null)}
        variant="danger"
      />
    </motion.div>
  );
}
