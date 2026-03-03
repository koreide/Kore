import { useMemo, useState } from "react";
import { motion } from "framer-motion";
import { Search, Shield, ShieldCheck, User, Users, Box, Inbox } from "lucide-react";
import type { RoleSummary, ReverseLookupResult, RbacIdentity } from "@/lib/api";
import { cn } from "@/lib/utils";

interface RbacReverseLookupProps {
  roles: RoleSummary[];
  reverseLookup: ReverseLookupResult | null;
  loading: boolean;
  onSelectRole: (kind: string, name: string, namespace?: string) => void;
  onSelectIdentity: (identity: RbacIdentity) => void;
}

export function RbacReverseLookup({
  roles,
  reverseLookup,
  loading,
  onSelectRole,
  onSelectIdentity,
}: RbacReverseLookupProps) {
  const [search, setSearch] = useState("");
  const [selectedKey, setSelectedKey] = useState<string | null>(null);

  const filtered = useMemo(() => {
    const q = search.toLowerCase();
    return roles.filter((r) => r.name.toLowerCase().includes(q));
  }, [roles, search]);

  const clusterRoles = useMemo(() => filtered.filter((r) => r.kind === "ClusterRole"), [filtered]);
  const namespacedRoles = useMemo(() => filtered.filter((r) => r.kind === "Role"), [filtered]);

  function handleSelectRole(role: RoleSummary) {
    const key = `${role.kind}/${role.namespace || ""}/${role.name}`;
    setSelectedKey(key);
    onSelectRole(role.kind, role.name, role.namespace ?? undefined);
  }

  return (
    <div className="flex h-full gap-0 border border-slate-800 rounded-lg overflow-hidden">
      {/* Left panel: Role list */}
      <div className="w-[280px] shrink-0 border-r border-slate-800 flex flex-col">
        <div className="p-2 border-b border-slate-800">
          <div className="relative">
            <Search className="absolute left-2 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-slate-500" />
            <input
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Filter roles..."
              className="w-full pl-7 pr-3 py-1.5 bg-transparent text-sm text-slate-200 placeholder:text-slate-600 outline-none"
            />
          </div>
        </div>
        <div className="overflow-y-auto flex-1">
          {clusterRoles.length > 0 && (
            <div className="p-1">
              <div className="px-2 py-1 text-[10px] uppercase tracking-wider text-slate-500 font-medium flex items-center gap-1.5">
                <ShieldCheck className="w-3 h-3" />
                ClusterRoles ({clusterRoles.length})
              </div>
              {clusterRoles.map((role) => {
                const key = `ClusterRole//${role.name}`;
                return (
                  <button
                    key={key}
                    onClick={() => handleSelectRole(role)}
                    className={cn(
                      "w-full flex items-center justify-between px-2 py-1.5 rounded text-sm text-left",
                      "hover:bg-slate-700/40 transition-colors",
                      selectedKey === key && "bg-slate-700/60",
                    )}
                  >
                    <span className="font-mono text-slate-300 truncate text-xs">{role.name}</span>
                    <span className="text-[10px] text-slate-600 shrink-0 ml-2">
                      {role.rule_count}
                    </span>
                  </button>
                );
              })}
            </div>
          )}
          {namespacedRoles.length > 0 && (
            <div className="p-1">
              <div className="px-2 py-1 text-[10px] uppercase tracking-wider text-slate-500 font-medium flex items-center gap-1.5">
                <Shield className="w-3 h-3" />
                Roles ({namespacedRoles.length})
              </div>
              {namespacedRoles.map((role) => {
                const key = `Role/${role.namespace || ""}/${role.name}`;
                return (
                  <button
                    key={key}
                    onClick={() => handleSelectRole(role)}
                    className={cn(
                      "w-full flex items-center justify-between px-2 py-1.5 rounded text-sm text-left",
                      "hover:bg-slate-700/40 transition-colors",
                      selectedKey === key && "bg-slate-700/60",
                    )}
                  >
                    <span className="font-mono text-slate-300 truncate text-xs">
                      <span className="text-slate-600">{role.namespace}/</span>
                      {role.name}
                    </span>
                    <span className="text-[10px] text-slate-600 shrink-0 ml-2">
                      {role.rule_count}
                    </span>
                  </button>
                );
              })}
            </div>
          )}
          {!clusterRoles.length && !namespacedRoles.length && (
            <div className="p-6 text-center text-sm text-slate-500 flex flex-col items-center gap-2">
              <Inbox className="w-5 h-5" />
              No roles found
            </div>
          )}
        </div>
      </div>

      {/* Right panel: Reverse lookup detail */}
      <div className="flex-1 overflow-y-auto p-4">
        {loading && (
          <div className="space-y-3">
            {[1, 2, 3].map((i) => (
              <div key={i} className="h-12 rounded bg-slate-800/40 skeleton-shimmer" />
            ))}
          </div>
        )}
        {!loading && !reverseLookup && (
          <div className="h-full flex items-center justify-center text-sm text-slate-500">
            Select a role to see who holds it
          </div>
        )}
        {!loading && reverseLookup && (
          <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} className="space-y-4">
            {/* Role info */}
            <div className="flex items-center gap-2">
              <ShieldCheck className="w-4 h-4 text-purple-400" />
              <span className="text-sm font-mono text-slate-200">
                {reverseLookup.role_kind}/{reverseLookup.role_name}
              </span>
              {reverseLookup.role_namespace && (
                <span className="text-xs text-slate-500">(ns: {reverseLookup.role_namespace})</span>
              )}
            </div>

            {/* Rules */}
            {reverseLookup.rules.length > 0 && (
              <div>
                <div className="text-[10px] uppercase tracking-wider text-slate-500 font-medium mb-2">
                  Rules ({reverseLookup.rules.length})
                </div>
                <div className="space-y-1">
                  {reverseLookup.rules.map((rule, i) => (
                    <div
                      key={i}
                      className="px-2.5 py-1.5 rounded bg-purple-500/5 border border-purple-500/10 text-xs font-mono"
                    >
                      <span className="text-purple-300">{rule.resources.join(", ")}</span>
                      <span className="text-slate-500"> [</span>
                      <span className="text-slate-300">{rule.verbs.join(", ")}</span>
                      <span className="text-slate-500">]</span>
                      {rule.api_groups.length > 0 && rule.api_groups[0] !== "" && (
                        <span className="text-slate-600 ml-1">
                          in &quot;{rule.api_groups.join(", ")}&quot;
                        </span>
                      )}
                    </div>
                  ))}
                </div>
              </div>
            )}

            {/* Subjects */}
            <div>
              <div className="text-[10px] uppercase tracking-wider text-slate-500 font-medium mb-2">
                Subjects ({reverseLookup.subjects.length})
              </div>
              {reverseLookup.subjects.length === 0 ? (
                <div className="text-sm text-slate-500 italic">No subjects bound to this role</div>
              ) : (
                <div className="space-y-1">
                  {reverseLookup.subjects.map((sub, i) => (
                    <button
                      key={i}
                      onClick={() => onSelectIdentity(sub.identity)}
                      className="w-full flex items-center gap-2 px-2.5 py-2 rounded bg-slate-800/30 border border-slate-800 hover:bg-slate-700/40 transition-colors text-left"
                    >
                      {sub.identity.kind === "ServiceAccount" && (
                        <Box className="w-3.5 h-3.5 text-emerald-400 shrink-0" />
                      )}
                      {sub.identity.kind === "User" && (
                        <User className="w-3.5 h-3.5 text-blue-400 shrink-0" />
                      )}
                      {sub.identity.kind === "Group" && (
                        <Users className="w-3.5 h-3.5 text-purple-400 shrink-0" />
                      )}
                      <div className="flex-1 min-w-0">
                        <div className="text-xs font-mono text-slate-200 truncate">
                          {sub.identity.kind === "ServiceAccount"
                            ? `${(sub.identity as { namespace: string }).namespace}/${sub.identity.name}`
                            : sub.identity.name}
                        </div>
                        <div className="text-[10px] text-slate-500">
                          via {sub.binding_kind} &quot;{sub.binding_name}&quot;
                          <span className="ml-1">({sub.scope})</span>
                        </div>
                      </div>
                    </button>
                  ))}
                </div>
              )}
            </div>
          </motion.div>
        )}
      </div>
    </div>
  );
}
