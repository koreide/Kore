// ── K8s quantity parsing ─────────────────────────────────────────────

export function parseKubeQuantity(qty: string): number {
  const trimmed = qty.trim();
  if (trimmed.endsWith("Ki")) return parseFloat(trimmed) * 1024;
  if (trimmed.endsWith("Mi")) return parseFloat(trimmed) * 1024 * 1024;
  if (trimmed.endsWith("Gi")) return parseFloat(trimmed) * 1024 * 1024 * 1024;
  if (trimmed.endsWith("Ti")) return parseFloat(trimmed) * 1024 * 1024 * 1024 * 1024;
  if (trimmed.endsWith("m")) return parseFloat(trimmed) / 1000;
  if (trimmed.endsWith("n")) return parseFloat(trimmed) / 1_000_000_000;
  const num = parseFloat(trimmed);
  return isNaN(num) ? 0 : num;
}

export function parseUsagePercent(usage: string | undefined, capacity: string | undefined): number {
  if (!usage || !capacity) return 0;
  const u = parseKubeQuantity(usage);
  const c = parseKubeQuantity(capacity);
  if (c === 0) return 0;
  return Math.min(Math.round((u / c) * 100), 100);
}

// ── Health score UI mappings ────────────────────────────────────────────

export function scoreColor(score: number): string {
  if (score >= 80) return "text-emerald-400";
  if (score >= 50) return "text-amber-400";
  return "text-red-400";
}

export function scoreBorderColor(score: number): string {
  if (score >= 80) return "border-emerald-500/30";
  if (score >= 50) return "border-amber-500/30";
  return "border-red-500/30";
}

export function scoreGlowColor(score: number): string {
  if (score >= 80) return "shadow-emerald-500/10";
  if (score >= 50) return "shadow-amber-500/10";
  return "shadow-red-500/10";
}

export function scoreLabel(score: number): string {
  if (score >= 80) return "Healthy";
  if (score >= 50) return "Degraded";
  return "Critical";
}

export function scoreBgColor(score: number): string {
  if (score >= 80) return "bg-emerald-500/15 text-emerald-400 border-emerald-500/30";
  if (score >= 50) return "bg-amber-500/15 text-amber-400 border-amber-500/30";
  return "bg-red-500/15 text-red-400 border-red-500/30";
}

// ── Resource table utilities ────────────────────────────────────────────

export function formatRelativeAge(timestamp: string | undefined): string {
  if (!timestamp) return "-";

  try {
    const created = new Date(timestamp);
    if (isNaN(created.getTime())) return "-";
    const now = new Date();
    const diffMs = now.getTime() - created.getTime();
    const diffSeconds = Math.floor(diffMs / 1000);
    const diffMinutes = Math.floor(diffSeconds / 60);
    const diffHours = Math.floor(diffMinutes / 60);
    const diffDays = Math.floor(diffHours / 24);

    if (diffDays > 0) return `${diffDays}d`;
    if (diffHours > 0) return `${diffHours}h`;
    if (diffMinutes > 0) return `${diffMinutes}m`;
    return `${diffSeconds}s`;
  } catch {
    return "-";
  }
}

export type StatusVariant = "running" | "pending" | "failed" | "terminating" | "default";

export function getStatusVariant(status: string | undefined): StatusVariant {
  if (!status) return "default";
  const s = status.toLowerCase();
  if (
    s === "running" ||
    s === "ready" ||
    s === "succeeded" ||
    s === "completed" ||
    s === "complete"
  )
    return "running";
  if (s === "pending" || s === "containercreating" || s === "init" || s === "waiting")
    return "pending";
  if (
    s === "failed" ||
    s === "crashloopbackoff" ||
    s === "error" ||
    s === "imagepullbackoff" ||
    s === "evicted" ||
    s === "oomkilled"
  )
    return "failed";
  if (s === "terminating") return "terminating";
  return "default";
}
