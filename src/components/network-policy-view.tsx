import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import {
  Loader2,
  Inbox,
  Shield,
  ShieldAlert,
  ShieldCheck,
  ShieldX,
  Maximize2,
  RefreshCw,
  Search,
  X,
  ChevronDown,
  ChevronUp,
  Globe,
  Box,
  Container,
  Server,
  Layers,
} from "lucide-react";
import { useNetworkPolicyGraph } from "@/hooks/use-network-policy-graph";
import type {
  NetworkPolicyGraph,
  NetworkPolicyPodGroup,
  NetworkPolicyCidrNode,
  NetworkPolicyTrafficEdge,
  NetworkPolicySummary,
  TrafficSimulationResult,
} from "@/lib/api";

// ── Constants ────────────────────────────────────────────────────────

const NODE_RADIUS = 28;
const CIDR_RADIUS = 22;
const REPULSION = 5000;
const ATTRACTION = 0.005;
const REST_LENGTH = 200;
const NS_GRAVITY = 0.1;
const CENTER_GRAVITY = 0.01;
const DAMPING = 0.85;
const ALPHA_DECAY = 0.99;
const NS_PADDING = 50;

// ── Kind colors ──────────────────────────────────────────────────────

const GROUP_COLORS: Record<string, { fill: string; stroke: string; text: string }> = {
  Deployment: { fill: "#1e3a5f", stroke: "#3b82f6", text: "#93c5fd" },
  ReplicaSet: { fill: "#1e293b", stroke: "#64748b", text: "#94a3b8" },
  DaemonSet: { fill: "#2d1b4e", stroke: "#a855f7", text: "#d8b4fe" },
  StatefulSet: { fill: "#1a3a2a", stroke: "#10b981", text: "#6ee7b7" },
  Job: { fill: "#3b2a0a", stroke: "#f97316", text: "#fdba74" },
  Pod: { fill: "#1a3a2a", stroke: "#10b981", text: "#6ee7b7" },
};
const DEFAULT_COLOR = { fill: "#1e293b", stroke: "#64748b", text: "#94a3b8" };
const CIDR_COLOR = { fill: "#2a1a1a", stroke: "#ef4444", text: "#fca5a5" };

function getGroupColor(kind: string) {
  return GROUP_COLORS[kind] ?? DEFAULT_COLOR;
}

// ── Simulation node type ─────────────────────────────────────────────

interface SimNode {
  id: string;
  type: "group" | "cidr";
  x: number;
  y: number;
  vx: number;
  vy: number;
  fx: number | null;
  fy: number | null;
  namespace: string;
  label: string;
  kind: string;
  podCount: number;
  isIsolatedIngress: boolean;
  isIsolatedEgress: boolean;
  matchingPolicies: string[];
}

// ── Force simulation ─────────────────────────────────────────────────

function initNodes(
  groups: NetworkPolicyPodGroup[],
  cidrs: NetworkPolicyCidrNode[],
  width: number,
  height: number,
): SimNode[] {
  // Spread groups by namespace in circles
  const nsByIdx = new Map<string, number>();
  let nsCounter = 0;
  for (const g of groups) {
    if (!nsByIdx.has(g.namespace)) {
      nsByIdx.set(g.namespace, nsCounter++);
    }
  }
  const nsCount = nsByIdx.size || 1;

  const nodes: SimNode[] = groups.map((g, i) => {
    const nsIdx = nsByIdx.get(g.namespace) ?? 0;
    const angle = ((nsIdx / nsCount) * 2 * Math.PI) + (i * 0.3);
    const r = 150 + Math.random() * 100;
    return {
      id: g.id,
      type: "group",
      x: width / 2 + Math.cos(angle) * r,
      y: height / 2 + Math.sin(angle) * r,
      vx: 0,
      vy: 0,
      fx: null,
      fy: null,
      namespace: g.namespace,
      label: g.name,
      kind: g.kind,
      podCount: g.pod_count,
      isIsolatedIngress: g.is_isolated_ingress,
      isIsolatedEgress: g.is_isolated_egress,
      matchingPolicies: g.matching_policies,
    };
  });

  // Add CIDR nodes at edges
  for (const c of cidrs) {
    nodes.push({
      id: c.id,
      type: "cidr",
      x: width / 2 + (Math.random() - 0.5) * 400,
      y: height / 2 + (Math.random() - 0.5) * 400,
      vx: 0,
      vy: 0,
      fx: null,
      fy: null,
      namespace: "__external__",
      label: c.cidr,
      kind: "CIDR",
      podCount: 0,
      isIsolatedIngress: false,
      isIsolatedEgress: false,
      matchingPolicies: [c.from_policy],
    });
  }

  return nodes;
}

function tickSimulation(
  nodes: SimNode[],
  edges: NetworkPolicyTrafficEdge[],
  width: number,
  height: number,
  alpha: number,
): number {
  const nodeMap = new Map<string, number>();
  for (let i = 0; i < nodes.length; i++) nodeMap.set(nodes[i].id, i);

  // Namespace centroids
  const nsCentroids = new Map<string, { x: number; y: number; count: number }>();
  for (const n of nodes) {
    if (n.namespace === "__external__") continue;
    const c = nsCentroids.get(n.namespace) ?? { x: 0, y: 0, count: 0 };
    c.x += n.x;
    c.y += n.y;
    c.count++;
    nsCentroids.set(n.namespace, c);
  }

  // Repulsion
  for (let i = 0; i < nodes.length; i++) {
    for (let j = i + 1; j < nodes.length; j++) {
      const a = nodes[i];
      const b = nodes[j];
      let dx = b.x - a.x;
      let dy = b.y - a.y;
      const dist = Math.sqrt(dx * dx + dy * dy) || 1;
      const force = (REPULSION * alpha) / (dist * dist);
      dx = (dx / dist) * force;
      dy = (dy / dist) * force;
      if (a.fx === null) { a.vx -= dx; a.vy -= dy; }
      if (b.fx === null) { b.vx += dx; b.vy += dy; }
    }
  }

  // Attraction along edges
  for (const e of edges) {
    const si = nodeMap.get(e.source);
    const ti = nodeMap.get(e.target);
    if (si === undefined || ti === undefined) continue;
    const a = nodes[si];
    const b = nodes[ti];
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    const dist = Math.sqrt(dx * dx + dy * dy) || 1;
    const force = (dist - REST_LENGTH) * ATTRACTION * alpha;
    dx = (dx / dist) * force;
    dy = (dy / dist) * force;
    if (a.fx === null) { a.vx += dx; a.vy += dy; }
    if (b.fx === null) { b.vx -= dx; b.vy -= dy; }
  }

  // Namespace gravity
  for (const n of nodes) {
    if (n.fx !== null) continue;
    const c = nsCentroids.get(n.namespace);
    if (c && c.count > 1) {
      const cx = c.x / c.count;
      const cy = c.y / c.count;
      n.vx += (cx - n.x) * NS_GRAVITY * alpha;
      n.vy += (cy - n.y) * NS_GRAVITY * alpha;
    }
    // Center gravity
    n.vx += (width / 2 - n.x) * CENTER_GRAVITY * alpha;
    n.vy += (height / 2 - n.y) * CENTER_GRAVITY * alpha;
  }

  // Apply velocities
  for (const n of nodes) {
    if (n.fx !== null) { n.x = n.fx; n.y = n.fy!; n.vx = 0; n.vy = 0; continue; }
    n.vx *= DAMPING;
    n.vy *= DAMPING;
    n.x += n.vx;
    n.y += n.vy;
  }

  return alpha * ALPHA_DECAY;
}

// ── Namespace bounding boxes ─────────────────────────────────────────

interface NsBounds {
  ns: string;
  x: number;
  y: number;
  w: number;
  h: number;
}

function computeNsBounds(nodes: SimNode[]): NsBounds[] {
  const nsNodes = new Map<string, SimNode[]>();
  for (const n of nodes) {
    if (n.namespace === "__external__") continue;
    const arr = nsNodes.get(n.namespace) ?? [];
    arr.push(n);
    nsNodes.set(n.namespace, arr);
  }
  const bounds: NsBounds[] = [];
  for (const [ns, arr] of nsNodes) {
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const n of arr) {
      minX = Math.min(minX, n.x - NODE_RADIUS);
      minY = Math.min(minY, n.y - NODE_RADIUS);
      maxX = Math.max(maxX, n.x + NODE_RADIUS);
      maxY = Math.max(maxY, n.y + NODE_RADIUS);
    }
    bounds.push({
      ns,
      x: minX - NS_PADDING,
      y: minY - NS_PADDING,
      w: maxX - minX + NS_PADDING * 2,
      h: maxY - minY + NS_PADDING * 2,
    });
  }
  return bounds;
}

// ── Edge path ────────────────────────────────────────────────────────

function edgePath(
  sx: number, sy: number, tx: number, ty: number,
): string {
  const mx = (sx + tx) / 2;
  const my = (sy + ty) / 2;
  const dx = tx - sx;
  const dy = ty - sy;
  const perpX = -dy * 0.15;
  const perpY = dx * 0.15;
  return `M ${sx} ${sy} Q ${mx + perpX} ${my + perpY}, ${tx} ${ty}`;
}

// ── Kind icon ────────────────────────────────────────────────────────

function KindIcon({ kind, color }: { kind: string; color: string }) {
  const props = { width: 14, height: 14, color, strokeWidth: 1.5 };
  switch (kind) {
    case "Deployment": return <Container {...props} />;
    case "Pod": return <Box {...props} />;
    case "CIDR": return <Globe {...props} />;
    case "ReplicaSet": return <Layers {...props} />;
    default: return <Server {...props} />;
  }
}

// ── Main Component ───────────────────────────────────────────────────

interface NetworkPolicyViewProps {
  namespace?: string;
  currentContext?: string;
}

export function NetworkPolicyView({ namespace, currentContext }: NetworkPolicyViewProps) {
  const { graph, loading, error, refresh, simulate } = useNetworkPolicyGraph(namespace, currentContext);

  // Simulation nodes
  const [simNodes, setSimNodes] = useState<SimNode[]>([]);
  const simNodesRef = useRef<SimNode[]>([]);
  const alphaRef = useRef(1);
  const animRef = useRef<number>(0);
  const containerRef = useRef<HTMLDivElement>(null);
  const [containerSize, setContainerSize] = useState({ width: 1200, height: 800 });

  // Pan/zoom
  const [transform, setTransform] = useState({ x: 0, y: 0, scale: 1 });
  const [isPanning, setIsPanning] = useState(false);
  const panStart = useRef({ x: 0, y: 0, tx: 0, ty: 0 });
  const dragNode = useRef<string | null>(null);

  // Selection/filter
  const [hoveredNode, setHoveredNode] = useState<string | null>(null);
  const [selectedPolicy, setSelectedPolicy] = useState<string | null>(null);
  const [policySearch, setPolicySearch] = useState("");

  // Simulation panel
  const [simOpen, setSimOpen] = useState(false);
  const [simSourceNs, setSimSourceNs] = useState("");
  const [simSourcePod, setSimSourcePod] = useState("");
  const [simDestNs, setSimDestNs] = useState("");
  const [simDestPod, setSimDestPod] = useState("");
  const [simPort, setSimPort] = useState("");
  const [simProtocol, setSimProtocol] = useState("TCP");
  const [simResult, setSimResult] = useState<TrafficSimulationResult | null>(null);
  const [simLoading, setSimLoading] = useState(false);

  // Policy panel
  const [policyPanelOpen, setPolicyPanelOpen] = useState(true);

  // Measure container
  useEffect(() => {
    const update = () => {
      if (containerRef.current) {
        const r = containerRef.current.getBoundingClientRect();
        setContainerSize({ width: r.width, height: r.height });
      }
    };
    update();
    const obs = new ResizeObserver(update);
    if (containerRef.current) obs.observe(containerRef.current);
    return () => obs.disconnect();
  }, []);

  // Initialize simulation when graph changes
  useEffect(() => {
    if (!graph) return;
    const nodes = initNodes(graph.groups, graph.external_cidrs, containerSize.width, containerSize.height);
    simNodesRef.current = nodes;
    setSimNodes([...nodes]);
    alphaRef.current = 1;
  }, [graph, containerSize.width, containerSize.height]);

  // Run simulation loop
  useEffect(() => {
    if (!graph || simNodesRef.current.length === 0) return;

    const tick = () => {
      if (alphaRef.current < 0.001) {
        animRef.current = requestAnimationFrame(tick); // Keep checking for reheat
        return;
      }
      alphaRef.current = tickSimulation(
        simNodesRef.current,
        graph.edges,
        containerSize.width,
        containerSize.height,
        alphaRef.current,
      );
      setSimNodes([...simNodesRef.current]);
      animRef.current = requestAnimationFrame(tick);
    };

    animRef.current = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(animRef.current);
  }, [graph, containerSize.width, containerSize.height]);

  // Namespace bounds
  const nsBounds = useMemo(() => computeNsBounds(simNodes), [simNodes]);

  // Node map for edges
  const nodeMap = useMemo(() => {
    const m = new Map<string, SimNode>();
    for (const n of simNodes) m.set(n.id, n);
    return m;
  }, [simNodes]);

  // Connected nodes for hover
  const connectedToHovered = useMemo(() => {
    if (!hoveredNode || !graph) return new Set<string>();
    const connected = new Set<string>();
    connected.add(hoveredNode);
    for (const e of graph.edges) {
      if (e.source === hoveredNode) connected.add(e.target);
      if (e.target === hoveredNode) connected.add(e.source);
    }
    return connected;
  }, [hoveredNode, graph]);

  // Affected groups for selected policy
  const policyAffectedGroups = useMemo(() => {
    if (!selectedPolicy || !graph) return new Set<string>();
    const affected = new Set<string>();
    for (const g of graph.groups) {
      if (g.matching_policies.includes(selectedPolicy)) {
        affected.add(g.id);
      }
    }
    for (const e of graph.edges) {
      if (e.policy_name === selectedPolicy) {
        affected.add(e.source);
        affected.add(e.target);
      }
    }
    return affected;
  }, [selectedPolicy, graph]);

  // Filtered policies
  const filteredPolicies = useMemo(() => {
    if (!graph) return [];
    if (!policySearch) return graph.policies;
    const q = policySearch.toLowerCase();
    return graph.policies.filter(
      (p) => p.name.toLowerCase().includes(q) || p.namespace.toLowerCase().includes(q),
    );
  }, [graph, policySearch]);

  // Pod list for simulation dropdowns
  const allPods = useMemo(() => {
    if (!graph) return [];
    const pods: { name: string; namespace: string }[] = [];
    for (const g of graph.groups) {
      for (const p of g.pods) {
        pods.push({ name: p.name, namespace: p.namespace });
      }
    }
    return pods;
  }, [graph]);

  const podNamespaces = useMemo(() => {
    const ns = new Set<string>();
    for (const p of allPods) ns.add(p.namespace);
    return Array.from(ns).sort();
  }, [allPods]);

  // Fit to view
  const fitToView = useCallback(() => {
    if (simNodes.length === 0 || !containerRef.current) return;
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const n of simNodes) {
      minX = Math.min(minX, n.x - 50);
      minY = Math.min(minY, n.y - 50);
      maxX = Math.max(maxX, n.x + 50);
      maxY = Math.max(maxY, n.y + 50);
    }
    const gw = maxX - minX || 1;
    const gh = maxY - minY || 1;
    const rect = containerRef.current.getBoundingClientRect();
    const scaleX = (rect.width - 80) / gw;
    const scaleY = (rect.height - 80) / gh;
    const scale = Math.min(scaleX, scaleY, 1.5);
    const x = (rect.width - gw * scale) / 2 - minX * scale;
    const y = (rect.height - gh * scale) / 2 - minY * scale;
    setTransform({ x, y, scale });
  }, [simNodes]);

  // Fit on initial load
  useEffect(() => {
    if (simNodes.length > 0 && alphaRef.current < 0.5) {
      const t = setTimeout(fitToView, 100);
      return () => clearTimeout(t);
    }
  }, [simNodes.length > 0, fitToView]);

  // Mouse handlers
  const handleWheel = useCallback((e: React.WheelEvent) => {
    e.preventDefault();
    const rect = containerRef.current?.getBoundingClientRect();
    if (!rect) return;
    const mx = e.clientX - rect.left;
    const my = e.clientY - rect.top;
    const factor = e.deltaY > 0 ? 0.92 : 1.08;
    setTransform((t) => {
      const newScale = Math.min(Math.max(t.scale * factor, 0.1), 5);
      const sc = newScale / t.scale;
      return { x: mx - (mx - t.x) * sc, y: my - (my - t.y) * sc, scale: newScale };
    });
  }, []);

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    if (e.button !== 0 || dragNode.current) return;
    setIsPanning(true);
    panStart.current = { x: e.clientX, y: e.clientY, tx: transform.x, ty: transform.y };
  }, [transform]);

  const handleMouseMove = useCallback((e: React.MouseEvent) => {
    if (dragNode.current) {
      const rect = containerRef.current?.getBoundingClientRect();
      if (!rect) return;
      const svgX = (e.clientX - rect.left - transform.x) / transform.scale;
      const svgY = (e.clientY - rect.top - transform.y) / transform.scale;
      const node = simNodesRef.current.find((n) => n.id === dragNode.current);
      if (node) { node.fx = svgX; node.fy = svgY; node.x = svgX; node.y = svgY; }
      alphaRef.current = Math.max(alphaRef.current, 0.3);
      return;
    }
    if (!isPanning) return;
    const dx = e.clientX - panStart.current.x;
    const dy = e.clientY - panStart.current.y;
    setTransform((t) => ({ ...t, x: panStart.current.tx + dx, y: panStart.current.ty + dy }));
  }, [isPanning, transform]);

  const handleMouseUp = useCallback(() => {
    if (dragNode.current) {
      const node = simNodesRef.current.find((n) => n.id === dragNode.current);
      if (node) { node.fx = null; node.fy = null; }
      dragNode.current = null;
    }
    setIsPanning(false);
  }, []);

  const handleNodeMouseDown = useCallback((e: React.MouseEvent, nodeId: string) => {
    e.stopPropagation();
    dragNode.current = nodeId;
    const rect = containerRef.current?.getBoundingClientRect();
    if (!rect) return;
    const svgX = (e.clientX - rect.left - transform.x) / transform.scale;
    const svgY = (e.clientY - rect.top - transform.y) / transform.scale;
    const node = simNodesRef.current.find((n) => n.id === nodeId);
    if (node) { node.fx = svgX; node.fy = svgY; }
    alphaRef.current = Math.max(alphaRef.current, 0.5);
  }, [transform]);

  // Simulation handler
  const runSimulation = useCallback(async () => {
    if (!simSourceNs || !simSourcePod || !simDestNs || !simDestPod) return;
    setSimLoading(true);
    try {
      const result = await simulate(
        simSourceNs, simSourcePod, simDestNs, simDestPod,
        simPort ? parseInt(simPort) : undefined,
        simProtocol || undefined,
      );
      setSimResult(result);
    } catch (err) {
      setSimResult({
        allowed: false,
        summary: `Error: ${err}`,
        ingress_evaluation: { isolated: false, policy_results: [] },
        egress_evaluation: { isolated: false, policy_results: [] },
      });
    } finally {
      setSimLoading(false);
    }
  }, [simulate, simSourceNs, simSourcePod, simDestNs, simDestPod, simPort, simProtocol]);

  // ── Render states ──────────────────────────────────────────────────

  if (loading && !graph) {
    return (
      <div className="h-full w-full flex flex-col items-center justify-center gap-3">
        <Loader2 className="w-8 h-8 text-accent animate-spin" />
        <p className="text-sm text-slate-400">Loading network policies...</p>
      </div>
    );
  }

  if (error) {
    return (
      <div className="h-full w-full flex flex-col items-center justify-center gap-3 text-red-400">
        <ShieldX className="w-10 h-10" />
        <p className="text-sm font-medium">Failed to load network policies</p>
        <p className="text-xs text-slate-500 max-w-md text-center">{error}</p>
      </div>
    );
  }

  if (!graph || (graph.groups.length === 0 && graph.policies.length === 0)) {
    return (
      <div className="h-full w-full flex flex-col items-center justify-center text-slate-400 gap-3">
        <Inbox className="w-12 h-12 text-slate-600" />
        <p className="text-sm font-medium">No network policies found</p>
        <p className="text-xs text-slate-500">
          {namespace ? `No NetworkPolicies in "${namespace}"` : "Select a namespace to view network policies"}
        </p>
      </div>
    );
  }

  const isHighlighting = hoveredNode !== null || selectedPolicy !== null;

  function getNodeOpacity(nodeId: string): number {
    if (!isHighlighting) return 1;
    if (selectedPolicy) {
      return policyAffectedGroups.has(nodeId) ? 1 : 0.12;
    }
    if (hoveredNode) {
      return connectedToHovered.has(nodeId) ? 1 : 0.2;
    }
    return 1;
  }

  function getEdgeOpacity(edge: NetworkPolicyTrafficEdge): number {
    if (!isHighlighting) return 0.6;
    if (selectedPolicy) {
      return edge.policy_name === selectedPolicy ? 0.9 : 0.05;
    }
    if (hoveredNode) {
      const connected = connectedToHovered.has(edge.source) && connectedToHovered.has(edge.target);
      return connected ? 0.9 : 0.08;
    }
    return 0.6;
  }

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      className="h-full w-full relative overflow-hidden flex"
      ref={containerRef}
    >
      {/* SVG Canvas */}
      <div className="flex-1 relative overflow-hidden">
        {/* Toolbar */}
        <div className="absolute top-3 left-3 z-20 flex items-center gap-2">
          <div className="text-[10px] text-slate-500 font-mono px-2 py-1 bg-surface/80 border border-slate-800 rounded-lg backdrop-blur-sm">
            <Shield className="w-3 h-3 inline mr-1" />
            {graph.policies.length} policies &middot; {graph.groups.length} groups &middot; {graph.edges.length} edges
          </div>
        </div>

        <div className="absolute top-3 right-3 z-20 flex items-center gap-1.5" style={{ right: policyPanelOpen ? "316px" : "12px" }}>
          <button
            onClick={refresh}
            className="flex items-center gap-1.5 px-2.5 py-1.5 text-xs text-slate-300 bg-surface/80 border border-slate-800 rounded-lg hover:border-accent/50 transition backdrop-blur-sm"
            title="Refresh"
          >
            <RefreshCw className="w-3.5 h-3.5" />
          </button>
          <button
            onClick={fitToView}
            className="flex items-center gap-1.5 px-2.5 py-1.5 text-xs text-slate-300 bg-surface/80 border border-slate-800 rounded-lg hover:border-accent/50 transition backdrop-blur-sm"
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
          {[...Object.entries(GROUP_COLORS).slice(0, 4), ["CIDR", CIDR_COLOR] as const].map(([kind, color]) => (
            <div key={kind} className="flex items-center gap-1.5">
              <div className="w-2.5 h-2.5 rounded-full" style={{ backgroundColor: (color as typeof DEFAULT_COLOR).stroke }} />
              <span className="text-[10px] text-slate-400">{kind}</span>
            </div>
          ))}
          <div className="flex items-center gap-1.5 ml-2 pl-2 border-l border-slate-700">
            <div className="w-3 h-0 border-t border-emerald-500 border-dashed" />
            <span className="text-[10px] text-slate-400">Ingress</span>
          </div>
          <div className="flex items-center gap-1.5">
            <div className="w-3 h-0 border-t border-blue-500 border-dashed" />
            <span className="text-[10px] text-slate-400">Egress</span>
          </div>
        </div>

        {/* Simulation toggle */}
        <div className="absolute bottom-3 right-3 z-20" style={{ right: policyPanelOpen ? "316px" : "12px" }}>
          <button
            onClick={() => setSimOpen(!simOpen)}
            className={`flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-lg border transition backdrop-blur-sm ${
              simOpen
                ? "bg-accent/10 border-accent/50 text-accent"
                : "bg-surface/80 border-slate-800 text-slate-300 hover:border-accent/50"
            }`}
          >
            <ShieldCheck className="w-3.5 h-3.5" />
            Can they talk?
          </button>
        </div>

        {/* SVG */}
        <svg
          className="w-full h-full"
          style={{
            cursor: isPanning ? "grabbing" : dragNode.current ? "grabbing" : "grab",
            background: "radial-gradient(circle at 50% 50%, #0f1a2e 0%, #0b1221 100%)",
          }}
          onWheel={handleWheel}
          onMouseDown={handleMouseDown}
          onMouseMove={handleMouseMove}
          onMouseUp={handleMouseUp}
          onMouseLeave={handleMouseUp}
        >
          <defs>
            <pattern
              id="np-grid"
              width={40}
              height={40}
              patternUnits="userSpaceOnUse"
              patternTransform={`translate(${transform.x} ${transform.y}) scale(${transform.scale})`}
            >
              <circle cx={20} cy={20} r={0.5} fill="#1e293b" />
            </pattern>
            <marker id="np-arrow-ingress" markerWidth={8} markerHeight={6} refX={7} refY={3} orient="auto">
              <path d="M 0 0 L 8 3 L 0 6 z" fill="#10b981" opacity={0.7} />
            </marker>
            <marker id="np-arrow-egress" markerWidth={8} markerHeight={6} refX={7} refY={3} orient="auto">
              <path d="M 0 0 L 8 3 L 0 6 z" fill="#3b82f6" opacity={0.7} />
            </marker>
          </defs>

          <rect width="100%" height="100%" fill="url(#np-grid)" />

          <g transform={`translate(${transform.x}, ${transform.y}) scale(${transform.scale})`}>
            {/* Namespace boundaries */}
            {nsBounds.map((b) => (
              <g key={b.ns}>
                <rect
                  x={b.x}
                  y={b.y}
                  width={b.w}
                  height={b.h}
                  rx={16}
                  ry={16}
                  fill="#1a2235"
                  fillOpacity={0.3}
                  stroke="#334155"
                  strokeWidth={1}
                  strokeDasharray="6 3"
                />
                <text
                  x={b.x + 12}
                  y={b.y + 18}
                  fill="#475569"
                  fontSize={10}
                  fontFamily="SFMono-Regular, Menlo, Monaco, Consolas, monospace"
                >
                  ns: {b.ns}
                </text>
              </g>
            ))}

            {/* Edges */}
            {graph.edges.map((e, i) => {
              const s = nodeMap.get(e.source);
              const t = nodeMap.get(e.target);
              if (!s || !t) return null;
              const opacity = getEdgeOpacity(e);
              const color = e.direction === "ingress" ? "#10b981" : "#3b82f6";
              const marker = e.direction === "ingress" ? "url(#np-arrow-ingress)" : "url(#np-arrow-egress)";
              const portLabel = e.ports.length > 0
                ? e.ports.map((p) => p.port ? `${p.port}/${p.protocol}` : p.protocol).join(", ")
                : "";
              const mx = (s.x + t.x) / 2;
              const my = (s.y + t.y) / 2;

              return (
                <g key={`${e.source}-${e.target}-${e.direction}-${i}`} style={{ transition: "opacity 0.2s" }} opacity={opacity}>
                  <path
                    d={edgePath(s.x, s.y, t.x, t.y)}
                    fill="none"
                    stroke={color}
                    strokeWidth={1.5}
                    strokeDasharray="4 3"
                    markerEnd={marker}
                  />
                  {portLabel && (
                    <text
                      x={mx}
                      y={my - 6}
                      fill={color}
                      fontSize={8}
                      fontFamily="SFMono-Regular, Menlo, Monaco, Consolas, monospace"
                      textAnchor="middle"
                      opacity={0.8}
                    >
                      {portLabel}
                    </text>
                  )}
                </g>
              );
            })}

            {/* Nodes */}
            {simNodes.map((node) => {
              const opacity = getNodeOpacity(node.id);
              const isHovered = hoveredNode === node.id;
              const isCidr = node.type === "cidr";
              const color = isCidr ? CIDR_COLOR : getGroupColor(node.kind);
              const r = isCidr ? CIDR_RADIUS : NODE_RADIUS;
              const isIsolated = node.isIsolatedIngress || node.isIsolatedEgress;

              return (
                <g
                  key={node.id}
                  opacity={opacity}
                  style={{ transition: "opacity 0.2s", cursor: "grab" }}
                  onMouseDown={(e) => handleNodeMouseDown(e, node.id)}
                  onMouseEnter={() => setHoveredNode(node.id)}
                  onMouseLeave={() => setHoveredNode(null)}
                >
                  {/* Isolation ring */}
                  {isIsolated && (
                    <circle
                      cx={node.x}
                      cy={node.y}
                      r={r + 6}
                      fill="none"
                      stroke="#ef4444"
                      strokeWidth={2}
                      strokeDasharray="4 2"
                      opacity={0.6}
                    />
                  )}
                  {/* Hover glow */}
                  {isHovered && (
                    <circle
                      cx={node.x}
                      cy={node.y}
                      r={r + 4}
                      fill="none"
                      stroke={color.stroke}
                      strokeWidth={1}
                      opacity={0.5}
                    />
                  )}
                  {/* Node circle */}
                  <circle
                    cx={node.x}
                    cy={node.y}
                    r={r}
                    fill={color.fill}
                    stroke={color.stroke}
                    strokeWidth={isHovered ? 2 : 1.5}
                  />
                  {/* Icon */}
                  <foreignObject
                    x={node.x - 7}
                    y={node.y - (isCidr ? 7 : 14)}
                    width={14}
                    height={14}
                  >
                    <div
                      // @ts-expect-error xmlns is valid for foreignObject children
                      xmlns="http://www.w3.org/1999/xhtml"
                      style={{ display: "flex", alignItems: "center", justifyContent: "center" }}
                    >
                      <KindIcon kind={node.kind} color={color.text} />
                    </div>
                  </foreignObject>
                  {/* Label */}
                  <text
                    x={node.x}
                    y={node.y + (isCidr ? 5 : 6)}
                    fill="#e2e8f0"
                    fontSize={isCidr ? 7 : 9}
                    fontFamily="SFMono-Regular, Menlo, Monaco, Consolas, monospace"
                    textAnchor="middle"
                    fontWeight={500}
                  >
                    {node.label.length > 16 ? node.label.slice(0, 14) + ".." : node.label}
                  </text>
                  {/* Pod count badge */}
                  {!isCidr && node.podCount > 0 && (
                    <>
                      <circle
                        cx={node.x + r - 4}
                        cy={node.y - r + 4}
                        r={8}
                        fill="#0f172a"
                        stroke={color.stroke}
                        strokeWidth={1}
                      />
                      <text
                        x={node.x + r - 4}
                        y={node.y - r + 7}
                        fill={color.text}
                        fontSize={8}
                        fontFamily="SFMono-Regular, Menlo, Monaco, Consolas, monospace"
                        textAnchor="middle"
                        fontWeight={600}
                      >
                        {node.podCount}
                      </text>
                    </>
                  )}
                  {/* Isolation shield icon */}
                  {isIsolated && (
                    <foreignObject
                      x={node.x - r + 1}
                      y={node.y - r + 1}
                      width={12}
                      height={12}
                    >
                      <div
                        // @ts-expect-error xmlns is valid for foreignObject children
                        xmlns="http://www.w3.org/1999/xhtml"
                        style={{ display: "flex", alignItems: "center", justifyContent: "center" }}
                      >
                        <ShieldAlert width={10} height={10} color="#ef4444" strokeWidth={2} />
                      </div>
                    </foreignObject>
                  )}
                </g>
              );
            })}
          </g>
        </svg>

        {/* Simulation Panel */}
        <AnimatePresence>
          {simOpen && (
            <motion.div
              initial={{ y: "100%" }}
              animate={{ y: 0 }}
              exit={{ y: "100%" }}
              transition={{ type: "spring", stiffness: 400, damping: 35 }}
              className="absolute bottom-0 left-0 z-30 bg-surface/95 border-t border-slate-800 backdrop-blur-sm"
              style={{ right: policyPanelOpen ? "300px" : "0px" }}
            >
              <div className="p-4">
                <div className="flex items-center justify-between mb-3">
                  <h3 className="text-sm font-medium text-slate-200 flex items-center gap-2">
                    <ShieldCheck className="w-4 h-4 text-accent" />
                    Traffic Simulation
                  </h3>
                  <button onClick={() => setSimOpen(false)} className="text-slate-500 hover:text-slate-300">
                    <X className="w-4 h-4" />
                  </button>
                </div>
                <div className="flex items-end gap-3 flex-wrap">
                  <div className="flex-1 min-w-[140px]">
                    <label className="text-[10px] text-slate-500 uppercase mb-1 block">Source NS</label>
                    <select
                      value={simSourceNs}
                      onChange={(e) => { setSimSourceNs(e.target.value); setSimSourcePod(""); }}
                      className="w-full bg-background border border-slate-800 rounded px-2 py-1.5 text-xs text-slate-200 focus:border-accent/50 outline-none"
                    >
                      <option value="">Select...</option>
                      {podNamespaces.map((ns) => <option key={ns} value={ns}>{ns}</option>)}
                    </select>
                  </div>
                  <div className="flex-1 min-w-[160px]">
                    <label className="text-[10px] text-slate-500 uppercase mb-1 block">Source Pod</label>
                    <select
                      value={simSourcePod}
                      onChange={(e) => setSimSourcePod(e.target.value)}
                      className="w-full bg-background border border-slate-800 rounded px-2 py-1.5 text-xs text-slate-200 focus:border-accent/50 outline-none"
                    >
                      <option value="">Select...</option>
                      {allPods.filter((p) => p.namespace === simSourceNs).map((p) => (
                        <option key={p.name} value={p.name}>{p.name}</option>
                      ))}
                    </select>
                  </div>
                  <div className="text-slate-600 text-sm px-2 pb-1">&rarr;</div>
                  <div className="flex-1 min-w-[140px]">
                    <label className="text-[10px] text-slate-500 uppercase mb-1 block">Dest NS</label>
                    <select
                      value={simDestNs}
                      onChange={(e) => { setSimDestNs(e.target.value); setSimDestPod(""); }}
                      className="w-full bg-background border border-slate-800 rounded px-2 py-1.5 text-xs text-slate-200 focus:border-accent/50 outline-none"
                    >
                      <option value="">Select...</option>
                      {podNamespaces.map((ns) => <option key={ns} value={ns}>{ns}</option>)}
                    </select>
                  </div>
                  <div className="flex-1 min-w-[160px]">
                    <label className="text-[10px] text-slate-500 uppercase mb-1 block">Dest Pod</label>
                    <select
                      value={simDestPod}
                      onChange={(e) => setSimDestPod(e.target.value)}
                      className="w-full bg-background border border-slate-800 rounded px-2 py-1.5 text-xs text-slate-200 focus:border-accent/50 outline-none"
                    >
                      <option value="">Select...</option>
                      {allPods.filter((p) => p.namespace === simDestNs).map((p) => (
                        <option key={p.name} value={p.name}>{p.name}</option>
                      ))}
                    </select>
                  </div>
                  <div className="w-20">
                    <label className="text-[10px] text-slate-500 uppercase mb-1 block">Port</label>
                    <input
                      value={simPort}
                      onChange={(e) => setSimPort(e.target.value)}
                      placeholder="Any"
                      className="w-full bg-background border border-slate-800 rounded px-2 py-1.5 text-xs text-slate-200 focus:border-accent/50 outline-none placeholder:text-slate-600"
                    />
                  </div>
                  <div className="w-20">
                    <label className="text-[10px] text-slate-500 uppercase mb-1 block">Proto</label>
                    <select
                      value={simProtocol}
                      onChange={(e) => setSimProtocol(e.target.value)}
                      className="w-full bg-background border border-slate-800 rounded px-2 py-1.5 text-xs text-slate-200 focus:border-accent/50 outline-none"
                    >
                      <option value="TCP">TCP</option>
                      <option value="UDP">UDP</option>
                      <option value="SCTP">SCTP</option>
                    </select>
                  </div>
                  <button
                    onClick={runSimulation}
                    disabled={simLoading || !simSourcePod || !simDestPod}
                    className="px-4 py-1.5 text-xs font-medium rounded bg-accent/20 text-accent border border-accent/30 hover:bg-accent/30 transition disabled:opacity-40 disabled:cursor-not-allowed"
                  >
                    {simLoading ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : "Test"}
                  </button>
                </div>

                {simResult && (
                  <div className={`mt-3 px-3 py-2 rounded-lg border text-xs ${
                    simResult.allowed
                      ? "bg-emerald-500/10 border-emerald-500/30 text-emerald-400"
                      : "bg-red-500/10 border-red-500/30 text-red-400"
                  }`}>
                    <div className="font-medium mb-1">{simResult.allowed ? "ALLOWED" : "DENIED"}</div>
                    <div className="text-slate-400">{simResult.summary}</div>
                    {simResult.egress_evaluation.policy_results.length > 0 && (
                      <div className="mt-2">
                        <span className="text-slate-500">Egress:</span>
                        {simResult.egress_evaluation.policy_results.filter((r) => r.selects_pod).map((r) => (
                          <span key={r.policy_name} className={`ml-2 ${r.allows_traffic ? "text-emerald-400" : "text-red-400"}`}>
                            {r.policy_name}: {r.reason}
                          </span>
                        ))}
                      </div>
                    )}
                    {simResult.ingress_evaluation.policy_results.length > 0 && (
                      <div className="mt-1">
                        <span className="text-slate-500">Ingress:</span>
                        {simResult.ingress_evaluation.policy_results.filter((r) => r.selects_pod).map((r) => (
                          <span key={r.policy_name} className={`ml-2 ${r.allows_traffic ? "text-emerald-400" : "text-red-400"}`}>
                            {r.policy_name}: {r.reason}
                          </span>
                        ))}
                      </div>
                    )}
                  </div>
                )}
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </div>

      {/* Policy List Panel */}
      <div className={`relative transition-all duration-300 ${policyPanelOpen ? "w-[300px]" : "w-0"}`}>
        <button
          onClick={() => setPolicyPanelOpen(!policyPanelOpen)}
          className="absolute -left-6 top-3 z-30 w-6 h-8 bg-surface/90 border border-slate-800 border-r-0 rounded-l-md flex items-center justify-center text-slate-500 hover:text-slate-300 transition"
        >
          {policyPanelOpen ? <ChevronDown className="w-3 h-3 rotate-[-90deg]" /> : <ChevronUp className="w-3 h-3 rotate-[-90deg]" />}
        </button>

        {policyPanelOpen && (
          <div className="w-[300px] h-full border-l border-slate-800 bg-surface/80 glass flex flex-col overflow-hidden">
            <div className="p-3 border-b border-slate-800/50">
              <h3 className="text-xs font-medium text-slate-300 mb-2 flex items-center gap-1.5">
                <Shield className="w-3.5 h-3.5 text-accent" />
                Network Policies ({graph.policies.length})
              </h3>
              <div className="relative">
                <Search className="absolute left-2 top-1/2 -translate-y-1/2 w-3 h-3 text-slate-500" />
                <input
                  value={policySearch}
                  onChange={(e) => setPolicySearch(e.target.value)}
                  placeholder="Filter policies..."
                  className="w-full pl-6 pr-6 py-1 text-[11px] font-mono bg-background border border-slate-800 rounded placeholder:text-slate-600 text-slate-300 focus:outline-none focus:border-accent/50 transition"
                />
                {policySearch && (
                  <button onClick={() => setPolicySearch("")} className="absolute right-1.5 top-1/2 -translate-y-1/2 text-slate-500 hover:text-slate-300">
                    <X className="w-3 h-3" />
                  </button>
                )}
              </div>
            </div>
            <div className="flex-1 overflow-y-auto p-2 space-y-1.5">
              {filteredPolicies.map((policy) => (
                <PolicyCard
                  key={`${policy.namespace}/${policy.name}`}
                  policy={policy}
                  isSelected={selectedPolicy === policy.name}
                  onClick={() => setSelectedPolicy(selectedPolicy === policy.name ? null : policy.name)}
                />
              ))}
              {filteredPolicies.length === 0 && (
                <p className="text-[10px] text-slate-600 text-center py-4">No matching policies</p>
              )}
            </div>
          </div>
        )}
      </div>
    </motion.div>
  );
}

// ── Policy Card ──────────────────────────────────────────────────────

function PolicyCard({
  policy,
  isSelected,
  onClick,
}: {
  policy: NetworkPolicySummary;
  isSelected: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className={`w-full text-left p-2.5 rounded-lg border transition ${
        isSelected
          ? "border-accent/50 bg-accent/10"
          : "border-slate-800/50 bg-background/50 hover:border-slate-700"
      }`}
    >
      <div className="flex items-center gap-1.5 mb-1.5">
        <Shield className="w-3 h-3 text-accent flex-shrink-0" />
        <span className="text-[11px] font-medium text-slate-200 truncate">{policy.name}</span>
      </div>
      <div className="text-[10px] text-slate-500 mb-1.5">
        ns: {policy.namespace}
      </div>
      <div className="flex flex-wrap gap-1 mb-1.5">
        {policy.policy_types.map((t) => (
          <span
            key={t}
            className={`px-1.5 py-0.5 rounded text-[9px] font-medium ${
              t === "Ingress"
                ? "bg-emerald-500/15 text-emerald-400"
                : "bg-blue-500/15 text-blue-400"
            }`}
          >
            {t}
          </span>
        ))}
        <span className="px-1.5 py-0.5 rounded text-[9px] bg-slate-800 text-slate-400">
          {policy.affected_pod_count} pod{policy.affected_pod_count !== 1 ? "s" : ""}
        </span>
      </div>
      {Object.keys(policy.pod_selector).length > 0 && (
        <div className="flex flex-wrap gap-1">
          {Object.entries(policy.pod_selector).map(([k, v]) => (
            <span key={k} className="text-[9px] px-1.5 py-0.5 rounded bg-slate-800/80 text-slate-400 font-mono">
              {k}={v}
            </span>
          ))}
        </div>
      )}
      {Object.keys(policy.pod_selector).length === 0 && (
        <span className="text-[9px] px-1.5 py-0.5 rounded bg-slate-800/80 text-slate-400 font-mono italic">
          all pods
        </span>
      )}
      <div className="flex gap-2 mt-1.5 text-[9px] text-slate-500">
        {policy.ingress_rule_count > 0 && <span>{policy.ingress_rule_count} ingress rule{policy.ingress_rule_count > 1 ? "s" : ""}</span>}
        {policy.egress_rule_count > 0 && <span>{policy.egress_rule_count} egress rule{policy.egress_rule_count > 1 ? "s" : ""}</span>}
      </div>
    </button>
  );
}
