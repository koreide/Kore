import { motion } from "framer-motion";
import { Link2, Shield, FileText } from "lucide-react";
import type { RuleChainEntry } from "@/lib/api";

interface RbacRuleChainProps {
  chain: RuleChainEntry[];
  allowed: boolean;
}

export function RbacRuleChain({ chain, allowed }: RbacRuleChainProps) {
  if (chain.length === 0) {
    return <div className="text-sm text-slate-500 italic">No matching rule found</div>;
  }

  return (
    <div className="space-y-1">
      {chain.map((entry, i) => (
        <motion.div
          key={`${entry.binding_name}-${entry.role_name}-${i}`}
          initial={{ opacity: 0, y: 4 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.15, delay: i * 0.05 }}
          className="space-y-1"
        >
          {/* Binding */}
          <div className="flex items-center gap-2 px-2.5 py-1.5 rounded bg-indigo-500/10 border border-indigo-500/20">
            <Link2 className="w-3.5 h-3.5 text-indigo-300 shrink-0" />
            <div className="text-xs font-mono">
              <span className="text-indigo-400">{entry.binding_kind}</span>
              <span className="text-slate-500"> / </span>
              <span className="text-indigo-200">{entry.binding_name}</span>
              {entry.binding_namespace && (
                <span className="text-slate-600 ml-1">(ns: {entry.binding_namespace})</span>
              )}
            </div>
          </div>

          {/* Connector */}
          <div className="flex items-center justify-center">
            <div className="w-px h-3 bg-slate-700" />
          </div>

          {/* Role */}
          <div className="flex items-center gap-2 px-2.5 py-1.5 rounded bg-purple-500/10 border border-purple-500/20">
            <Shield className="w-3.5 h-3.5 text-purple-300 shrink-0" />
            <div className="text-xs font-mono">
              <span className="text-purple-400">{entry.role_kind}</span>
              <span className="text-slate-500"> / </span>
              <span className="text-purple-200">{entry.role_name}</span>
              {entry.role_namespace && (
                <span className="text-slate-600 ml-1">(ns: {entry.role_namespace})</span>
              )}
            </div>
          </div>

          {/* Connector */}
          <div className="flex items-center justify-center">
            <div className="w-px h-3 bg-slate-700" />
          </div>

          {/* Rule */}
          <div
            className={`flex items-start gap-2 px-2.5 py-1.5 rounded border ${
              allowed
                ? "bg-emerald-500/10 border-emerald-500/20"
                : "bg-red-500/10 border-red-500/20"
            }`}
          >
            <FileText
              className={`w-3.5 h-3.5 shrink-0 mt-0.5 ${
                allowed ? "text-emerald-300" : "text-red-300"
              }`}
            />
            <div className="text-xs font-mono">
              <span className={allowed ? "text-emerald-300" : "text-red-300"}>
                {entry.matching_rule.resources.join(", ")}
              </span>
              <span className="text-slate-500"> [</span>
              <span className="text-slate-300">{entry.matching_rule.verbs.join(", ")}</span>
              <span className="text-slate-500">]</span>
              {entry.matching_rule.api_groups.length > 0 &&
                entry.matching_rule.api_groups[0] !== "" && (
                  <span className="text-slate-600 ml-1">
                    in &quot;{entry.matching_rule.api_groups.join(", ")}&quot;
                  </span>
                )}
              {entry.matching_rule.resource_names.length > 0 && (
                <span className="text-amber-400 ml-1">
                  (only: {entry.matching_rule.resource_names.join(", ")})
                </span>
              )}
            </div>
          </div>

          {/* Separator between entries */}
          {i < chain.length - 1 && <div className="border-b border-slate-800 my-2" />}
        </motion.div>
      ))}
    </div>
  );
}
