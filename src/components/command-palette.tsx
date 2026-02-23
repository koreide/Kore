import { useEffect, useState, useCallback } from "react";
import { motion, AnimatePresence } from "framer-motion";
import * as Command from "cmdk";
import {
  Boxes,
  Server,
  Activity,
  Cpu,
  FileText,
  Lock,
  Globe,
  Timer,
  Calendar,
  Zap,
  Search,
  Clock,
  ArrowRight,
  LayoutDashboard,
  GitBranch,
  Puzzle,
  Package,
  Settings,
  Sparkles,
  Filter,
} from "lucide-react";
import { searchResources } from "@/lib/api";
import { toResourceItem } from "@/lib/transforms";
import type { ResourceKind, ResourceItem, AppView } from "@/lib/types";

const QUICK_FILTERS: { prefix: string; label: string; description: string }[] = [
  { prefix: ":pods", label: ":pods", description: "Filter to pods" },
  { prefix: ":deployments", label: ":deployments", description: "Filter to deployments" },
  { prefix: ":services", label: ":services", description: "Filter to services" },
  { prefix: ":running", label: ":running", description: "Show running resources" },
  { prefix: ":failed", label: ":failed", description: "Show failed/error resources" },
  { prefix: ":pending", label: ":pending", description: "Show pending resources" },
];

const RESOURCE_KINDS: { id: ResourceKind; label: string; icon: typeof Boxes }[] = [
  { id: "pods", label: "Pods", icon: Boxes },
  { id: "deployments", label: "Deployments", icon: Server },
  { id: "services", label: "Services", icon: Activity },
  { id: "nodes", label: "Nodes", icon: Cpu },
  { id: "jobs", label: "Jobs", icon: Timer },
  { id: "cronjobs", label: "CronJobs", icon: Calendar },
  { id: "configmaps", label: "ConfigMaps", icon: FileText },
  { id: "secrets", label: "Secrets", icon: Lock },
  { id: "ingresses", label: "Ingresses", icon: Globe },
  { id: "events", label: "Events", icon: Zap },
];

const RECENT_KEY = "kore-recent-resources";

interface RecentItem {
  kind: ResourceKind;
  name: string;
  namespace: string;
  timestamp: number;
}

function getRecent(): RecentItem[] {
  try {
    const stored = localStorage.getItem(RECENT_KEY);
    if (!stored) return [];
    return JSON.parse(stored).slice(0, 20);
  } catch {
    return [];
  }
}

function addRecent(kind: ResourceKind, name: string, namespace: string) {
  const recent = getRecent().filter(
    (r) => !(r.kind === kind && r.name === name && r.namespace === namespace),
  );
  recent.unshift({ kind, name, namespace, timestamp: Date.now() });
  localStorage.setItem(RECENT_KEY, JSON.stringify(recent.slice(0, 20)));
}

const VIEW_COMMANDS: { id: AppView; label: string; icon: typeof LayoutDashboard }[] = [
  { id: "dashboard", label: "Dashboard", icon: LayoutDashboard },
  { id: "graph", label: "Resource Graph", icon: GitBranch },
  { id: "crds", label: "Custom Resources", icon: Puzzle },
  { id: "helm", label: "Helm Releases", icon: Package },
  { id: "settings", label: "Settings", icon: Settings },
];

interface CommandPaletteProps {
  open: boolean;
  contexts: string[];
  namespace?: string;
  onClose: () => void;
  onSelectContext: (context: string) => void;
  onNavigateToKind: (kind: ResourceKind) => void;
  onNavigateToResource: (kind: ResourceKind, resource: ResourceItem) => void;
  onNavigateToView?: (view: AppView) => void;
  onAction?: (action: string, args?: Record<string, unknown>) => void;
}

export function CommandPalette({
  open,
  contexts,
  namespace,
  onClose,
  onSelectContext,
  onNavigateToKind,
  onNavigateToResource,
  onNavigateToView,
  onAction,
}: CommandPaletteProps) {
  const [search, setSearch] = useState("");
  const [searchResults, setSearchResults] = useState<ResourceItem[]>([]);
  const [searching, setSearching] = useState(false);
  const [recent] = useState<RecentItem[]>(() => getRecent());

  useEffect(() => {
    if (!open) {
      setSearch("");
      setSearchResults([]);
    }
  }, [open]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onClose();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

  // Debounced search
  useEffect(() => {
    if (!search || search.length < 2) {
      setSearchResults([]);
      return;
    }

    const timer = setTimeout(async () => {
      setSearching(true);
      try {
        const ns = namespace === "*" ? undefined : namespace;
        const results = await searchResources(search, ns);
        const items = results
          .map((r) => toResourceItem(r))
          .filter((r): r is ResourceItem => r !== null);
        setSearchResults(items);
      } catch {
        setSearchResults([]);
      } finally {
        setSearching(false);
      }
    }, 300);

    return () => clearTimeout(timer);
  }, [search, namespace]);

  const handleSelectResource = useCallback(
    (kind: ResourceKind, item: ResourceItem) => {
      addRecent(kind, item.name, item.namespace || "default");
      onNavigateToResource(kind, item);
      onClose();
    },
    [onNavigateToResource, onClose],
  );

  return (
    <AnimatePresence>
      {open && (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.15 }}
          className="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-start justify-center pt-[20vh] z-50"
          onClick={onClose}
        >
          <motion.div
            initial={{ opacity: 0, y: -20, scale: 0.96 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: -20, scale: 0.96 }}
            transition={{ type: "spring", stiffness: 400, damping: 30 }}
            onClick={(e) => e.stopPropagation()}
            className="w-full max-w-xl"
          >
            <Command.Command className="rounded-xl border border-slate-800 glass shadow-2xl overflow-hidden">
              <Command.CommandInput
                autoFocus
                placeholder="Search resources, navigate, switch context..."
                value={search}
                onValueChange={setSearch}
                className="w-full px-4 py-3 bg-transparent outline-none text-slate-100 border-b border-slate-800/50 text-sm"
              />
              <Command.CommandList className="max-h-[400px] overflow-auto p-1">
                <Command.CommandEmpty className="px-4 py-6 text-sm text-slate-500 text-center">
                  {searching ? "Searching..." : "No results found"}
                </Command.CommandEmpty>

                {/* Search results */}
                {searchResults.length > 0 && (
                  <Command.CommandGroup heading="Search Results">
                    {searchResults.map((item) => {
                      const kind = (item._kind || "pods") as ResourceKind;
                      return (
                        <Command.CommandItem
                          key={`${kind}-${item.namespace}-${item.name}`}
                          value={`search-${kind}-${item.name}`}
                          onSelect={() => handleSelectResource(kind, item)}
                          className="px-3 py-2 cursor-pointer rounded-md hover:bg-muted/60 text-sm text-slate-200 transition flex items-center gap-3"
                        >
                          <Search className="w-4 h-4 text-slate-500 shrink-0" />
                          <div className="flex-1 min-w-0">
                            <div className="flex items-center gap-2">
                              <span className="font-mono text-xs truncate">{item.name}</span>
                              <span className="text-[10px] bg-slate-800 text-slate-400 px-1.5 py-0.5 rounded-full">
                                {kind}
                              </span>
                            </div>
                            {item.namespace && (
                              <div className="text-[10px] text-slate-500">{item.namespace}</div>
                            )}
                          </div>
                          <ArrowRight className="w-3 h-3 text-slate-600" />
                        </Command.CommandItem>
                      );
                    })}
                  </Command.CommandGroup>
                )}

                {/* Views */}
                {onNavigateToView && (
                  <Command.CommandGroup heading="Views">
                    {VIEW_COMMANDS.map(({ id, label, icon: Icon }) => (
                      <Command.CommandItem
                        key={id}
                        value={`view-${label}`}
                        onSelect={() => {
                          onNavigateToView(id);
                          onClose();
                        }}
                        className="px-3 py-2 cursor-pointer rounded-md hover:bg-muted/60 text-sm text-slate-200 transition flex items-center gap-3"
                      >
                        <Icon className="w-4 h-4 text-slate-500" />
                        <span>{label}</span>
                      </Command.CommandItem>
                    ))}
                  </Command.CommandGroup>
                )}

                {/* Navigation */}
                <Command.CommandGroup heading="Resources">
                  {RESOURCE_KINDS.map(({ id, label, icon: Icon }) => (
                    <Command.CommandItem
                      key={id}
                      value={`nav-${label}`}
                      onSelect={() => {
                        onNavigateToKind(id);
                        onClose();
                      }}
                      className="px-3 py-2 cursor-pointer rounded-md hover:bg-muted/60 text-sm text-slate-200 transition flex items-center gap-3"
                    >
                      <Icon className="w-4 h-4 text-slate-500" />
                      <span>Go to {label}</span>
                    </Command.CommandItem>
                  ))}
                </Command.CommandGroup>

                {/* Quick Filters (shown when typing ":") */}
                {search.startsWith(":") && (
                  <Command.CommandGroup heading="Quick Filters">
                    {QUICK_FILTERS.filter((f) => f.prefix.startsWith(search.toLowerCase())).map(
                      (filter) => (
                        <Command.CommandItem
                          key={filter.prefix}
                          value={`filter-${filter.prefix}`}
                          onSelect={() => {
                            const kindMap: Record<string, ResourceKind> = {
                              ":pods": "pods",
                              ":deployments": "deployments",
                              ":services": "services",
                            };
                            const filterKind = kindMap[filter.prefix];
                            if (filterKind) {
                              onNavigateToKind(filterKind);
                            } else {
                              onAction?.("quick-filter", { status: filter.prefix.slice(1) });
                            }
                            onClose();
                          }}
                          className="px-3 py-2 cursor-pointer rounded-md hover:bg-muted/60 text-sm text-slate-200 transition flex items-center gap-3"
                        >
                          <Filter className="w-4 h-4 text-slate-500" />
                          <span className="font-mono text-xs">{filter.label}</span>
                          <span className="text-[10px] text-slate-500 ml-auto">
                            {filter.description}
                          </span>
                        </Command.CommandItem>
                      ),
                    )}
                  </Command.CommandGroup>
                )}

                {/* Actions */}
                {onAction && (
                  <Command.CommandGroup heading="Actions">
                    <Command.CommandItem
                      value="action-ask-ai"
                      onSelect={() => {
                        onAction("open-ai");
                        onClose();
                      }}
                      className="px-3 py-2 cursor-pointer rounded-md hover:bg-muted/60 text-sm text-slate-200 transition flex items-center gap-3"
                    >
                      <Sparkles className="w-4 h-4 text-slate-500" />
                      <span>Ask AI</span>
                      <span className="text-[10px] text-slate-500 ml-auto">
                        Troubleshoot with AI
                      </span>
                    </Command.CommandItem>
                  </Command.CommandGroup>
                )}

                {/* Contexts */}
                <Command.CommandGroup heading="Contexts">
                  {contexts.map((ctx) => (
                    <Command.CommandItem
                      key={ctx}
                      value={`ctx-${ctx}`}
                      onSelect={() => {
                        onSelectContext(ctx);
                        onClose();
                      }}
                      className="px-3 py-2 cursor-pointer rounded-md hover:bg-muted/60 text-sm text-slate-200 transition flex items-center gap-3"
                    >
                      <span className="w-1.5 h-1.5 rounded-full bg-emerald-400 shrink-0" />
                      <span className="font-mono text-xs truncate">{ctx}</span>
                    </Command.CommandItem>
                  ))}
                </Command.CommandGroup>

                {/* Recent */}
                {recent.length > 0 && (
                  <Command.CommandGroup heading="Recent">
                    {recent.slice(0, 8).map((item) => (
                      <Command.CommandItem
                        key={`recent-${item.kind}-${item.namespace}-${item.name}`}
                        value={`recent-${item.kind}-${item.name}`}
                        onSelect={() => {
                          onNavigateToResource(item.kind, {
                            name: item.name,
                            namespace: item.namespace,
                          });
                          onClose();
                        }}
                        className="px-3 py-2 cursor-pointer rounded-md hover:bg-muted/60 text-sm text-slate-200 transition flex items-center gap-3"
                      >
                        <Clock className="w-4 h-4 text-slate-500 shrink-0" />
                        <div className="flex items-center gap-2 flex-1 min-w-0">
                          <span className="font-mono text-xs truncate">{item.name}</span>
                          <span className="text-[10px] bg-slate-800 text-slate-400 px-1.5 py-0.5 rounded-full">
                            {item.kind}
                          </span>
                        </div>
                      </Command.CommandItem>
                    ))}
                  </Command.CommandGroup>
                )}
              </Command.CommandList>
            </Command.Command>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
