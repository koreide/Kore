import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Sidebar } from "./components/sidebar";
import { ResourceTable } from "./components/resource-table";
import { CommandPalette } from "./components/command-palette";
import { PodDetailsView } from "./components/pod-details-view";
import { listContexts, listNamespaces, listResources, ResourceItem, ResourceKind, startWatch, switchContext } from "./lib/api";
import type { SortingState } from "@tanstack/react-table";
import { Loader2, Search } from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { AnimatePresence } from "framer-motion";

const DEFAULT_NAMESPACES = ["default", "kube-system", "kube-public", "kube-node-lease"];

type WatchEventPayload = {
  action: string;
  kind: string;
  object: Record<string, unknown>;
};

function toResourceItem(obj: Record<string, unknown>): ResourceItem | null {
  const metadata = (obj["metadata"] as Record<string, unknown>) ?? {};
  const status = obj["status"] as Record<string, unknown>;
  const spec = obj["spec"] as Record<string, unknown>;
  const name = (metadata["name"] as string) ?? "";
  if (!name) return null;
  
  // Calculate ready status - for pods, count ready containers
  let ready: string | undefined;
  const containerStatuses = status?.["containerStatuses"] as any[] | undefined;
  if (containerStatuses && Array.isArray(containerStatuses)) {
    // This is a pod - count ready containers
    const readyCount = containerStatuses.filter((cs: any) => cs.ready === true).length;
    const totalCount = containerStatuses.length;
    ready = `${readyCount}/${totalCount}`;
  } else {
    // For other resources (like deployments), use readyReplicas
    const readyReplicas = status?.["readyReplicas"] as number | undefined;
    const replicas = status?.["replicas"] as number | undefined;
    if (readyReplicas !== undefined && replicas !== undefined) {
      ready = `${readyReplicas}/${replicas}`;
    } else if (readyReplicas !== undefined) {
      ready = readyReplicas.toString();
    }
  }
  
  // Extract deployment-specific fields
  const upToDate = status?.["updatedReplicas"] as number | undefined;
  const available = status?.["availableReplicas"] as number | undefined;
  
  // Extract service-specific fields
  const type = spec?.["type"] as string | undefined;
  const clusterIp = spec?.["clusterIP"] as string | undefined;
  
  // Extract external IP from loadBalancer ingress or externalIPs
  let externalIp: string | undefined;
  const loadBalancerIngress = status?.["loadBalancer"] as Record<string, unknown> | undefined;
  const ingress = loadBalancerIngress?.["ingress"] as any[] | undefined;
  if (ingress && Array.isArray(ingress) && ingress.length > 0) {
    externalIp = ingress[0]?.["ip"] || ingress[0]?.["hostname"] || undefined;
  }
  if (!externalIp) {
    const externalIPs = spec?.["externalIPs"] as string[] | undefined;
    if (externalIPs && Array.isArray(externalIPs) && externalIPs.length > 0) {
      externalIp = externalIPs[0];
    }
  }
  if (!externalIp && type === "LoadBalancer") {
    externalIp = "<pending>";
  }
  if (!externalIp) {
    externalIp = "<none>";
  }
  
  // Format ports from spec.ports
  let ports: string | undefined;
  const specPorts = spec?.["ports"] as any[] | undefined;
  if (specPorts && Array.isArray(specPorts)) {
    ports = specPorts.map((port: any) => {
      const portNum = port.port;
      const nodePort = port.nodePort;
      const protocol = port.protocol || "TCP";
      if (nodePort) {
        return `${nodePort}:${portNum}/${protocol}`;
      }
      return `${portNum}/${protocol}`;
    }).join(",");
  }
  
  // Extract node-specific fields
  let nodeStatus: string | undefined;
  const conditions = status?.["conditions"] as any[] | undefined;
  if (conditions && Array.isArray(conditions)) {
    const readyCondition = conditions.find((c: any) => c.type === "Ready");
    if (readyCondition) {
      nodeStatus = readyCondition.status === "True" ? "Ready" : "NotReady";
    }
  }
  if (!nodeStatus) {
    nodeStatus = status?.["phase"] as string | undefined;
  }
  
  // Extract roles from labels
  let roles: string | undefined;
  const labels = metadata["labels"] as Record<string, string> | undefined;
  if (labels) {
    const roleLabels: string[] = [];
    for (const [key, value] of Object.entries(labels)) {
      if (key.startsWith("node-role.kubernetes.io/")) {
        const role = key.replace("node-role.kubernetes.io/", "");
        roleLabels.push(role);
      } else if (key === "kubernetes.io/role") {
        roleLabels.push(value);
      }
    }
    if (roleLabels.length > 0) {
      roles = roleLabels.join(",");
    }
  }
  if (!roles) {
    roles = "<none>";
  }
  
  // Extract version from nodeInfo
  const nodeInfo = status?.["nodeInfo"] as Record<string, unknown> | undefined;
  const version = nodeInfo?.["kubeletVersion"] as string | undefined;
  
  // Extract pod IP
  const podIp = status?.["podIP"] as string | undefined;
  
  return {
    name,
    namespace: (metadata["namespace"] as string) ?? "",
    status: nodeStatus ?? (status?.["phase"] as string) ?? (status?.["conditions"] as any)?.[0]?.["type"],
    age: metadata["creationTimestamp"] as string,
    ready,
    restarts: (status?.["containerStatuses"] as any)?.[0]?.["restartCount"],
    node: (status?.["hostIP"] as string) ?? undefined,
    ip: podIp, // Add IP field for pods
    upToDate,
    available,
    type,
    clusterIp,
    externalIp,
    ports,
    roles,
    version
  };
}

export default function App() {
  const [contexts, setContexts] = useState<string[]>([]);
  const [currentContext, setCurrentContext] = useState<string>();
  const [namespaces, setNamespaces] = useState<string[]>(DEFAULT_NAMESPACES);
  const [namespace, setNamespace] = useState<string>("default");
  const [kind, setKind] = useState<ResourceKind>("pods");
  const [resources, setResources] = useState<ResourceItem[]>([]);
  const [selected, setSelected] = useState<ResourceItem | null>(null);
  const [sorting, setSorting] = useState<SortingState>([]);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [search, setSearch] = useState("");
  const [loading, setLoading] = useState(false);
  const [selectedRowIndex, setSelectedRowIndex] = useState(-1);
  const [viewMode, setViewMode] = useState<"table" | "details">("table");
  const searchRef = useRef<HTMLInputElement>(null);
  const watchInitializedRef = useRef(false);
  const currentKindRef = useRef<ResourceKind>(kind);

  // Handle context change - clear state and switch backend context
  const handleContextChange = useCallback(async (newContext: string) => {
    // Immediately clear all state
    setResources([]);
    setNamespaces([]);
    setSelected(null);
    setSearch("");
    setViewMode("table");
    watchInitializedRef.current = false;
    
    // Switch the backend context
    try {
      await switchContext(newContext);
      setCurrentContext(newContext);
    } catch (err) {
      console.error("Failed to switch context", err);
      // Don't update currentContext if switch failed
      return;
    }
  }, []);

  useEffect(() => {
    listContexts()
      .then(async (ctxs) => {
        if (ctxs && ctxs.length > 0) {
          setContexts(ctxs);
          // Use handleContextChange to properly initialize the first context
          await handleContextChange(ctxs[0]);
        } else {
          console.warn("No contexts found");
          setContexts([]);
        }
      })
      .catch((err) => {
        console.error("Failed to load contexts", err);
        // Don't block rendering if contexts fail to load
        setContexts([]);
      });
  }, [handleContextChange]);

  // Fetch namespaces when context changes
  useEffect(() => {
    if (currentContext) {
      // Clear namespaces immediately
      setNamespaces([]);
      
      listNamespaces()
        .then((ns) => {
          if (ns && ns.length > 0) {
            setNamespaces(ns);
            // Reset to default namespace if current namespace doesn't exist in new list
            // (but keep "*" if that's what's selected)
            setNamespace((currentNs) => {
              if (currentNs === "*" || ns.includes(currentNs)) {
                return currentNs;
              }
              return "default";
            });
          } else {
            console.warn("No namespaces found");
            setNamespaces(DEFAULT_NAMESPACES);
          }
        })
        .catch((err) => {
          console.error("Failed to load namespaces", err);
          // Fall back to default namespaces on error
          setNamespaces(DEFAULT_NAMESPACES);
        });
    }
  }, [currentContext]);

  // Register hotkeys
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPaletteOpen((v) => !v);
      }
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "r") {
        e.preventDefault();
        refresh();
      }
      if (e.key === "/") {
        e.preventDefault();
        searchRef.current?.focus();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [selected]);

  const refresh = useCallback(async () => {
    try {
      setLoading(true);
      // Clear existing resources when starting a new watch
      setResources([]);
      watchInitializedRef.current = false;
      // Use undefined for "all namespaces" (when namespace is "*")
      const apiNamespace = namespace === "*" ? undefined : namespace;
      console.log(`[REFRESH] Starting refresh for ${kind} in namespace ${namespace === "*" ? "all" : namespace}`);
      console.log(`[REFRESH] Current kind ref: ${currentKindRef.current}`);
      
      // Wait a bit longer to ensure event listener is ready
      await new Promise(resolve => setTimeout(resolve, 500));
      console.log(`[REFRESH] Event listener should be ready now`);
      
      console.log(`[REFRESH] Listing resources for ${kind} in namespace ${namespace === "*" ? "all" : namespace}`);
      // First, list existing resources to get current state
      const items = await listResources(kind, apiNamespace);
      console.log(`[REFRESH] Received ${items.length} resources`);
      
      // Convert and set resources directly
      const resources = items
        .map((obj) => toResourceItem(obj))
        .filter((item): item is ResourceItem => item !== null);
      
      console.log(`[REFRESH] Converted ${resources.length} resources`);
      setResources(resources);
      
      // Small delay between list and watch
      await new Promise(resolve => setTimeout(resolve, 100));
      
      console.log(`[REFRESH] Starting watch for ${kind} in namespace ${namespace === "*" ? "all" : namespace}`);
      // Then start watching for changes
      await startWatch(kind, apiNamespace);
      console.log(`[REFRESH] Watch started successfully`);
      // Resources will be populated via list events and watch events
    } catch (err) {
      console.error("[REFRESH] Failed to refresh resources", err);
      // Show error but don't block UI
    } finally {
      setLoading(false);
    }
  }, [kind, namespace]);

  // Set up event listener once - it will check the current kind via ref
  useEffect(() => {
    console.log(`[LISTENER] Setting up event listener for resource://event`);
    let unlistenFn: (() => void) | null = null;
    let isMounted = true;
    
    const setupListener = async () => {
      try {
        console.log(`[LISTENER] Calling listen()...`);
        const unlisten = await listen<WatchEventPayload>("resource://event", (event) => {
          console.log(`[LISTENER] Event callback triggered!`);
          if (!isMounted) {
            console.log(`[LISTENER] Component unmounted, ignoring event`);
            return;
          }
          
          const payload = event.payload;
          const currentKind = currentKindRef.current;
          console.log(`[LISTENER] Received event: kind=${payload.kind}, action=${payload.action}, current kind=${currentKind}`);
          console.log(`[LISTENER] Full event:`, event);
          
          // Check if this event is for the current resource kind
          if (payload.kind !== currentKind) {
            console.log(`[LISTENER] Ignoring event for ${payload.kind}, current kind is ${currentKind}`);
            return;
          }
          
          console.log(`[LISTENER] Processing event for ${currentKind}: action=${payload.action}`);
          
          // Mark as initialized once we receive any event for this kind
          if (!watchInitializedRef.current) {
            watchInitializedRef.current = true;
            console.log(`[LISTENER] Watch initialized for ${currentKind}, received ${payload.action} event`);
          }
          
          setResources((prevResources) => {
            const next = [...prevResources];
            const item = toResourceItem(payload.object);
            if (!item) {
              console.warn(`[LISTENER] Failed to convert resource item for ${currentKind}:`, payload.object);
              return prevResources;
            }
            console.log(`[LISTENER] Processing ${payload.action} event for ${item.name} in namespace ${item.namespace}`);
            const existing = next.findIndex((r) => r.name === item.name && r.namespace === item.namespace);
            if (payload.action === "deleted") {
              if (existing >= 0) {
                next.splice(existing, 1);
                console.log(`[LISTENER] Removed ${item.name} from resources (total: ${next.length})`);
              }
            } else {
              if (existing >= 0) {
                next[existing] = item;
                console.log(`[LISTENER] Updated ${item.name} in resources (total: ${next.length})`);
              } else {
                next.push(item);
                console.log(`[LISTENER] Added ${item.name} to resources (total: ${next.length})`);
              }
            }
            return next;
          });
        });
        
        console.log(`[LISTENER] Event listener set up successfully, unlisten function received`);
        unlistenFn = unlisten;
      } catch (err) {
        console.error(`[LISTENER] Failed to set up event listener:`, err);
        console.error(`[LISTENER] Error details:`, JSON.stringify(err, null, 2));
      }
    };
    
    setupListener().catch((err) => {
      console.error(`[LISTENER] Setup listener promise rejected:`, err);
    });
    
    return () => {
      console.log(`[LISTENER] Cleaning up event listener`);
      isMounted = false;
      if (unlistenFn) {
        unlistenFn();
      }
    };
  }, []); // Set up once, use ref for current kind
  
  // Debug: Log when resources change
  useEffect(() => {
    console.log(`[RESOURCES] Resources updated: ${resources.length} items`, resources);
  }, [resources]);

  useEffect(() => {
    currentKindRef.current = kind;
  }, [kind]);

  // Reset view mode when resource kind changes
  useEffect(() => {
    setViewMode("table");
    setSelected(null);
  }, [kind]);

  // Refresh resources when context changes
  useEffect(() => {
    if (currentContext) {
      // Clear resources immediately when context changes
      setResources([]);
      setSelected(null);
      watchInitializedRef.current = false;
      refresh();
    }
  }, [refresh, currentContext]);

  // Automatically start watch when kind or namespace changes (for live updates)
  useEffect(() => {
    if (currentContext) {
      // Clear resources and restart watch when kind or namespace changes
      setResources([]);
      watchInitializedRef.current = false;
      refresh();
    }
  }, [kind, namespace, refresh, currentContext]);

  const filtered = useMemo(() => {
    if (!search) return resources;
    const lower = search.toLowerCase();
    return resources.filter(
      (r) =>
        r.name.toLowerCase().includes(lower) ||
        (r.namespace && r.namespace.toLowerCase().includes(lower)) ||
        (r.status && r.status.toLowerCase().includes(lower))
    );
  }, [resources, search]);

  const handleRowSelect = async (row: ResourceItem) => {
    setSelected(row);
    if (kind === "pods" && row.name && row.namespace) {
      // Navigate to details view for pods
      setViewMode("details");
    }
  };

  const handleBack = () => {
    setViewMode("table");
    setSelected(null);
  };

  return (
    <div className="h-screen w-screen bg-background text-slate-100 flex overflow-hidden">
      <Sidebar
        contexts={contexts}
        currentContext={currentContext}
        namespaces={namespaces}
        currentNamespace={namespace}
        currentResource={kind}
        onContextChange={handleContextChange}
        onNamespaceChange={setNamespace}
        onResourceChange={setKind}
      />
      <main className="flex-1 p-4 grid grid-rows-[auto,1fr] gap-4">
        <header className="flex items-center gap-3">
          <div className="relative flex-1">
            <Search className="w-4 h-4 text-slate-400 absolute left-3 top-1/2 -translate-y-1/2" />
            <input
              ref={searchRef}
              placeholder="Search (/)"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              className="w-full bg-surface/70 glass border border-slate-800 rounded-md px-10 py-2 outline-none focus:border-accent transition"
            />
          </div>
          <div className="px-3 py-2 rounded border border-slate-800 bg-surface/70">
            {currentContext ?? "No context"}
          </div>
        </header>

        <section className="overflow-hidden relative">
          <AnimatePresence mode="wait">
            {viewMode === "table" ? (
              loading ? (
                <div key="loading" className="h-full flex items-center justify-center text-slate-400 gap-2">
                  <Loader2 className="w-5 h-5 animate-spin" />
                  Refreshing...
                </div>
              ) : (
                <ResourceTable
                  key="table"
                  data={filtered}
                  sorting={sorting}
                  onSortingChange={setSorting}
                  onRowSelect={handleRowSelect}
                  kind={kind}
                  selectedRowIndex={selectedRowIndex}
                  onSelectedRowIndexChange={setSelectedRowIndex}
                />
              )
            ) : selected && kind === "pods" ? (
              <PodDetailsView key="details" pod={selected} onBack={handleBack} />
            ) : null}
          </AnimatePresence>
        </section>
      </main>

      <CommandPalette
        open={paletteOpen}
        contexts={contexts}
        onClose={() => setPaletteOpen(false)}
        onSelectContext={handleContextChange}
      />
    </div>
  );
}

