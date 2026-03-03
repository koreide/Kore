import { useCallback, useEffect, useState } from "react";
import { motion } from "framer-motion";
import {
  ShieldCheck,
  Grid3X3,
  BookOpen,
  MessageCircleQuestion,
  UserCog,
  Loader2,
  ArrowLeft,
  AlertCircle,
  Play,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { useRbac } from "@/hooks/use-rbac";
import { RbacIdentityPicker } from "@/components/rbac-identity-picker";
import { RbacMatrix } from "@/components/rbac-matrix";
import { RbacReverseLookup } from "@/components/rbac-reverse-lookup";
import { RbacForbiddenBanner } from "@/components/rbac-forbidden-banner";
import type { RbacIdentity, ForbiddenAnalysis, PermissionCheckResult } from "@/lib/api";
import { rbacCheckPermission, rbacWhoCan } from "@/lib/api";
import { RbacRuleChain } from "@/components/rbac-rule-chain";

type RbacTab = "matrix" | "roles" | "query" | "impersonation";

interface RbacViewProps {
  namespace?: string;
  onBack?: () => void;
  initialAnalysis?: ForbiddenAnalysis | null;
  impersonatedIdentity: RbacIdentity | null;
  onSetImpersonation: (identity: RbacIdentity | null) => void;
}

const TABS: { id: RbacTab; label: string; icon: React.ReactNode; key: string }[] = [
  { id: "matrix", label: "Permission Matrix", icon: <Grid3X3 className="w-3.5 h-3.5" />, key: "1" },
  { id: "roles", label: "Role Browser", icon: <BookOpen className="w-3.5 h-3.5" />, key: "2" },
  {
    id: "query",
    label: "Query",
    icon: <MessageCircleQuestion className="w-3.5 h-3.5" />,
    key: "3",
  },
  {
    id: "impersonation",
    label: "Impersonation",
    icon: <UserCog className="w-3.5 h-3.5" />,
    key: "4",
  },
];

function Kbd({ children }: { children: React.ReactNode }) {
  return (
    <kbd className="inline-flex items-center justify-center min-w-[24px] h-[20px] px-1 bg-slate-800/80 rounded text-[10px] text-slate-400 font-mono border border-slate-700/50 shadow-[0_1px_0_rgba(0,0,0,0.3)]">
      {children}
    </kbd>
  );
}

export function RbacView({
  namespace,
  onBack,
  initialAnalysis,
  impersonatedIdentity,
  onSetImpersonation,
}: RbacViewProps) {
  const [activeTab, setActiveTab] = useState<RbacTab>("matrix");
  const [queryMode, setQueryMode] = useState<"check" | "whocan">("check");
  const [queryVerb, setQueryVerb] = useState("get");
  const [queryResource, setQueryResource] = useState("pods");
  const [queryIdentity, setQueryIdentity] = useState<RbacIdentity | null>(null);
  const [queryResult, setQueryResult] = useState<PermissionCheckResult | null>(null);
  const [whoCanResults, setWhoCanResults] = useState<PermissionCheckResult[]>([]);
  const [queryLoading, setQueryLoading] = useState(false);
  const [queryRan, setQueryRan] = useState(false);

  const rbac = useRbac(namespace);

  // Fetch identities on mount
  useEffect(() => {
    rbac.fetchIdentities();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Fetch roles when switching to roles tab
  useEffect(() => {
    if (activeTab === "roles") {
      rbac.fetchRoles();
    }
  }, [activeTab]); // eslint-disable-line react-hooks/exhaustive-deps

  // Build matrix when identity changes
  useEffect(() => {
    if (rbac.selectedIdentity) {
      rbac.buildMatrix(rbac.selectedIdentity);
    }
  }, [rbac.selectedIdentity, namespace]); // eslint-disable-line react-hooks/exhaustive-deps

  // Handle initial forbidden analysis
  useEffect(() => {
    if (initialAnalysis) {
      rbac.setSelectedIdentity(initialAnalysis.identity);
    }
  }, [initialAnalysis]); // eslint-disable-line react-hooks/exhaustive-deps

  // Sync impersonated identity into the selected identity
  useEffect(() => {
    if (impersonatedIdentity) {
      rbac.setSelectedIdentity(impersonatedIdentity);
    }
  }, [impersonatedIdentity]); // eslint-disable-line react-hooks/exhaustive-deps

  // Keyboard shortcuts for tabs
  useEffect(() => {
    function handleKey(e: KeyboardEvent) {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) {
        return;
      }
      const tabIndex = ["1", "2", "3", "4"].indexOf(e.key);
      if (tabIndex !== -1) {
        setActiveTab(TABS[tabIndex].id);
      }
      if (e.key === "i" && !e.metaKey && !e.ctrlKey) {
        if (impersonatedIdentity) {
          onSetImpersonation(null);
        }
      }
    }
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [impersonatedIdentity, onSetImpersonation]);

  const handleIdentitySelect = useCallback(
    (identity: RbacIdentity) => {
      rbac.setSelectedIdentity(identity);
      setActiveTab("matrix");
    },
    [rbac],
  );

  const handleQuerySubmit = useCallback(async () => {
    setQueryLoading(true);
    setQueryResult(null);
    setWhoCanResults([]);
    try {
      if (queryMode === "check") {
        if (!queryIdentity) return;
        const result = await rbacCheckPermission(
          queryIdentity,
          queryVerb,
          queryResource,
          namespace,
        );
        setQueryResult(result);
      } else {
        const results = await rbacWhoCan(queryVerb, queryResource, namespace);
        setWhoCanResults(results);
      }
      setQueryRan(true);
    } finally {
      setQueryLoading(false);
    }
  }, [queryMode, queryIdentity, queryVerb, queryResource, namespace]);

  const handleEnterImpersonation = useCallback(() => {
    if (rbac.selectedIdentity) {
      onSetImpersonation(rbac.selectedIdentity);
    }
  }, [rbac.selectedIdentity, onSetImpersonation]);

  return (
    <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} className="flex flex-col h-full">
      {/* Header */}
      <div className="flex items-center gap-3 px-4 py-3 border-b border-slate-800">
        {onBack && (
          <button
            onClick={onBack}
            className="p-1 rounded hover:bg-slate-700/40 transition-colors text-slate-400 hover:text-slate-200"
          >
            <ArrowLeft className="w-4 h-4" />
          </button>
        )}
        <ShieldCheck className="w-5 h-5 text-accent" />
        <h1 className="text-sm font-medium text-slate-200">RBAC Simulator</h1>
        <div className="flex-1" />
        {rbac.error && (
          <div className="flex items-center gap-1.5 text-xs text-red-400">
            <AlertCircle className="w-3.5 h-3.5" />
            {rbac.error}
          </div>
        )}
      </div>

      {/* Tab bar */}
      <div className="flex items-center gap-1 px-4 py-2 border-b border-slate-800">
        {TABS.map((tab) => (
          <button
            key={tab.id}
            onClick={() => setActiveTab(tab.id)}
            className={cn(
              "flex items-center gap-1.5 px-3 py-1.5 rounded-md text-xs transition-colors",
              activeTab === tab.id
                ? "bg-slate-700/60 text-slate-200"
                : "text-slate-500 hover:text-slate-300 hover:bg-slate-800/50",
            )}
          >
            {tab.icon}
            {tab.label}
            <Kbd>{tab.key}</Kbd>
          </button>
        ))}
      </div>

      {/* Content */}
      <div className="flex-1 overflow-y-auto p-4">
        {/* Forbidden analysis banner */}
        {initialAnalysis && (
          <div className="mb-4">
            <RbacForbiddenBanner
              analysis={initialAnalysis}
              onClose={() => {
                /* handled by parent */
              }}
            />
          </div>
        )}

        {/* Tab 1: Permission Matrix */}
        {activeTab === "matrix" && (
          <div className="flex flex-col gap-4 h-[calc(100vh-200px)]">
            <div className="flex items-center gap-3 shrink-0">
              <RbacIdentityPicker
                identities={rbac.identities}
                selected={rbac.selectedIdentity}
                onSelect={handleIdentitySelect}
              />
              {rbac.loading && <Loader2 className="w-4 h-4 text-slate-500 animate-spin" />}
            </div>
            {rbac.matrix ? (
              <div className="flex-1 min-h-0">
                <RbacMatrix matrix={rbac.matrix} />
              </div>
            ) : !rbac.loading ? (
              <div className="flex items-center justify-center flex-1 text-sm text-slate-500">
                Select an identity to view their permissions
              </div>
            ) : null}
          </div>
        )}

        {/* Tab 2: Role Browser */}
        {activeTab === "roles" && (
          <div className="h-[calc(100vh-200px)]">
            <RbacReverseLookup
              roles={rbac.roles}
              reverseLookup={rbac.reverseLookup}
              loading={rbac.loading}
              onSelectRole={(kind, name, ns) => rbac.doReverseLookup(kind, name, ns)}
              onSelectIdentity={handleIdentitySelect}
            />
          </div>
        )}

        {/* Tab 3: Structured Query */}
        {activeTab === "query" && (
          <div className="space-y-4">
            {/* Mode toggle + form row */}
            <div className="flex items-center gap-2 flex-wrap">
              {/* Mode toggle */}
              <div className="flex rounded-md border border-slate-700/50 overflow-hidden shrink-0">
                <button
                  onClick={() => {
                    setQueryMode("check");
                    setQueryResult(null);
                    setWhoCanResults([]);
                    setQueryRan(false);
                  }}
                  className={cn(
                    "px-3 py-1.5 text-xs transition-colors",
                    queryMode === "check"
                      ? "bg-slate-700/60 text-slate-200"
                      : "text-slate-500 hover:text-slate-300 hover:bg-slate-800/50",
                  )}
                >
                  Check Permission
                </button>
                <button
                  onClick={() => {
                    setQueryMode("whocan");
                    setQueryResult(null);
                    setWhoCanResults([]);
                    setQueryRan(false);
                  }}
                  className={cn(
                    "px-3 py-1.5 text-xs transition-colors border-l border-slate-700/50",
                    queryMode === "whocan"
                      ? "bg-slate-700/60 text-slate-200"
                      : "text-slate-500 hover:text-slate-300 hover:bg-slate-800/50",
                  )}
                >
                  Who Can?
                </button>
              </div>

              {/* Identity picker — only in check mode */}
              {queryMode === "check" && (
                <RbacIdentityPicker
                  identities={rbac.identities}
                  selected={queryIdentity}
                  onSelect={setQueryIdentity}
                />
              )}

              {/* Verb dropdown */}
              <select
                value={queryVerb}
                onChange={(e) => setQueryVerb(e.target.value)}
                className="px-2 py-1.5 rounded-md bg-surface border border-slate-700/50 text-xs text-slate-200 outline-none focus:border-accent/50 transition-colors"
              >
                {["get", "list", "watch", "create", "update", "patch", "delete"].map((v) => (
                  <option key={v} value={v}>
                    {v}
                  </option>
                ))}
              </select>

              {/* Resource dropdown */}
              <select
                value={queryResource}
                onChange={(e) => setQueryResource(e.target.value)}
                className="px-2 py-1.5 rounded-md bg-surface border border-slate-700/50 text-xs text-slate-200 outline-none focus:border-accent/50 transition-colors"
              >
                {[
                  "pods",
                  "deployments",
                  "replicasets",
                  "statefulsets",
                  "daemonsets",
                  "services",
                  "ingresses",
                  "configmaps",
                  "secrets",
                  "nodes",
                  "namespaces",
                  "persistentvolumeclaims",
                  "persistentvolumes",
                  "serviceaccounts",
                  "jobs",
                  "cronjobs",
                  "roles",
                  "rolebindings",
                  "clusterroles",
                  "clusterrolebindings",
                  "events",
                  "networkpolicies",
                ].map((r) => (
                  <option key={r} value={r}>
                    {r}
                  </option>
                ))}
              </select>

              {/* Namespace indicator */}
              {namespace && (
                <span className="px-2 py-1.5 rounded-md bg-slate-800/50 text-[10px] text-slate-500 font-mono shrink-0">
                  ns: {namespace}
                </span>
              )}

              {/* Check button */}
              <button
                onClick={handleQuerySubmit}
                disabled={queryLoading || (queryMode === "check" && !queryIdentity)}
                className={cn(
                  "flex items-center gap-1.5 px-3 py-1.5 rounded-md text-xs font-medium transition-colors shrink-0",
                  queryLoading || (queryMode === "check" && !queryIdentity)
                    ? "bg-slate-700/30 text-slate-600 cursor-not-allowed"
                    : "bg-accent/10 border border-accent/30 text-accent hover:bg-accent/20",
                )}
              >
                {queryLoading ? (
                  <Loader2 className="w-3.5 h-3.5 animate-spin" />
                ) : (
                  <Play className="w-3.5 h-3.5" />
                )}
                Check
              </button>
            </div>

            {/* Results */}
            {queryLoading && (
              <div className="flex items-center gap-2 text-sm text-slate-500">
                <Loader2 className="w-4 h-4 animate-spin" />
                Checking permissions...
              </div>
            )}

            {/* Single check result */}
            {queryResult && (
              <motion.div
                initial={{ opacity: 0, y: 8 }}
                animate={{ opacity: 1, y: 0 }}
                className={cn(
                  "rounded-lg border p-4 max-w-2xl",
                  queryResult.allowed
                    ? "border-emerald-500/20 bg-emerald-500/5"
                    : "border-red-500/20 bg-red-500/5",
                )}
              >
                <div
                  className={cn(
                    "text-sm font-medium mb-2",
                    queryResult.allowed ? "text-emerald-300" : "text-red-300",
                  )}
                >
                  {queryResult.allowed ? "Allowed" : "Denied"}
                </div>
                <div className="text-xs text-slate-400 mb-3">{queryResult.summary}</div>
                <RbacRuleChain chain={queryResult.rule_chain} allowed={queryResult.allowed} />
              </motion.div>
            )}

            {/* Who can results */}
            {whoCanResults.length > 0 && (
              <motion.div
                initial={{ opacity: 0, y: 8 }}
                animate={{ opacity: 1, y: 0 }}
                className="space-y-2 max-w-2xl"
              >
                <div className="text-xs text-slate-500 font-medium">
                  {whoCanResults.length} identities can {queryVerb} {queryResource}
                  {namespace ? ` in ${namespace}` : ""}:
                </div>
                <div className="max-h-[calc(100vh-320px)] overflow-y-auto space-y-1.5 pr-1">
                  {whoCanResults.map((result, i) => (
                    <button
                      key={i}
                      onClick={() => handleIdentitySelect(result.identity)}
                      className="w-full flex items-center gap-2 px-3 py-2 rounded bg-emerald-500/5 border border-emerald-500/15 hover:bg-emerald-500/10 transition-colors text-left"
                    >
                      <div className="w-2 h-2 rounded-full bg-emerald-400 shrink-0" />
                      <span className="text-xs font-mono text-emerald-200">
                        {result.identity.kind === "ServiceAccount"
                          ? `${(result.identity as { namespace: string }).namespace}/${result.identity.name}`
                          : result.identity.name}
                      </span>
                      <span className="text-[10px] text-slate-500 ml-auto">
                        via {result.rule_chain[0]?.role_name}
                      </span>
                    </button>
                  ))}
                </div>
              </motion.div>
            )}

            {/* Empty who-can result */}
            {queryRan && !queryLoading && queryMode === "whocan" && whoCanResults.length === 0 && (
              <div className="text-sm text-slate-500 italic">
                No identities can {queryVerb} {queryResource}
                {namespace ? ` in ${namespace}` : ""}.
              </div>
            )}
          </div>
        )}

        {/* Tab 4: Impersonation */}
        {activeTab === "impersonation" && (
          <div className="space-y-5 max-w-lg">
            <div className="space-y-3">
              <div className="text-sm text-slate-300 font-medium">What does impersonation do?</div>
              <div className="text-xs text-slate-400 leading-relaxed">
                Impersonation lets you pick a ServiceAccount, User, or Group and see what they are
                allowed to do. When active, the Permission Matrix tab automatically shows that
                identity&apos;s permissions instead of requiring you to re-select it each time.
              </div>
              <div className="rounded-lg border border-slate-700/50 bg-slate-800/30 p-3 space-y-2">
                <div className="text-[10px] uppercase tracking-wider text-slate-500 font-medium">
                  While impersonating you can
                </div>
                <ul className="space-y-1.5 text-xs text-slate-400">
                  <li className="flex items-start gap-2">
                    <span className="text-emerald-400 mt-0.5">&#x2022;</span>
                    Browse the Permission Matrix pre-filled for the selected identity
                  </li>
                  <li className="flex items-start gap-2">
                    <span className="text-emerald-400 mt-0.5">&#x2022;</span>
                    Run permission queries scoped to that identity
                  </li>
                  <li className="flex items-start gap-2">
                    <span className="text-emerald-400 mt-0.5">&#x2022;</span>
                    Quickly check &quot;would this identity be able to...?&quot; without leaving
                    your current view
                  </li>
                </ul>
                <div className="text-[10px] text-amber-500/70 pt-1 border-t border-slate-700/30">
                  Read-only simulation — all actual API calls still use your real credentials.
                </div>
              </div>
            </div>
            <div className="space-y-3">
              <div className="text-xs text-slate-500 font-medium">
                Select an identity to impersonate
              </div>
              <div className="flex items-center gap-3">
                <RbacIdentityPicker
                  identities={rbac.identities}
                  selected={rbac.selectedIdentity}
                  onSelect={(id) => rbac.setSelectedIdentity(id)}
                />
              </div>
            </div>
            {rbac.selectedIdentity && (
              <div className="flex items-center gap-3">
                {impersonatedIdentity ? (
                  <button
                    onClick={() => onSetImpersonation(null)}
                    className="px-4 py-2 rounded-lg bg-amber-500/10 border border-amber-500/30 text-amber-300 text-sm hover:bg-amber-500/20 transition-colors"
                  >
                    Exit Impersonation
                  </button>
                ) : (
                  <button
                    onClick={handleEnterImpersonation}
                    className="px-4 py-2 rounded-lg bg-accent/10 border border-accent/30 text-accent text-sm hover:bg-accent/20 transition-colors"
                  >
                    Enter Impersonation Mode
                  </button>
                )}
              </div>
            )}
          </div>
        )}
      </div>
    </motion.div>
  );
}
