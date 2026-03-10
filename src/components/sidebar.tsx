import {
  LucideIcon,
  Boxes,
  Server,
  Activity,
  Cpu,
  Command,
  Calendar,
  Timer,
  Globe,
  FileText,
  Lock,
  Zap,
  FolderOpen,
  Layers,
  LayoutDashboard,
  GitBranch,
  Puzzle,
  Package,
  Settings,
  Star,
  MessageCircle,
  Heart,
  Shield,
  ShieldCheck,
} from "lucide-react";
import { motion } from "framer-motion";
import type { ResourceKind, AppView } from "@/lib/types";
import { SearchableDropdown } from "./searchable-dropdown";
import { PinnedResources } from "./pinned-resources";
import type { PinnedResource } from "@/hooks/use-pinned-resources";
import { cn } from "@/lib/utils";

type MenuItem = {
  id: ResourceKind;
  label: string;
  icon: LucideIcon;
};

type MenuGroup = {
  label: string;
  items: MenuItem[];
};

const menuGroups: MenuGroup[] = [
  {
    label: "Workloads",
    items: [
      { id: "pods", label: "Pods", icon: Boxes },
      { id: "deployments", label: "Deployments", icon: Server },
      { id: "jobs", label: "Jobs", icon: Timer },
      { id: "cronjobs", label: "CronJobs", icon: Calendar },
    ],
  },
  {
    label: "Network",
    items: [
      { id: "services", label: "Services", icon: Activity },
      { id: "ingresses", label: "Ingresses", icon: Globe },
    ],
  },
  {
    label: "Config",
    items: [
      { id: "configmaps", label: "ConfigMaps", icon: FileText },
      { id: "secrets", label: "Secrets", icon: Lock },
    ],
  },
  {
    label: "Cluster",
    items: [
      { id: "nodes", label: "Nodes", icon: Cpu },
      { id: "namespaces", label: "Namespaces", icon: FolderOpen },
      { id: "events", label: "Events", icon: Zap },
    ],
  },
];

type SpecialView = {
  id: AppView;
  label: string;
  icon: LucideIcon;
};

const specialViews: SpecialView[] = [
  { id: "chat", label: "AI Chat", icon: MessageCircle },
  { id: "dashboard", label: "Dashboard", icon: LayoutDashboard },
  { id: "graph", label: "Graph", icon: GitBranch },
  { id: "network-policies", label: "Network Policies", icon: Shield },
  { id: "rbac", label: "RBAC", icon: ShieldCheck },
  { id: "crds", label: "Custom Resources", icon: Puzzle },
  { id: "helm", label: "Helm", icon: Package },
];

interface SidebarProps {
  contexts: string[];
  currentContext?: string;
  namespaces: string[];
  currentNamespace?: string;
  currentResource?: ResourceKind;
  currentView?: AppView;
  onContextChange: (context: string) => void;
  onNamespaceChange: (ns: string) => void;
  onResourceChange: (kind: ResourceKind) => void;
  onViewChange?: (view: AppView) => void;
  resourceCount?: number;
  onNamespaceDropdownOpen?: () => void;
  pinned?: PinnedResource[];
  onPinSelect?: (kind: string, name: string, namespace: string) => void;
  onPinRemove?: (kind: string, name: string, namespace: string) => void;
  multiCluster?: boolean;
  onMultiClusterToggle?: (enabled: boolean) => void;
  updateAvailable?: boolean;
  onUpdateClick?: () => void;
  updating?: boolean;
}

export function Sidebar({
  contexts,
  currentContext,
  namespaces,
  currentNamespace,
  currentResource,
  currentView = "table",
  onContextChange,
  onNamespaceChange,
  onResourceChange,
  onViewChange,
  resourceCount,
  onNamespaceDropdownOpen,
  pinned = [],
  onPinSelect,
  onPinRemove,
  multiCluster = false,
  onMultiClusterToggle,
  updateAvailable = false,
  onUpdateClick,
  updating = false,
}: SidebarProps) {
  const isResourceActive = (id: ResourceKind) =>
    currentResource === id && (currentView === "table" || currentView === "details");
  const isViewActive = (id: AppView) => currentView === id;

  return (
    <aside className="w-64 h-full border-r border-slate-800 bg-surface/80 glass flex flex-col">
      {/* Branding */}
      <div className="p-4 pb-3 border-b border-slate-800/50">
        <div className="flex items-center gap-3">
          <div className="w-8 h-8 rounded-lg bg-accent/15 flex items-center justify-center">
            <span className="text-accent font-bold text-sm">K</span>
          </div>
          <div className="flex-1 min-w-0">
            <div className="text-sm font-semibold text-slate-100">Kore</div>
            <div className="text-[10px] text-slate-500">Kubernetes Desktop</div>
          </div>
          {updateAvailable && (
            <button
              onClick={onUpdateClick}
              disabled={updating}
              title={updating ? "Updating..." : "Update available — click to install"}
              className="px-2 py-1 rounded-md text-[10px] font-medium bg-accent/15 border border-accent/40 text-accent hover:bg-accent/25 transition disabled:opacity-60 flex items-center gap-1 shrink-0"
            >
              {updating ? (
                <span className="w-3 h-3 border-[1.5px] border-accent/30 border-t-accent rounded-full animate-spin" />
              ) : (
                "Update"
              )}
            </button>
          )}
        </div>
      </div>

      {/* Dropdowns */}
      <div className="p-4 space-y-4 flex-shrink-0">
        <SearchableDropdown
          items={contexts}
          selected={currentContext}
          onSelect={onContextChange}
          placeholder="Select context..."
          label="Context"
          storageKey="kore-favorite-contexts"
        />

        <SearchableDropdown
          items={namespaces}
          selected={currentNamespace}
          onSelect={onNamespaceChange}
          placeholder="Select namespace..."
          label="Namespaces"
          allOption={{ label: "All Namespaces", value: "*" }}
          storageKey="kore-favorite-namespaces"
          onOpen={onNamespaceDropdownOpen}
        />

        {/* Multi-cluster toggle */}
        <button
          onClick={() => onMultiClusterToggle?.(!multiCluster)}
          className={cn(
            "w-full flex items-center gap-2 px-3 py-1.5 rounded-md text-xs transition border",
            multiCluster
              ? "border-accent/50 text-accent bg-accent/10"
              : "border-slate-800 text-slate-500 hover:text-slate-300 hover:border-slate-700",
          )}
        >
          <Layers className="w-3.5 h-3.5" />
          <span className="flex-1 text-left">Multi-Cluster</span>
          <span
            className={cn(
              "w-2 h-2 rounded-full transition",
              multiCluster ? "bg-accent" : "bg-slate-700",
            )}
          />
        </button>
      </div>

      {/* Special Views */}
      <div className="px-4 mb-3">
        <p className="text-[10px] uppercase tracking-wider text-slate-500 mb-1.5 font-medium">
          Views
        </p>
        <div className="space-y-0.5 relative">
          {specialViews.map(({ id, label, icon: Icon }) => {
            const isActive = isViewActive(id);
            return (
              <button
                key={id}
                onClick={() => onViewChange?.(id)}
                className={cn(
                  "w-full px-3 py-1.5 rounded-md transition flex items-center gap-2.5 text-sm relative",
                  isActive
                    ? "text-accent"
                    : "text-slate-400 hover:text-slate-200 hover:bg-muted/30",
                )}
              >
                {isActive && (
                  <motion.div
                    layoutId="sidebar-active-indicator"
                    className="absolute left-0 top-0.5 bottom-0.5 w-0.5 bg-accent rounded-full"
                    transition={{ type: "spring", stiffness: 500, damping: 35 }}
                  />
                )}
                <Icon className="w-4 h-4" />
                <span className="flex-1 text-left">{label}</span>
              </button>
            );
          })}
        </div>
      </div>

      {/* Pinned Resources */}
      {pinned.length > 0 && (
        <div className="px-4 mb-3">
          <p className="text-[10px] uppercase tracking-wider text-slate-500 mb-1.5 font-medium flex items-center gap-1">
            <Star className="w-3 h-3" />
            Pinned
          </p>
          <PinnedResources
            pinned={pinned}
            onSelect={(kind, name, namespace) => onPinSelect?.(kind, name, namespace)}
            onRemove={(kind, name, namespace) => onPinRemove?.(kind, name, namespace)}
          />
        </div>
      )}

      {/* Resources — grouped */}
      <div className="px-4 flex-1 overflow-auto">
        {menuGroups.map((group) => (
          <div key={group.label} className="mb-3">
            <p className="text-[10px] uppercase tracking-wider text-slate-500 mb-1.5 font-medium">
              {group.label}
            </p>
            <div className="space-y-0.5 relative">
              {group.items.map(({ id, label, icon: Icon }) => {
                const isActive = isResourceActive(id);
                return (
                  <button
                    key={id}
                    onClick={() => onResourceChange(id)}
                    className={cn(
                      "w-full px-3 py-1.5 rounded-md transition flex items-center gap-2.5 text-sm relative",
                      isActive
                        ? "text-accent"
                        : "text-slate-400 hover:text-slate-200 hover:bg-muted/30",
                    )}
                  >
                    {isActive && (
                      <motion.div
                        layoutId="sidebar-active-indicator"
                        className="absolute left-0 top-0.5 bottom-0.5 w-0.5 bg-accent rounded-full"
                        transition={{ type: "spring", stiffness: 500, damping: 35 }}
                      />
                    )}
                    <Icon className="w-4 h-4" />
                    <span className="flex-1 text-left">{label}</span>
                    {isActive && resourceCount !== undefined && (
                      <span className="text-[10px] bg-accent/15 text-accent px-1.5 py-0.5 rounded-full font-medium">
                        {resourceCount}
                      </span>
                    )}
                  </button>
                );
              })}
            </div>
          </div>
        ))}
      </div>

      {/* Footer */}
      <div className="p-4 border-t border-slate-800/50">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2 text-[11px] text-slate-500">
            <Command className="w-3 h-3" />
            <span>
              <kbd className="px-1 py-0.5 bg-slate-800/80 rounded text-[10px] text-slate-400 font-mono">
                ⌘K
              </kbd>{" "}
              Command Palette
            </span>
          </div>
          <div className="flex items-center gap-1">
            <button
              title="Sponsor Kore"
              onClick={async () => {
                try {
                  const { open } = await import("@tauri-apps/plugin-shell");
                  await open("https://github.com/sponsors/eladbash");
                } catch {
                  window.open("https://github.com/sponsors/eladbash", "_blank");
                }
              }}
              className="p-1.5 rounded-md transition text-slate-500 hover:text-pink-400 hover:bg-pink-500/10"
            >
              <Heart className="w-4 h-4" />
            </button>
            <button
              onClick={() => onViewChange?.("settings")}
              className={cn(
                "p-1.5 rounded-md transition",
                currentView === "settings"
                  ? "text-accent bg-accent/10"
                  : "text-slate-500 hover:text-slate-300 hover:bg-muted/30",
              )}
            >
              <Settings className="w-4 h-4" />
            </button>
          </div>
        </div>
      </div>
    </aside>
  );
}
