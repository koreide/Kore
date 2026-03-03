import { motion } from "framer-motion";
import { ShieldAlert, X } from "lucide-react";
import type { RbacIdentity } from "@/lib/api";

interface RbacImpersonationBannerProps {
  identity: RbacIdentity;
  onExit: () => void;
}

function identityLabel(identity: RbacIdentity): string {
  switch (identity.kind) {
    case "ServiceAccount":
      return `${identity.namespace}/${identity.name}`;
    case "User":
    case "Group":
      return identity.name;
  }
}

export function RbacImpersonationBanner({ identity, onExit }: RbacImpersonationBannerProps) {
  return (
    <motion.div
      initial={{ opacity: 0, y: -8 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -8 }}
      className="flex items-center gap-3 px-4 py-2 bg-amber-500/10 border-b border-amber-500/20"
    >
      <ShieldAlert className="w-4 h-4 text-amber-400 shrink-0" />
      <span className="text-xs text-amber-300">Impersonation Mode:</span>
      <span className="text-xs font-mono text-amber-200">viewing as {identityLabel(identity)}</span>
      <span className="text-[10px] text-amber-500/70 ml-1">
        (simulated — actual API calls use your real credentials)
      </span>
      <div className="flex-1" />
      <button
        onClick={onExit}
        className="flex items-center gap-1 px-2 py-0.5 rounded text-xs text-amber-400 hover:bg-amber-500/10 transition-colors"
      >
        <X className="w-3 h-3" />
        Exit
      </button>
    </motion.div>
  );
}
