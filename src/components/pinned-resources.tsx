import {
  Boxes,
  Server,
  Activity,
  Cpu,
  Calendar,
  Timer,
  Globe,
  FileText,
  Lock,
  Zap,
  Pin,
  X,
  LucideIcon,
} from "lucide-react";
import type { PinnedResource } from "@/hooks/use-pinned-resources";
import { cn } from "@/lib/utils";

const kindIcons: Record<string, LucideIcon> = {
  pods: Boxes,
  deployments: Server,
  services: Activity,
  nodes: Cpu,
  jobs: Timer,
  cronjobs: Calendar,
  ingresses: Globe,
  configmaps: FileText,
  secrets: Lock,
  events: Zap,
};

interface PinnedResourcesProps {
  pinned: PinnedResource[];
  onSelect: (kind: string, name: string, namespace: string) => void;
  onRemove: (kind: string, name: string, namespace: string) => void;
}

export function PinnedResources({ pinned, onSelect, onRemove }: PinnedResourcesProps) {
  if (pinned.length === 0) {
    return (
      <div className="px-4 py-3">
        <p className="text-[10px] uppercase tracking-wider text-slate-500 mb-1.5 font-medium">
          Pinned
        </p>
        <p className="text-xs text-slate-600 italic">No pinned resources</p>
      </div>
    );
  }

  return (
    <div className="px-4 py-3 border-b border-slate-800/50">
      <p className="text-[10px] uppercase tracking-wider text-slate-500 mb-1.5 font-medium flex items-center gap-1.5">
        <Pin className="w-3 h-3" />
        Pinned
      </p>
      <div className="space-y-0.5">
        {pinned.map((pin) => {
          const Icon = kindIcons[pin.kind] || Boxes;
          return (
            <div
              key={`${pin.kind}-${pin.namespace}-${pin.name}`}
              className="group flex items-center gap-2 w-full px-2 py-1 rounded-md text-sm text-slate-400 hover:text-slate-200 hover:bg-muted/30 transition cursor-pointer"
              onClick={() => onSelect(pin.kind, pin.name, pin.namespace)}
            >
              <Icon className="w-3.5 h-3.5 flex-shrink-0 text-slate-500 group-hover:text-slate-400" />
              <span
                className={cn(
                  "flex-1 truncate font-mono text-xs",
                  "text-slate-300 group-hover:text-slate-100",
                )}
                title={pin.name}
              >
                {pin.name}
              </span>
              <span
                className="text-[10px] text-slate-600 truncate max-w-[60px]"
                title={pin.namespace}
              >
                {pin.namespace}
              </span>
              <button
                className="opacity-0 group-hover:opacity-100 transition-opacity p-0.5 rounded hover:bg-slate-700/50"
                onClick={(e) => {
                  e.stopPropagation();
                  onRemove(pin.kind, pin.name, pin.namespace);
                }}
                title="Unpin"
              >
                <X className="w-3 h-3 text-slate-500 hover:text-slate-300" />
              </button>
            </div>
          );
        })}
      </div>
    </div>
  );
}
