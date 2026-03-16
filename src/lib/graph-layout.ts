import type { GraphNode, GraphEdge } from "@/lib/api";

// ── Constants ───────────────────────────────────────────────────────────

export const NODE_WIDTH = 180;
export const NODE_HEIGHT = 56;
export const LAYER_GAP_Y = 100;
export const NODE_GAP_X = 40;
export const PADDING = 60;

// ── Kind → layer ordering (top to bottom) ───────────────────────────────

export const KIND_LAYER_ORDER: Record<string, number> = {
  Ingress: 0,
  Service: 1,
  Deployment: 2,
  CronJob: 2,
  ReplicaSet: 3,
  Job: 3,
  Pod: 4,
};

export function getLayerForKind(kind: string): number {
  return KIND_LAYER_ORDER[kind] ?? 2;
}

// ── Kind colors ─────────────────────────────────────────────────────────

export const KIND_COLORS: Record<string, { fill: string; stroke: string; text: string }> = {
  Deployment: { fill: "#1e3a5f", stroke: "#3b82f6", text: "#93c5fd" },
  Service: { fill: "#2d1b4e", stroke: "#a855f7", text: "#d8b4fe" },
  Pod: { fill: "#1a3a2a", stroke: "#10b981", text: "#6ee7b7" },
  Ingress: { fill: "#0c3642", stroke: "#22d3ee", text: "#67e8f9" },
  Job: { fill: "#3b2a0a", stroke: "#f97316", text: "#fdba74" },
  CronJob: { fill: "#3b2a0a", stroke: "#f97316", text: "#fdba74" },
  ReplicaSet: { fill: "#1e293b", stroke: "#64748b", text: "#94a3b8" },
};

const DEFAULT_KIND_COLOR = { fill: "#1e293b", stroke: "#64748b", text: "#94a3b8" };

export function getKindColor(kind: string) {
  return KIND_COLORS[kind] ?? DEFAULT_KIND_COLOR;
}

// ── Status → border color ───────────────────────────────────────────────

export function getStatusBorderColor(status: string): string {
  const s = status.toLowerCase();
  if (
    s === "running" ||
    s === "ready" ||
    s === "active" ||
    s === "succeeded" ||
    s === "complete" ||
    s === "completed" ||
    s === "available"
  )
    return "#10b981"; // emerald
  if (s === "pending" || s === "containercreating" || s === "init" || s === "waiting")
    return "#f59e0b"; // amber
  if (
    s === "failed" ||
    s === "crashloopbackoff" ||
    s === "error" ||
    s === "imagepullbackoff" ||
    s === "evicted" ||
    s === "oomkilled"
  )
    return "#ef4444"; // red
  return "#64748b"; // slate default
}

// ── Layout engine ───────────────────────────────────────────────────────

export interface LayoutNode extends GraphNode {
  x: number;
  y: number;
}

export function computeLayout(
  nodes: GraphNode[],
  edges: GraphEdge[],
): { layoutNodes: LayoutNode[]; width: number; height: number } {
  if (nodes.length === 0) {
    return { layoutNodes: [], width: 0, height: 0 };
  }

  // Group nodes by layer
  const layers = new Map<number, GraphNode[]>();
  for (const node of nodes) {
    const layer = getLayerForKind(node.kind);
    if (!layers.has(layer)) layers.set(layer, []);
    layers.get(layer)!.push(node);
  }

  // Sort layers by key
  const sortedLayerKeys = Array.from(layers.keys()).sort((a, b) => a - b);

  // Build an adjacency lookup for ordering within layers based on edge connections
  const parentMap = new Map<string, string[]>();
  for (const edge of edges) {
    if (!parentMap.has(edge.target)) parentMap.set(edge.target, []);
    parentMap.get(edge.target)!.push(edge.source);
  }

  // Assign positions
  const layoutNodes: LayoutNode[] = [];
  let maxLayerWidth = 0;

  // First pass: compute each layer's width to find the widest
  for (const layerKey of sortedLayerKeys) {
    const layerNodes = layers.get(layerKey)!;
    const layerWidth = layerNodes.length * NODE_WIDTH + (layerNodes.length - 1) * NODE_GAP_X;
    if (layerWidth > maxLayerWidth) maxLayerWidth = layerWidth;
  }

  const totalWidth = maxLayerWidth + PADDING * 2;

  // Second pass: position nodes centered within the total width
  for (let li = 0; li < sortedLayerKeys.length; li++) {
    const layerKey = sortedLayerKeys[li];
    const layerNodes = layers.get(layerKey)!;

    // Sort nodes within a layer: try to order based on connections to the parent layer
    // This reduces edge crossings
    const nodePositionScores = new Map<string, number>();
    // Pre-compute source node positions for ordering
    const positionLookup = new Map<string, number>();
    for (const ln of layoutNodes) {
      positionLookup.set(ln.id, ln.x);
    }

    for (const node of layerNodes) {
      const parents = parentMap.get(node.id) ?? [];
      if (parents.length > 0) {
        let avg = 0;
        let count = 0;
        for (const pId of parents) {
          const px = positionLookup.get(pId);
          if (px !== undefined) {
            avg += px;
            count++;
          }
        }
        nodePositionScores.set(node.id, count > 0 ? avg / count : 0);
      } else {
        nodePositionScores.set(node.id, 0);
      }
    }

    layerNodes.sort(
      (a, b) => (nodePositionScores.get(a.id) ?? 0) - (nodePositionScores.get(b.id) ?? 0),
    );

    const layerWidth = layerNodes.length * NODE_WIDTH + (layerNodes.length - 1) * NODE_GAP_X;
    const startX = (totalWidth - layerWidth) / 2;
    const y = PADDING + li * (NODE_HEIGHT + LAYER_GAP_Y);

    for (let ni = 0; ni < layerNodes.length; ni++) {
      const node = layerNodes[ni];
      const x = startX + ni * (NODE_WIDTH + NODE_GAP_X);
      layoutNodes.push({ ...node, x, y });
    }
  }

  const totalHeight =
    PADDING * 2 + sortedLayerKeys.length * NODE_HEIGHT + (sortedLayerKeys.length - 1) * LAYER_GAP_Y;

  return {
    layoutNodes,
    width: totalWidth,
    height: totalHeight,
  };
}

// ── Edge path computation ───────────────────────────────────────────────

export function computeEdgePath(source: LayoutNode, target: LayoutNode): string {
  const sx = source.x + NODE_WIDTH / 2;
  const sy = source.y + NODE_HEIGHT;
  const tx = target.x + NODE_WIDTH / 2;
  const ty = target.y;

  const midY = (sy + ty) / 2;

  return `M ${sx} ${sy} C ${sx} ${midY}, ${tx} ${midY}, ${tx} ${ty}`;
}
