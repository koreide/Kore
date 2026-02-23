import { useEffect, useState } from "react";
import { AlertCircle, AlertTriangle, Info, Clock, History, Radio } from "lucide-react";
import { listEventsForResource, queryStoredEvents } from "@/lib/api";
import { cn } from "@/lib/utils";

interface EventItem {
  reason?: string;
  message?: string;
  count?: number;
  lastTimestamp?: string;
  firstTimestamp?: string;
  type?: string;
  involvedObject?: { kind?: string; name?: string };
  metadata?: { creationTimestamp?: string };
}

interface EventsTimelineProps {
  kind: string;
  namespace: string;
  name: string;
}

function formatTimeAgo(timestamp: string | undefined): string {
  if (!timestamp) return "-";
  try {
    const d = new Date(timestamp);
    const now = new Date();
    const diffS = Math.floor((now.getTime() - d.getTime()) / 1000);
    if (diffS < 60) return `${diffS}s ago`;
    if (diffS < 3600) return `${Math.floor(diffS / 60)}m ago`;
    if (diffS < 86400) return `${Math.floor(diffS / 3600)}h ago`;
    return `${Math.floor(diffS / 86400)}d ago`;
  } catch {
    return "-";
  }
}

const typeIcon = {
  Normal: Info,
  Warning: AlertTriangle,
} as const;

const typeStyles = {
  Normal: { border: "border-blue-500/30", icon: "text-blue-400", bg: "bg-blue-500/5" },
  Warning: { border: "border-amber-500/30", icon: "text-amber-400", bg: "bg-amber-500/5" },
  Error: { border: "border-red-500/30", icon: "text-red-400", bg: "bg-red-500/5" },
} as const;

type TimeRange = "1h" | "6h" | "24h" | "7d";

const timeRangeMs: Record<TimeRange, number> = {
  "1h": 60 * 60 * 1000,
  "6h": 6 * 60 * 60 * 1000,
  "24h": 24 * 60 * 60 * 1000,
  "7d": 7 * 24 * 60 * 60 * 1000,
};

export function EventsTimeline({ kind, namespace, name }: EventsTimelineProps) {
  const [events, setEvents] = useState<EventItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [mode, setMode] = useState<"live" | "history">("live");
  const [timeRange, setTimeRange] = useState<TimeRange>("1h");

  useEffect(() => {
    setLoading(true);
    setError(null);

    if (mode === "live") {
      listEventsForResource(kind, namespace, name)
        .then((data) => {
          const items = data as unknown as EventItem[];
          items.sort((a, b) => {
            const timeA = a.lastTimestamp || a.metadata?.creationTimestamp || "";
            const timeB = b.lastTimestamp || b.metadata?.creationTimestamp || "";
            return new Date(timeB).getTime() - new Date(timeA).getTime();
          });
          setEvents(items);
          setLoading(false);
        })
        .catch((err) => {
          setError(String(err));
          setLoading(false);
        });
    } else {
      const now = Date.now();
      const since = new Date(now - timeRangeMs[timeRange]).toISOString();
      const until = new Date(now).toISOString();
      queryStoredEvents(since, until, namespace === "*" ? undefined : namespace)
        .then((stored) => {
          // Filter to events relevant to this resource if name is provided
          const items: EventItem[] = stored
            .filter((e) => {
              if (!name) return true;
              const obj = e.involved_object || "";
              return obj.includes(name);
            })
            .map((e) => ({
              reason: e.reason,
              message: e.message,
              count: e.count,
              lastTimestamp: e.last_seen,
              type: e.event_type,
              involvedObject: { name: e.involved_object },
            }));
          items.sort((a, b) => {
            const timeA = a.lastTimestamp || "";
            const timeB = b.lastTimestamp || "";
            return new Date(timeB).getTime() - new Date(timeA).getTime();
          });
          setEvents(items);
          setLoading(false);
        })
        .catch((err) => {
          setError(String(err));
          setLoading(false);
        });
    }
  }, [kind, namespace, name, mode, timeRange]);

  if (loading) {
    return (
      <div className="p-4 space-y-3">
        {[...Array(4)].map((_, i) => (
          <div key={i} className="skeleton h-16 rounded-lg" style={{ opacity: 1 - i * 0.15 }} />
        ))}
      </div>
    );
  }

  if (error) {
    return (
      <div className="flex items-center gap-3 p-4 text-red-400 text-sm">
        <AlertCircle className="w-4 h-4 shrink-0" />
        <span>Failed to load events: {error}</span>
      </div>
    );
  }

  if (events.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center h-full text-slate-400 gap-2 py-12">
        <Clock className="w-8 h-8 text-slate-600" />
        <p className="text-sm">No events found</p>
      </div>
    );
  }

  return (
    <div className="p-4 space-y-2 overflow-auto">
      {/* Live / History toggle */}
      <div className="flex items-center gap-2 mb-3">
        <button
          onClick={() => setMode("live")}
          className={cn(
            "flex items-center gap-1.5 px-3 py-1.5 rounded-md text-xs border transition",
            mode === "live"
              ? "border-accent/50 text-accent bg-accent/10"
              : "border-slate-700 text-slate-400 hover:text-slate-200 hover:border-slate-600",
          )}
        >
          <Radio className="w-3 h-3" />
          Live
        </button>
        <button
          onClick={() => setMode("history")}
          className={cn(
            "flex items-center gap-1.5 px-3 py-1.5 rounded-md text-xs border transition",
            mode === "history"
              ? "border-accent/50 text-accent bg-accent/10"
              : "border-slate-700 text-slate-400 hover:text-slate-200 hover:border-slate-600",
          )}
        >
          <History className="w-3 h-3" />
          History
        </button>

        {mode === "history" && (
          <div className="flex items-center gap-1 ml-2">
            {(["1h", "6h", "24h", "7d"] as TimeRange[]).map((range) => (
              <button
                key={range}
                onClick={() => setTimeRange(range)}
                className={cn(
                  "px-2 py-1 rounded text-[10px] font-mono transition",
                  timeRange === range
                    ? "bg-accent/15 text-accent"
                    : "text-slate-500 hover:text-slate-300",
                )}
              >
                {range}
              </button>
            ))}
          </div>
        )}
      </div>

      {events.map((event, i) => {
        const eventType = (event.type || "Normal") as keyof typeof typeStyles;
        const style = typeStyles[eventType] || typeStyles.Normal;
        const Icon = typeIcon[eventType as keyof typeof typeIcon] || Info;

        return (
          <div
            key={i}
            className={cn(
              "border rounded-lg p-3 transition hover:bg-white/[0.02]",
              style.border,
              style.bg,
            )}
          >
            <div className="flex items-start gap-3">
              <Icon className={cn("w-4 h-4 mt-0.5 shrink-0", style.icon)} />
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2 mb-1">
                  <span className="text-xs font-medium text-slate-200">
                    {event.reason || "Unknown"}
                  </span>
                  {event.count && event.count > 1 && (
                    <span className="text-[10px] bg-slate-800 text-slate-400 px-1.5 py-0.5 rounded-full">
                      x{event.count}
                    </span>
                  )}
                  <span className="text-[10px] text-slate-500 ml-auto shrink-0">
                    {formatTimeAgo(event.lastTimestamp || event.metadata?.creationTimestamp)}
                  </span>
                </div>
                <p className="text-xs text-slate-400 break-words">{event.message || "-"}</p>
              </div>
            </div>
          </div>
        );
      })}
    </div>
  );
}
