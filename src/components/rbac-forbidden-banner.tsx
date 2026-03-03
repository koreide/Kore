import { motion } from "framer-motion";
import { ShieldX, Lightbulb } from "lucide-react";
import type { ForbiddenAnalysis } from "@/lib/api";
import { RbacRuleChain } from "@/components/rbac-rule-chain";

interface RbacForbiddenBannerProps {
  analysis: ForbiddenAnalysis;
  onClose: () => void;
}

export function RbacForbiddenBanner({ analysis, onClose }: RbacForbiddenBannerProps) {
  const identityLabel =
    analysis.identity.kind === "ServiceAccount"
      ? `${(analysis.identity as { namespace: string }).namespace}/${analysis.identity.name}`
      : analysis.identity.name;

  return (
    <motion.div
      initial={{ opacity: 0, y: 12 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.3 }}
      className="rounded-lg border border-red-500/20 bg-red-500/5 p-4 space-y-3"
    >
      <div className="flex items-start gap-3">
        <ShieldX className="w-5 h-5 text-red-400 shrink-0 mt-0.5" />
        <div className="flex-1 min-w-0">
          <div className="text-sm font-medium text-red-300">Permission Denied</div>
          <div className="text-xs text-slate-400 mt-1 font-mono">
            {identityLabel} cannot {analysis.verb} {analysis.resource}
            {analysis.namespace && ` in namespace "${analysis.namespace}"`}
          </div>
        </div>
        <button onClick={onClose} className="text-slate-500 hover:text-slate-300 text-xs">
          Dismiss
        </button>
      </div>

      {/* Closest matching rules */}
      {analysis.closest_rules.length > 0 && (
        <div>
          <div className="text-[10px] uppercase tracking-wider text-slate-500 font-medium mb-1.5">
            Closest Matching Rules
          </div>
          <RbacRuleChain chain={analysis.closest_rules} allowed={false} />
        </div>
      )}

      {/* Suggestion */}
      <div className="flex items-start gap-2 px-3 py-2 rounded bg-amber-500/5 border border-amber-500/15">
        <Lightbulb className="w-3.5 h-3.5 text-amber-400 shrink-0 mt-0.5" />
        <div className="text-xs text-amber-200/80">{analysis.suggestion}</div>
      </div>
    </motion.div>
  );
}
