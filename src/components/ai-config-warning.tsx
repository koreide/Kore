import { AlertCircle } from "lucide-react";
import { cn } from "@/lib/utils";

interface AIConfigWarningProps {
  className?: string;
}

export function AIConfigWarning({ className }: AIConfigWarningProps) {
  return (
    <div className={cn("px-4 py-2.5 border-b border-slate-800/50 bg-amber-500/5", className)}>
      <div className="flex items-center gap-2 text-xs">
        <AlertCircle className="w-3.5 h-3.5 text-amber-400 shrink-0" />
        <span className="text-amber-300/90">
          Configure an AI provider in Settings to use this feature
        </span>
      </div>
    </div>
  );
}
