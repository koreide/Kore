import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Sidebar } from "./components/sidebar";
import { ResourceTable } from "./components/resource-table";
import { CommandPalette } from "./components/command-palette";
import { PodDetailsView } from "./components/pod-details-view";
import { ResourceDetailsView } from "./components/resource-details-view";
import { DeploymentDetailsView } from "./components/deployment-details-view";
import { LabelFilterBar } from "./components/label-filter-bar";
import { ClusterDashboard } from "./components/cluster-dashboard";
import { ResourceGraphView } from "./components/resource-graph";
import { CrdBrowser } from "./components/crd-browser";
import { HelmReleases } from "./components/helm-releases";
import { HelmDetailView } from "./components/helm-detail-view";
import { Settings as SettingsPage } from "./components/settings";
import { ShortcutOverlay } from "./components/shortcut-overlay";
import { AIPanel } from "./components/ai-panel";
import { useK8sContext } from "./hooks/use-k8s-context";
import { useResourceWatch } from "./hooks/use-resource-watch";
import { useKeyboardShortcuts } from "./hooks/use-keyboard-shortcuts";
import { usePinnedResources } from "./hooks/use-pinned-resources";
import { useRestartHistory } from "./hooks/use-restart-history";
import { deleteResource } from "./lib/api";
import { formatError } from "./lib/errors";
import type { ResourceItem, ResourceKind, AppView } from "./lib/types";
import type { SortingState } from "@tanstack/react-table";
import { Search, ChevronRight, Filter, Sparkles } from "lucide-react";
import { AnimatePresence } from "framer-motion";
import { ConfirmDialog } from "./components/confirm-dialog";
import { useToast } from "./components/toast";

function SkeletonTable() {
  const widths = [
    ["40%", "20%", "15%", "12%", "10%"],
    ["55%", "25%", "18%", "10%", "8%"],
    ["35%", "30%", "12%", "15%", "10%"],
    ["60%", "15%", "20%", "8%", "12%"],
    ["45%", "22%", "16%", "14%", "9%"],
    ["50%", "18%", "14%", "11%", "10%"],
    ["38%", "28%", "17%", "13%", "8%"],
    ["52%", "20%", "15%", "10%", "11%"],
  ];

  return (
    <div className="w-full h-full rounded-lg border border-slate-800 glass overflow-hidden">
      <div className="flex gap-4 px-3 py-3 border-b border-slate-800 bg-surface/80">
        {["16%", "14%", "10%", "10%", "8%"].map((w, i) => (
          <div key={i} className="skeleton h-3" style={{ width: w }} />
        ))}
      </div>
      {widths.map((row, rowIdx) => (
        <div
          key={rowIdx}
          className="flex gap-4 px-3 py-3 border-b border-slate-800/50"
          style={{ opacity: 1 - rowIdx * 0.08 }}
        >
          {row.map((w, colIdx) => (
            <div key={colIdx} className="skeleton h-3" style={{ width: w }} />
          ))}
        </div>
      ))}
    </div>
  );
}

function Kbd({ children }: { children: React.ReactNode }) {
  return (
    <kbd className="px-1.5 py-0.5 bg-slate-800/80 rounded text-[10px] text-slate-400 font-mono border border-slate-700/50">
      {children}
    </kbd>
  );
}

export default function App() {
  const { contexts, currentContext, namespaces, namespace, setNamespace, handleContextChange, refreshNamespaces } =
    useK8sContext();
  const [kind, setKind] = useState<ResourceKind>("pods");
  const [selected, setSelected] = useState<ResourceItem | null>(null);
  const [sorting, setSorting] = useState<SortingState>([]);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [search, setSearch] = useState("");
  const [selectedRowIndex, setSelectedRowIndex] = useState(-1);
  const [viewMode, setViewMode] = useState<AppView>("table");
  const [labelFilters, setLabelFilters] = useState<string[]>([]);
  const [showLabelFilter, setShowLabelFilter] = useState(false);
  const [shortcutsOpen, setShortcutsOpen] = useState(false);
  const [selectedHelmRelease, setSelectedHelmRelease] = useState<{ name: string; namespace: string } | null>(null);
  const [aiPanelOpen, setAiPanelOpen] = useState(false);
  const [multiCluster, setMultiCluster] = useState(false);
  const searchRef = useRef<HTMLInputElement>(null);
  const toast = useToast();
  const { pinned, togglePin, isPinned, removePin } = usePinnedResources();
  const { getHistory: getRestartHistory } = useRestartHistory();

  const [deleteConfirm, setDeleteConfirm] = useState<{
    open: boolean;
    kind: ResourceKind;
    name: string;
    namespace: string;
  }>({ open: false, kind: "pods", name: "", namespace: "" });

  const labelSelector = labelFilters.length > 0 ? labelFilters.join(",") : undefined;

  const { loading, refresh, filtered } = useResourceWatch(
    currentContext,
    kind,
    namespace,
    labelSelector,
    multiCluster,
  );

  const filteredResources = useMemo(() => filtered(search), [filtered, search]);

  useKeyboardShortcuts({
    onTogglePalette: useCallback(() => setPaletteOpen((v) => !v), []),
    onRefresh: refresh,
    searchRef,
  });

  const handleKindChange = useCallback((newKind: ResourceKind) => {
    setKind(newKind);
    setViewMode("table");
    setSelected(null);
    setLabelFilters([]);
    setShowLabelFilter(false);
  }, []);

  const handleViewChange = useCallback((view: AppView) => {
    setViewMode(view);
    setSelected(null);
    if (view === "helm") {
      setSelectedHelmRelease(null);
    }
  }, []);

  const handleContextSwitch = useCallback(
    async (newContext: string) => {
      setSelected(null);
      setSearch("");
      setViewMode("table");
      setLabelFilters([]);
      await handleContextChange(newContext);
    },
    [handleContextChange],
  );

  const handleRowSelect = useCallback(
    (row: ResourceItem) => {
      setSelected(row);
      if (row.name && (row.namespace || kind === "nodes" || kind === "events")) {
        setViewMode("details");
      }
    },
    [kind],
  );

  const handleBack = useCallback(() => {
    if (viewMode === "helm-detail") {
      setViewMode("helm");
      setSelectedHelmRelease(null);
      return;
    }
    setViewMode("table");
    setSelected(null);
  }, [viewMode]);

  // ? shortcut for shortcut overlay, j/k/l/h vim navigation
  const handleGlobalKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      if (e.key === "?" && !paletteOpen) {
        e.preventDefault();
        setShortcutsOpen((v) => !v);
      }
      if (viewMode === "table" && !paletteOpen && !shortcutsOpen) {
        if (e.key === "j") {
          e.preventDefault();
          setSelectedRowIndex((i) => Math.min(i + 1, filteredResources.length - 1));
        } else if (e.key === "k") {
          e.preventDefault();
          setSelectedRowIndex((i) => Math.max(i - 1, 0));
        } else if (e.key === "l" && selectedRowIndex >= 0 && selectedRowIndex < filteredResources.length) {
          e.preventDefault();
          handleRowSelect(filteredResources[selectedRowIndex]);
        }
      }
      if (e.key === "h" && viewMode === "details") {
        e.preventDefault();
        handleBack();
      }
    },
    [viewMode, paletteOpen, shortcutsOpen, filteredResources, selectedRowIndex, handleRowSelect, handleBack],
  );

  useEffect(() => {
    window.addEventListener("keydown", handleGlobalKeyDown);
    return () => window.removeEventListener("keydown", handleGlobalKeyDown);
  }, [handleGlobalKeyDown]);

  const handleNavigateToKind = useCallback((newKind: ResourceKind) => {
    setKind(newKind);
    setViewMode("table");
    setSelected(null);
  }, []);

  const handleNavigateToResource = useCallback(
    (resourceKind: ResourceKind, resource: ResourceItem) => {
      setKind(resourceKind);
      setSelected(resource);
      setViewMode("details");
    },
    [],
  );

  const handlePinSelect = useCallback(
    (pinKind: string, name: string, ns: string) => {
      setKind(pinKind as ResourceKind);
      setSelected({ name, namespace: ns });
      setViewMode("details");
    },
    [],
  );

  const handleRowAction = useCallback(
    (action: string, row: ResourceItem) => {
      if (action === "delete") {
        setDeleteConfirm({
          open: true,
          kind,
          name: row.name,
          namespace: row.namespace || "default",
        });
      } else if (action === "logs" || action === "exec" || action === "port-forward" || action === "describe") {
        setSelected(row);
        setViewMode("details");
      } else if (action === "scale" || action === "restart") {
        setSelected(row);
        setViewMode("details");
      }
    },
    [kind],
  );

  const handleConfirmDelete = useCallback(async () => {
    try {
      await deleteResource({
        kind: deleteConfirm.kind,
        namespace: deleteConfirm.namespace,
        name: deleteConfirm.name,
      });
      toast(`${deleteConfirm.name} deleted`, "success");
    } catch (err) {
      toast(formatError(err), "error");
    }
    setDeleteConfirm((prev) => ({ ...prev, open: false }));
  }, [deleteConfirm, toast]);

  const handleDashboardNavigate = useCallback(
    (navKind: string, name: string, ns: string) => {
      setKind(navKind as ResourceKind);
      setSelected({ name, namespace: ns });
      setViewMode("details");
    },
    [],
  );

  const handleGraphSelect = useCallback(
    (graphKind: string, name: string, ns: string) => {
      const kindMap: Record<string, ResourceKind> = {
        Pod: "pods",
        Deployment: "deployments",
        Service: "services",
        Ingress: "ingresses",
        Job: "jobs",
        CronJob: "cronjobs",
        ReplicaSet: "deployments",
      };
      const k = kindMap[graphKind] || "pods";
      setKind(k);
      setSelected({ name, namespace: ns });
      setViewMode("details");
    },
    [],
  );

  const handleHelmSelect = useCallback((release: { name: string; namespace: string }) => {
    setSelectedHelmRelease(release);
    setViewMode("helm-detail");
  }, []);

  const kindLabel = kind.charAt(0).toUpperCase() + kind.slice(1);
  const isTableView = viewMode === "table";
  const isDetailView = viewMode === "details";
  const showHeader = isTableView || isDetailView;

  return (
    <div className="h-screen w-screen bg-background text-slate-100 flex overflow-hidden">
      <Sidebar
        contexts={contexts}
        currentContext={currentContext}
        namespaces={namespaces}
        currentNamespace={namespace}
        currentResource={kind}
        currentView={viewMode}
        onContextChange={handleContextSwitch}
        onNamespaceChange={setNamespace}
        onResourceChange={handleKindChange}
        onViewChange={handleViewChange}
        resourceCount={filteredResources.length}
        onNamespaceDropdownOpen={refreshNamespaces}
        pinned={pinned}
        onPinSelect={handlePinSelect}
        onPinRemove={(k, n, ns) => removePin(k, n, ns)}
        multiCluster={multiCluster}
        onMultiClusterToggle={setMultiCluster}
      />
      <main className="flex-1 p-4 grid grid-rows-[auto,1fr] gap-3">
        {showHeader && (
          <header className="flex flex-col gap-2">
            <div className="flex items-center gap-3">
              <div className="flex items-center gap-2 text-sm min-w-0 shrink-0">
                <span className="text-slate-400 font-medium">{kindLabel}</span>
                {isDetailView && selected ? (
                  <>
                    <ChevronRight className="w-3.5 h-3.5 text-slate-600" />
                    <span className="text-slate-100 font-medium truncate max-w-[200px]">
                      {selected.name}
                    </span>
                  </>
                ) : (
                  <span className="text-[10px] bg-slate-800/80 text-slate-400 px-1.5 py-0.5 rounded-full font-medium">
                    {filteredResources.length}
                  </span>
                )}
              </div>

              <div className="relative flex-1 max-w-md mx-auto">
                <Search className="w-4 h-4 text-slate-500 absolute left-3 top-1/2 -translate-y-1/2" />
                <input
                  ref={searchRef}
                  placeholder="Search..."
                  value={search}
                  onChange={(e) => setSearch(e.target.value)}
                  className="w-full bg-surface/60 border border-slate-800 rounded-lg px-10 py-1.5 text-sm outline-none focus:border-accent/50 transition placeholder:text-slate-600"
                />
                <div className="absolute right-3 top-1/2 -translate-y-1/2">
                  <Kbd>/</Kbd>
                </div>
              </div>

              <div className="flex items-center gap-2 shrink-0">
                <button
                  onClick={() => setShowLabelFilter(!showLabelFilter)}
                  className={`px-2 py-1.5 rounded-lg border transition text-xs ${
                    showLabelFilter || labelFilters.length > 0
                      ? "border-accent/50 text-accent bg-accent/10"
                      : "border-slate-800 text-slate-400 hover:text-slate-200"
                  }`}
                >
                  <Filter className="w-3.5 h-3.5" />
                </button>
                <div className="flex items-center gap-2 px-3 py-1.5 rounded-lg border border-slate-800 bg-surface/60 text-xs text-slate-300">
                  <span className="w-1.5 h-1.5 rounded-full bg-emerald-400" />
                  <span className="font-mono truncate max-w-[140px]">
                    {currentContext ?? "No context"}
                  </span>
                </div>
                <button
                  onClick={() => setAiPanelOpen(true)}
                  className="flex items-center gap-1.5 px-2 py-1.5 rounded-lg border border-slate-800 text-slate-400 hover:text-accent hover:border-accent/50 hover:bg-accent/10 transition text-xs"
                  title="Ask AI"
                >
                  <Sparkles className="w-3.5 h-3.5" />
                  <span className="hidden sm:inline">AI</span>
                </button>
                <Kbd>⌘K</Kbd>
              </div>
            </div>

            {(showLabelFilter || labelFilters.length > 0) && isTableView && (
              <LabelFilterBar labels={labelFilters} onLabelsChange={setLabelFilters} />
            )}
          </header>
        )}

        {!showHeader && <div />}

        <section className="overflow-hidden relative">
          <AnimatePresence mode="wait">
            {viewMode === "dashboard" ? (
              <ClusterDashboard
                key="dashboard"
                onNavigateToResource={handleDashboardNavigate}
              />
            ) : viewMode === "graph" ? (
              <ResourceGraphView
                key="graph"
                namespace={namespace === "*" ? undefined : namespace}
                onSelectResource={handleGraphSelect}
              />
            ) : viewMode === "crds" ? (
              <CrdBrowser
                key="crds"
                namespace={namespace === "*" ? undefined : namespace}
                onBack={() => setViewMode("table")}
              />
            ) : viewMode === "helm" ? (
              <HelmReleases
                key="helm"
                namespace={namespace === "*" ? undefined : namespace}
                onSelectRelease={handleHelmSelect}
              />
            ) : viewMode === "helm-detail" && selectedHelmRelease ? (
              <HelmDetailView
                key="helm-detail"
                release={selectedHelmRelease}
                onBack={handleBack}
              />
            ) : viewMode === "settings" ? (
              <SettingsPage
                key="settings"
                onBack={() => setViewMode("table")}
              />
            ) : isTableView ? (
              loading ? (
                <SkeletonTable key="skeleton" />
              ) : (
                <ResourceTable
                  key="table"
                  data={filteredResources}
                  sorting={sorting}
                  onSortingChange={setSorting}
                  onRowSelect={handleRowSelect}
                  kind={kind}
                  selectedRowIndex={selectedRowIndex}
                  onSelectedRowIndexChange={setSelectedRowIndex}
                  onRowAction={handleRowAction}
                  onTogglePin={togglePin}
                  isPinned={isPinned}
                  getRestartHistory={kind === "pods" ? getRestartHistory : undefined}
                  multiCluster={multiCluster}
                />
              )
            ) : selected && kind === "pods" ? (
              <PodDetailsView key="pod-details" pod={selected} onBack={handleBack} />
            ) : selected && kind === "deployments" ? (
              <DeploymentDetailsView
                key="deploy-details"
                deployment={selected}
                onBack={handleBack}
              />
            ) : selected ? (
              <ResourceDetailsView
                key="resource-details"
                resource={selected}
                kind={kind}
                onBack={handleBack}
              />
            ) : null}
          </AnimatePresence>
        </section>
      </main>

      <CommandPalette
        open={paletteOpen}
        contexts={contexts}
        namespace={namespace}
        onClose={() => setPaletteOpen(false)}
        onSelectContext={handleContextSwitch}
        onNavigateToKind={handleNavigateToKind}
        onNavigateToResource={handleNavigateToResource}
        onNavigateToView={handleViewChange}
        onAction={(action) => {
          if (action === "open-ai") setAiPanelOpen(true);
        }}
      />

      <ShortcutOverlay
        open={shortcutsOpen}
        onClose={() => setShortcutsOpen(false)}
      />

      <ConfirmDialog
        open={deleteConfirm.open}
        title={`Delete ${deleteConfirm.kind.slice(0, 1).toUpperCase() + deleteConfirm.kind.slice(1).replace(/s$/, "")}`}
        message={`Are you sure you want to delete "${deleteConfirm.name}" in namespace "${deleteConfirm.namespace}"?`}
        confirmText="Delete"
        cancelText="Cancel"
        onConfirm={handleConfirmDelete}
        onCancel={() => setDeleteConfirm((prev) => ({ ...prev, open: false }))}
        variant="danger"
      />

      <AIPanel
        open={aiPanelOpen}
        onClose={() => setAiPanelOpen(false)}
        resourceContext={
          isDetailView && selected
            ? { kind, namespace: selected.namespace || "default", name: selected.name }
            : undefined
        }
      />
    </div>
  );
}
