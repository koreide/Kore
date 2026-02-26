import { useEffect, useState, useRef, useCallback, useMemo } from "react";
import { motion, AnimatePresence } from "framer-motion";
import {
  Maximize2,
  Loader2,
  Inbox,
  Box,
  Globe,
  Network,
  Container,
  Server,
  Timer,
  Clock,
  Layers,
  Search,
  X,
} from "lucide-react";
import { buildResourceGraph } from "@/lib/api";
import type { ResourceGraph, GraphNode, GraphEdge } from "@/lib/api";

// ── Constants ───────────────────────────────────────────────────────────

const NODE_WIDTH = 180;
const NODE_HEIGHT = 56;
const LAYER_GAP_Y = 100;
const NODE_GAP_X = 40;
const PADDING = 60;

// ── Kind → layer ordering (top to bottom) ───────────────────────────────

const KIND_LAYER_ORDER: Record<string, number> = {
  Ingress: 0,
  Service: 1,
  Deployment: 2,
  CronJob: 2,
  ReplicaSet: 3,
  Job: 3,
  Pod: 4,
};

function getLayerForKind(kind: string): number {
  return KIND_LAYER_ORDER[kind] ?? 2;
}

// ── Kind colors ─────────────────────────────────────────────────────────

const KIND_COLORS: Record<string, { fill: string; stroke: string; text: string }> = {
  Deployment: { fill: "#1e3a5f", stroke: "#3b82f6", text: "#93c5fd" },
  Service: { fill: "#2d1b4e", stroke: "#a855f7", text: "#d8b4fe" },
  Pod: { fill: "#1a3a2a", stroke: "#10b981", text: "#6ee7b7" },
  Ingress: { fill: "#0c3642", stroke: "#22d3ee", text: "#67e8f9" },
  Job: { fill: "#3b2a0a", stroke: "#f97316", text: "#fdba74" },
  CronJob: { fill: "#3b2a0a", stroke: "#f97316", text: "#fdba74" },
  ReplicaSet: { fill: "#1e293b", stroke: "#64748b", text: "#94a3b8" },
};

const DEFAULT_KIND_COLOR = { fill: "#1e293b", stroke: "#64748b", text: "#94a3b8" };

function getKindColor(kind: string) {
  return KIND_COLORS[kind] ?? DEFAULT_KIND_COLOR;
}

// ── Status → border color ───────────────────────────────────────────────

function getStatusBorderColor(status: string): string {
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

// ── Kind icon component ─────────────────────────────────────────────────

function KindIcon({ kind, color }: { kind: string; color: string }) {
  const props = { width: 14, height: 14, color, strokeWidth: 1.5 };
  switch (kind) {
    case "Pod":
      return <Box {...props} />;
    case "Service":
      return <Network {...props} />;
    case "Deployment":
      return <Container {...props} />;
    case "Ingress":
      return <Globe {...props} />;
    case "ReplicaSet":
      return <Layers {...props} />;
    case "Job":
      return <Timer {...props} />;
    case "CronJob":
      return <Clock {...props} />;
    default:
      return <Server {...props} />;
  }
}

// ── Layout engine ───────────────────────────────────────────────────────

interface LayoutNode extends GraphNode {
  x: number;
  y: number;
}

function computeLayout(
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

function computeEdgePath(source: LayoutNode, target: LayoutNode): string {
  const sx = source.x + NODE_WIDTH / 2;
  const sy = source.y + NODE_HEIGHT;
  const tx = target.x + NODE_WIDTH / 2;
  const ty = target.y;

  const midY = (sy + ty) / 2;

  return `M ${sx} ${sy} C ${sx} ${midY}, ${tx} ${midY}, ${tx} ${ty}`;
}

// ── SVG Graph Node ──────────────────────────────────────────────────────

interface SVGGraphNodeProps {
  node: LayoutNode;
  onSelect: () => void;
  isHovered: boolean;
  onMouseEnter: () => void;
  onMouseLeave: () => void;
}

function SVGGraphNode({
  node,
  onSelect,
  isHovered,
  onMouseEnter,
  onMouseLeave,
}: SVGGraphNodeProps) {
  const kindColor = getKindColor(node.kind);
  const statusColor = getStatusBorderColor(node.status);

  // Truncate long names
  const displayName = node.name.length > 20 ? node.name.slice(0, 18) + "..." : node.name;

  return (
    <g
      onClick={onSelect}
      onMouseEnter={onMouseEnter}
      onMouseLeave={onMouseLeave}
      style={{ cursor: "pointer" }}
    >
      {/* Glow effect on hover */}
      {isHovered && (
        <rect
          x={node.x - 3}
          y={node.y - 3}
          width={NODE_WIDTH + 6}
          height={NODE_HEIGHT + 6}
          rx={12}
          ry={12}
          fill="none"
          stroke={statusColor}
          strokeWidth={1}
          opacity={0.4}
        />
      )}

      {/* Node background */}
      <rect
        x={node.x}
        y={node.y}
        width={NODE_WIDTH}
        height={NODE_HEIGHT}
        rx={10}
        ry={10}
        fill={kindColor.fill}
        stroke={statusColor}
        strokeWidth={isHovered ? 2 : 1.5}
        opacity={isHovered ? 1 : 0.9}
      />

      {/* Status indicator dot */}
      <circle cx={node.x + NODE_WIDTH - 12} cy={node.y + 12} r={3.5} fill={statusColor} />

      {/* Kind icon area (rendered as foreignObject for React icons) */}
      <foreignObject x={node.x + 10} y={node.y + 8} width={16} height={16}>
        <div
          // @ts-expect-error xmlns is valid for foreignObject children
          xmlns="http://www.w3.org/1999/xhtml"
          style={{ display: "flex", alignItems: "center", justifyContent: "center" }}
        >
          <KindIcon kind={node.kind} color={kindColor.text} />
        </div>
      </foreignObject>

      {/* Kind label */}
      <text
        x={node.x + 30}
        y={node.y + 19}
        fill={kindColor.text}
        fontSize={9}
        fontFamily="SFMono-Regular, Menlo, Monaco, Consolas, monospace"
        fontWeight={500}
        opacity={0.8}
      >
        {node.kind}
      </text>

      {/* Name */}
      <text
        x={node.x + 10}
        y={node.y + 40}
        fill="#e2e8f0"
        fontSize={11}
        fontFamily="SFMono-Regular, Menlo, Monaco, Consolas, monospace"
        fontWeight={500}
      >
        {displayName}
      </text>
    </g>
  );
}

// ── Tooltip ─────────────────────────────────────────────────────────────

interface TooltipProps {
  node: LayoutNode;
  containerRect: DOMRect | null;
  svgTransform: { x: number; y: number; scale: number };
}

function GraphTooltip({ node, containerRect, svgTransform }: TooltipProps) {
  if (!containerRect) return null;

  const screenX = (node.x + NODE_WIDTH / 2) * svgTransform.scale + svgTransform.x;
  const screenY = node.y * svgTransform.scale + svgTransform.y - 8;

  return (
    <div
      className="absolute z-50 pointer-events-none"
      style={{
        left: screenX,
        top: screenY,
        transform: "translate(-50%, -100%)",
      }}
    >
      <div className="glass rounded-lg px-3 py-2 text-xs max-w-[240px]">
        <div className="text-slate-100 font-mono font-medium mb-1 break-all">{node.name}</div>
        <div className="flex items-center gap-3 text-slate-400">
          <span>{node.kind}</span>
          <span className="font-medium" style={{ color: getStatusBorderColor(node.status) }}>
            {node.status || "Unknown"}
          </span>
        </div>
        {node.namespace && <div className="text-slate-500 mt-0.5">ns: {node.namespace}</div>}
      </div>
    </div>
  );
}

// ── Main Component ──────────────────────────────────────────────────────

interface ResourceGraphProps {
  namespace?: string;
  onSelectResource?: (kind: string, name: string, namespace: string) => void;
}

export function ResourceGraphView({ namespace, onSelectResource }: ResourceGraphProps) {
  const [graph, setGraph] = useState<ResourceGraph | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [hoveredNode, setHoveredNode] = useState<string | null>(null);

  // Filter state
  const [disabledKinds, setDisabledKinds] = useState<Set<string>>(new Set());
  const [searchQuery, setSearchQuery] = useState("");

  // Pan and zoom state
  const [transform, setTransform] = useState({ x: 0, y: 0, scale: 1 });
  const [isPanning, setIsPanning] = useState(false);
  const panStart = useRef({ x: 0, y: 0, tx: 0, ty: 0 });

  const containerRef = useRef<HTMLDivElement>(null);
  const [containerRect, setContainerRect] = useState<DOMRect | null>(null);

  // Fetch graph data
  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);

    buildResourceGraph(namespace)
      .then((data) => {
        if (!cancelled) {
          setGraph(data);
          setLoading(false);
        }
      })
      .catch((err) => {
        if (!cancelled) {
          setError(String(err));
          setLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [namespace]);

  // Compute layout
  const {
    layoutNodes,
    width: graphWidth,
    height: graphHeight,
  } = useMemo(() => {
    if (!graph) return { layoutNodes: [], width: 0, height: 0 };
    return computeLayout(graph.nodes, graph.edges);
  }, [graph]);

  // Build lookup for positioned nodes
  const nodeMap = useMemo(() => {
    const map = new Map<string, LayoutNode>();
    for (const node of layoutNodes) {
      map.set(node.id, node);
    }
    return map;
  }, [layoutNodes]);

  // Compute connected node ids for highlighting
  const connectedToHovered = useMemo(() => {
    if (!hoveredNode || !graph) return new Set<string>();
    const connected = new Set<string>();
    connected.add(hoveredNode);
    for (const edge of graph.edges) {
      if (edge.source === hoveredNode) connected.add(edge.target);
      if (edge.target === hoveredNode) connected.add(edge.source);
    }
    return connected;
  }, [hoveredNode, graph]);

  // Unique kinds present in the graph
  const uniqueKinds = useMemo(() => {
    if (!graph) return [];
    const kinds = new Set<string>();
    for (const node of graph.nodes) kinds.add(node.kind);
    // Sort by layer order for consistent display
    return Array.from(kinds).sort(
      (a, b) => (KIND_LAYER_ORDER[a] ?? 2) - (KIND_LAYER_ORDER[b] ?? 2),
    );
  }, [graph]);

  // Filter visibility: "direct" | "connected" | "hidden"
  const isFilterActive = disabledKinds.size > 0 || searchQuery.length > 0;

  const nodeVisibility = useMemo(() => {
    const vis = new Map<string, "direct" | "connected" | "hidden">();
    if (!graph || !isFilterActive) return vis;

    const search = searchQuery.toLowerCase();

    // First pass: find direct matches
    const directIds = new Set<string>();
    for (const node of graph.nodes) {
      const kindEnabled = !disabledKinds.has(node.kind);
      const nameMatch = search.length === 0 || node.name.toLowerCase().includes(search);
      if (kindEnabled && nameMatch) {
        directIds.add(node.id);
        vis.set(node.id, "direct");
      }
    }

    // Second pass: find connected matches (1-hop from direct)
    for (const edge of graph.edges) {
      if (directIds.has(edge.source) && !directIds.has(edge.target)) {
        if (!vis.has(edge.target)) vis.set(edge.target, "connected");
      }
      if (directIds.has(edge.target) && !directIds.has(edge.source)) {
        if (!vis.has(edge.source)) vis.set(edge.source, "connected");
      }
    }

    // Everything else is "hidden"
    for (const node of graph.nodes) {
      if (!vis.has(node.id)) vis.set(node.id, "hidden");
    }

    return vis;
  }, [graph, disabledKinds, searchQuery, isFilterActive]);

  const toggleKind = useCallback((kind: string) => {
    setDisabledKinds((prev) => {
      const next = new Set(prev);
      if (next.has(kind)) next.delete(kind);
      else next.add(kind);
      return next;
    });
  }, []);

  // Fit to view
  const fitToView = useCallback(() => {
    if (!containerRef.current || graphWidth === 0 || graphHeight === 0) return;
    const rect = containerRef.current.getBoundingClientRect();
    const scaleX = (rect.width - 40) / graphWidth;
    const scaleY = (rect.height - 40) / graphHeight;
    const scale = Math.min(scaleX, scaleY, 1.5);
    const x = (rect.width - graphWidth * scale) / 2;
    const y = (rect.height - graphHeight * scale) / 2;
    setTransform({ x, y, scale });
  }, [graphWidth, graphHeight]);

  // Fit to view on initial load and when graph changes
  useEffect(() => {
    if (layoutNodes.length > 0) {
      // Small delay to ensure container is measured
      const timer = setTimeout(fitToView, 50);
      return () => clearTimeout(timer);
    }
  }, [layoutNodes.length, fitToView]);

  // Update container rect on resize
  useEffect(() => {
    const updateRect = () => {
      if (containerRef.current) {
        setContainerRect(containerRef.current.getBoundingClientRect());
      }
    };
    updateRect();
    const observer = new ResizeObserver(updateRect);
    if (containerRef.current) observer.observe(containerRef.current);
    return () => observer.disconnect();
  }, []);

  // Mouse wheel zoom
  const handleWheel = useCallback(
    (e: React.WheelEvent) => {
      e.preventDefault();
      const rect = containerRef.current?.getBoundingClientRect();
      if (!rect) return;

      const mouseX = e.clientX - rect.left;
      const mouseY = e.clientY - rect.top;

      const zoomFactor = e.deltaY > 0 ? 0.92 : 1.08;
      const newScale = Math.min(Math.max(transform.scale * zoomFactor, 0.15), 4);

      // Zoom toward mouse position
      const scaleChange = newScale / transform.scale;
      const newX = mouseX - (mouseX - transform.x) * scaleChange;
      const newY = mouseY - (mouseY - transform.y) * scaleChange;

      setTransform({ x: newX, y: newY, scale: newScale });
    },
    [transform],
  );

  // Pan handlers
  const handleMouseDown = useCallback(
    (e: React.MouseEvent) => {
      // Only pan on left click on the SVG background (not on nodes)
      if (e.button !== 0) return;
      setIsPanning(true);
      panStart.current = {
        x: e.clientX,
        y: e.clientY,
        tx: transform.x,
        ty: transform.y,
      };
    },
    [transform],
  );

  const handleMouseMove = useCallback(
    (e: React.MouseEvent) => {
      if (!isPanning) return;
      const dx = e.clientX - panStart.current.x;
      const dy = e.clientY - panStart.current.y;
      setTransform((prev) => ({
        ...prev,
        x: panStart.current.tx + dx,
        y: panStart.current.ty + dy,
      }));
    },
    [isPanning],
  );

  const handleMouseUp = useCallback(() => {
    setIsPanning(false);
  }, []);

  // Handle node click
  const handleNodeClick = useCallback(
    (node: LayoutNode) => {
      onSelectResource?.(node.kind, node.name, node.namespace);
    },
    [onSelectResource],
  );

  // Find the hovered node for tooltip
  const hoveredLayoutNode = hoveredNode ? nodeMap.get(hoveredNode) : null;

  // ── Render ────────────────────────────────────────────────────────────

  if (loading) {
    return (
      <div className="h-full w-full flex flex-col items-center justify-center gap-3">
        <Loader2 className="w-8 h-8 text-accent animate-spin" />
        <p className="text-sm text-slate-400">Building resource graph...</p>
      </div>
    );
  }

  if (error) {
    return (
      <div className="h-full w-full flex flex-col items-center justify-center gap-3 text-red-400">
        <p className="text-sm font-medium">Failed to load resource graph</p>
        <p className="text-xs text-slate-500 max-w-md text-center">{error}</p>
      </div>
    );
  }

  if (!graph || graph.nodes.length === 0) {
    return (
      <div className="h-full w-full flex flex-col items-center justify-center text-slate-400 gap-3">
        <Inbox className="w-12 h-12 text-slate-600" />
        <p className="text-sm font-medium">No resources found</p>
        <p className="text-xs text-slate-500">
          {namespace
            ? `No resources in namespace "${namespace}"`
            : "Select a namespace to view the resource graph"}
        </p>
      </div>
    );
  }

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      className="h-full w-full relative overflow-hidden"
      ref={containerRef}
    >
      {/* Toolbar */}
      <div className="absolute top-3 right-3 z-20 flex items-center gap-1.5">
        <button
          onClick={fitToView}
          className="flex items-center gap-1.5 px-2.5 py-1.5 text-xs text-slate-300 bg-surface/80 border border-slate-800 rounded-lg hover:border-accent/50 hover:bg-muted/30 transition backdrop-blur-sm"
          title="Fit to view"
        >
          <Maximize2 className="w-3.5 h-3.5" />
          Fit
        </button>
        <span className="text-[10px] text-slate-600 font-mono px-1.5">
          {Math.round(transform.scale * 100)}%
        </span>
      </div>

      {/* Legend */}
      <div className="absolute bottom-3 left-3 z-20 flex items-center gap-3 px-3 py-2 bg-surface/80 border border-slate-800 rounded-lg backdrop-blur-sm">
        {Object.entries(KIND_COLORS).map(([kind, color]) => (
          <div key={kind} className="flex items-center gap-1.5">
            <div className="w-2.5 h-2.5 rounded-sm" style={{ backgroundColor: color.stroke }} />
            <span className="text-[10px] text-slate-400">{kind}</span>
          </div>
        ))}
      </div>

      {/* Node count badge */}
      <div className="absolute top-3 left-3 z-20 text-[10px] text-slate-500 font-mono px-2 py-1 bg-surface/80 border border-slate-800 rounded-lg backdrop-blur-sm">
        {graph.nodes.length} nodes &middot; {graph.edges.length} edges
      </div>

      {/* Filter bar */}
      <div className="absolute top-10 left-3 z-20 flex items-center gap-2 mt-1.5">
        {/* Kind toggle chips */}
        <div className="flex items-center gap-1 px-2 py-1.5 bg-surface/80 border border-slate-800 rounded-lg backdrop-blur-sm">
          {uniqueKinds.map((kind) => {
            const color = getKindColor(kind);
            const enabled = !disabledKinds.has(kind);
            return (
              <button
                key={kind}
                onClick={() => toggleKind(kind)}
                className="flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] font-mono transition-all"
                style={{
                  backgroundColor: enabled ? color.fill : "transparent",
                  borderWidth: 1,
                  borderStyle: "solid",
                  borderColor: enabled ? color.stroke : "#334155",
                  color: enabled ? color.text : "#475569",
                  opacity: enabled ? 1 : 0.5,
                }}
              >
                <div
                  className="w-1.5 h-1.5 rounded-full"
                  style={{ backgroundColor: enabled ? color.stroke : "#475569" }}
                />
                {kind}
              </button>
            );
          })}
        </div>

        {/* Name search */}
        <div className="relative flex items-center">
          <Search className="absolute left-2 w-3 h-3 text-slate-500 pointer-events-none" />
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="Filter by name..."
            className="pl-6 pr-6 py-1 w-40 text-[11px] font-mono text-slate-300 bg-surface/80 border border-slate-800 rounded-lg backdrop-blur-sm placeholder:text-slate-600 focus:outline-none focus:border-accent/50 transition"
          />
          {searchQuery && (
            <button
              onClick={() => setSearchQuery("")}
              className="absolute right-1.5 p-0.5 text-slate-500 hover:text-slate-300 transition"
            >
              <X className="w-3 h-3" />
            </button>
          )}
        </div>
      </div>

      {/* SVG Canvas */}
      <svg
        className="w-full h-full"
        style={{
          cursor: isPanning ? "grabbing" : "grab",
          background: "radial-gradient(circle at 50% 50%, #0f1a2e 0%, #0b1221 100%)",
        }}
        onWheel={handleWheel}
        onMouseDown={handleMouseDown}
        onMouseMove={handleMouseMove}
        onMouseUp={handleMouseUp}
        onMouseLeave={handleMouseUp}
      >
        {/* Grid pattern */}
        <defs>
          <pattern
            id="graph-grid"
            width={40}
            height={40}
            patternUnits="userSpaceOnUse"
            patternTransform={`translate(${transform.x} ${transform.y}) scale(${transform.scale})`}
          >
            <circle cx={20} cy={20} r={0.5} fill="#1e293b" />
          </pattern>

          {/* Arrow marker for edges */}
          <marker id="arrowhead" markerWidth={8} markerHeight={6} refX={7} refY={3} orient="auto">
            <path d="M 0 0 L 8 3 L 0 6 z" fill="#334155" opacity={0.6} />
          </marker>
          <marker
            id="arrowhead-highlight"
            markerWidth={8}
            markerHeight={6}
            refX={7}
            refY={3}
            orient="auto"
          >
            <path d="M 0 0 L 8 3 L 0 6 z" fill="var(--accent)" opacity={0.8} />
          </marker>
        </defs>

        {/* Grid background */}
        <rect width="100%" height="100%" fill="url(#graph-grid)" />

        {/* Transform group for pan/zoom */}
        <g transform={`translate(${transform.x}, ${transform.y}) scale(${transform.scale})`}>
          {/* Edges */}
          {graph.edges.map((edge) => {
            const source = nodeMap.get(edge.source);
            const target = nodeMap.get(edge.target);
            if (!source || !target) return null;

            const isHighlighted =
              hoveredNode !== null &&
              connectedToHovered.has(edge.source) &&
              connectedToHovered.has(edge.target);

            const hoverDimmed = hoveredNode !== null && !isHighlighted;

            // Filter: edge is visible if both endpoints are visible (direct or connected)
            const sourceVis = nodeVisibility.get(edge.source);
            const targetVis = nodeVisibility.get(edge.target);
            const filterDimmed =
              isFilterActive && (sourceVis === "hidden" || targetVis === "hidden");

            let edgeOpacity = isHighlighted ? 0.9 : 0.5;
            if (filterDimmed) edgeOpacity = 0.05;
            else if (hoverDimmed) edgeOpacity = 0.15;

            return (
              <path
                key={`${edge.source}-${edge.target}`}
                d={computeEdgePath(source, target)}
                fill="none"
                stroke={isHighlighted ? "var(--accent)" : "#334155"}
                strokeWidth={isHighlighted ? 2 : 1}
                strokeDasharray={edge.relation === "owns" ? undefined : "4 3"}
                opacity={edgeOpacity}
                markerEnd={isHighlighted ? "url(#arrowhead-highlight)" : "url(#arrowhead)"}
                style={{
                  transition: "opacity 0.2s, stroke 0.2s, stroke-width 0.2s",
                }}
              />
            );
          })}

          {/* Nodes */}
          {layoutNodes.map((node) => {
            const hoverDimmed = hoveredNode !== null && !connectedToHovered.has(node.id);
            const filterVis = nodeVisibility.get(node.id);
            const filterDimmed = isFilterActive && filterVis === "hidden";
            const filterConnected = isFilterActive && filterVis === "connected";

            let opacity = 1;
            if (filterDimmed) opacity = 0.08;
            else if (filterConnected) opacity = 0.6;
            if (hoverDimmed && !filterDimmed) opacity = Math.min(opacity, 0.3);

            return (
              <g key={node.id} opacity={opacity} style={{ transition: "opacity 0.2s" }}>
                <SVGGraphNode
                  node={node}
                  onSelect={() => handleNodeClick(node)}
                  isHovered={hoveredNode === node.id}
                  onMouseEnter={() => setHoveredNode(node.id)}
                  onMouseLeave={() => setHoveredNode(null)}
                />
              </g>
            );
          })}
        </g>
      </svg>

      {/* Tooltip overlay (rendered outside SVG for proper HTML rendering) */}
      <AnimatePresence>
        {hoveredLayoutNode && (
          <motion.div
            initial={{ opacity: 0, y: 4 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 4 }}
            transition={{ duration: 0.15 }}
          >
            <GraphTooltip
              node={hoveredLayoutNode}
              containerRect={containerRect}
              svgTransform={transform}
            />
          </motion.div>
        )}
      </AnimatePresence>
    </motion.div>
  );
}
