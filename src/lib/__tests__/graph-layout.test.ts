import { describe, it, expect } from "vitest";
import {
  getLayerForKind,
  getKindColor,
  getStatusBorderColor,
  computeLayout,
  computeEdgePath,
  NODE_WIDTH,
  NODE_HEIGHT,
  LAYER_GAP_Y,
  NODE_GAP_X,
  PADDING,
} from "../graph-layout";
import type { GraphNode, GraphEdge } from "@/lib/api";
import type { LayoutNode } from "../graph-layout";

function makeNode(id: string, kind: string, status = "Running"): GraphNode {
  return { id, name: id, namespace: "default", kind, status };
}

describe("getLayerForKind", () => {
  it("returns correct layer for known kinds", () => {
    expect(getLayerForKind("Ingress")).toBe(0);
    expect(getLayerForKind("Service")).toBe(1);
    expect(getLayerForKind("Deployment")).toBe(2);
    expect(getLayerForKind("CronJob")).toBe(2);
    expect(getLayerForKind("ReplicaSet")).toBe(3);
    expect(getLayerForKind("Job")).toBe(3);
    expect(getLayerForKind("Pod")).toBe(4);
  });

  it("returns default layer 2 for unknown kinds", () => {
    expect(getLayerForKind("ConfigMap")).toBe(2);
    expect(getLayerForKind("Unknown")).toBe(2);
  });
});

describe("getKindColor", () => {
  it("returns correct colors for known kinds", () => {
    const deployColor = getKindColor("Deployment");
    expect(deployColor.stroke).toBe("#3b82f6");

    const podColor = getKindColor("Pod");
    expect(podColor.stroke).toBe("#10b981");

    const svcColor = getKindColor("Service");
    expect(svcColor.stroke).toBe("#a855f7");
  });

  it("returns default color for unknown kinds", () => {
    const color = getKindColor("SomeCustomResource");
    expect(color).toEqual({ fill: "#1e293b", stroke: "#64748b", text: "#94a3b8" });
  });
});

describe("getStatusBorderColor", () => {
  it("returns emerald for running/ready/active states", () => {
    for (const s of [
      "Running",
      "Ready",
      "Active",
      "Succeeded",
      "Complete",
      "Completed",
      "Available",
    ]) {
      expect(getStatusBorderColor(s)).toBe("#10b981");
    }
  });

  it("returns amber for pending/waiting states", () => {
    for (const s of ["Pending", "ContainerCreating", "Init", "Waiting"]) {
      expect(getStatusBorderColor(s)).toBe("#f59e0b");
    }
  });

  it("returns red for failed/error states", () => {
    for (const s of [
      "Failed",
      "CrashLoopBackOff",
      "Error",
      "ImagePullBackOff",
      "Evicted",
      "OOMKilled",
    ]) {
      expect(getStatusBorderColor(s)).toBe("#ef4444");
    }
  });

  it("returns slate for unknown status", () => {
    expect(getStatusBorderColor("Unknown")).toBe("#64748b");
    expect(getStatusBorderColor("SomethingElse")).toBe("#64748b");
  });
});

describe("computeLayout", () => {
  it("returns empty layout for empty graph", () => {
    const result = computeLayout([], []);
    expect(result).toEqual({ layoutNodes: [], width: 0, height: 0 });
  });

  it("positions a single node with correct padding", () => {
    const nodes: GraphNode[] = [makeNode("d1", "Deployment")];
    const result = computeLayout(nodes, []);

    expect(result.layoutNodes).toHaveLength(1);
    const node = result.layoutNodes[0];

    // Single node: width = NODE_WIDTH + PADDING*2 = 180 + 120 = 300
    expect(result.width).toBe(NODE_WIDTH + PADDING * 2);
    // Single layer: height = PADDING*2 + NODE_HEIGHT = 120 + 56 = 176
    expect(result.height).toBe(PADDING * 2 + NODE_HEIGHT);
    // Node should be centered
    expect(node.x).toBe(PADDING);
    expect(node.y).toBe(PADDING);
  });

  it("spaces multiple nodes in the same layer horizontally", () => {
    const nodes: GraphNode[] = [
      makeNode("d1", "Deployment"),
      makeNode("d2", "Deployment"),
      makeNode("d3", "Deployment"),
    ];
    const result = computeLayout(nodes, []);

    expect(result.layoutNodes).toHaveLength(3);

    // All should be at the same Y position
    const ys = result.layoutNodes.map((n) => n.y);
    expect(new Set(ys).size).toBe(1);

    // Check horizontal spacing
    const xs = result.layoutNodes.map((n) => n.x).sort((a, b) => a - b);
    expect(xs[1] - xs[0]).toBe(NODE_WIDTH + NODE_GAP_X);
    expect(xs[2] - xs[1]).toBe(NODE_WIDTH + NODE_GAP_X);
  });

  it("positions multi-layer nodes with correct Y spacing", () => {
    const nodes: GraphNode[] = [
      makeNode("svc1", "Service"),
      makeNode("d1", "Deployment"),
      makeNode("pod1", "Pod"),
    ];
    const result = computeLayout(nodes, []);

    // 3 layers: Service (1), Deployment (2), Pod (4)
    const sorted = [...result.layoutNodes].sort((a, b) => a.y - b.y);
    // Service is layer 1, should be first
    expect(sorted[0].kind).toBe("Service");
    // Deployment is layer 2, should be second
    expect(sorted[1].kind).toBe("Deployment");
    // Pod is layer 4, should be third
    expect(sorted[2].kind).toBe("Pod");

    // Y spacing between consecutive layers
    expect(sorted[1].y - sorted[0].y).toBe(NODE_HEIGHT + LAYER_GAP_Y);
    expect(sorted[2].y - sorted[1].y).toBe(NODE_HEIGHT + LAYER_GAP_Y);
  });

  it("reduces crossings via edge-based ordering", () => {
    // Parent on right, child should be placed closer to parent
    const nodes: GraphNode[] = [
      makeNode("svc-left", "Service"),
      makeNode("svc-right", "Service"),
      makeNode("dep-child", "Deployment"),
    ];
    const edges: GraphEdge[] = [{ source: "svc-right", target: "dep-child", relation: "selects" }];

    const result = computeLayout(nodes, edges);
    const svcRight = result.layoutNodes.find((n) => n.id === "svc-right")!;
    const depChild = result.layoutNodes.find((n) => n.id === "dep-child")!;

    // The child should be positioned near its parent horizontally
    const xDiff = Math.abs(depChild.x + NODE_WIDTH / 2 - (svcRight.x + NODE_WIDTH / 2));
    // With edge-based ordering, the child's center should be reasonably close to its parent
    expect(xDiff).toBeLessThan(NODE_WIDTH + NODE_GAP_X + 1);
  });

  it("calculates canvas dimensions correctly", () => {
    const nodes: GraphNode[] = [
      makeNode("svc1", "Service"),
      makeNode("svc2", "Service"),
      makeNode("pod1", "Pod"),
    ];
    const result = computeLayout(nodes, []);

    // Width: widest layer has 2 nodes → 2*180 + 1*40 + 2*60 = 520
    const expectedWidth = 2 * NODE_WIDTH + NODE_GAP_X + 2 * PADDING;
    expect(result.width).toBe(expectedWidth);

    // Height: 2 layers → PADDING*2 + 2*NODE_HEIGHT + 1*LAYER_GAP_Y = 120 + 112 + 100 = 332
    const expectedHeight = PADDING * 2 + 2 * NODE_HEIGHT + LAYER_GAP_Y;
    expect(result.height).toBe(expectedHeight);
  });
});

describe("computeEdgePath", () => {
  it("generates a valid SVG cubic Bezier path", () => {
    const source: LayoutNode = {
      ...makeNode("s", "Service"),
      x: 100,
      y: 50,
    };
    const target: LayoutNode = {
      ...makeNode("t", "Pod"),
      x: 200,
      y: 250,
    };

    const path = computeEdgePath(source, target);

    // Should start with M and contain C (cubic Bezier)
    expect(path).toMatch(/^M\s/);
    expect(path).toContain("C");
  });

  it("computes correct start/end and midpoint", () => {
    const source: LayoutNode = { ...makeNode("s", "Service"), x: 0, y: 0 };
    const target: LayoutNode = { ...makeNode("t", "Pod"), x: 0, y: 200 };

    const path = computeEdgePath(source, target);

    // Source: center-x = 0 + 180/2 = 90, bottom-y = 0 + 56 = 56
    // Target: center-x = 0 + 180/2 = 90, top-y = 200
    // Mid-y = (56 + 200) / 2 = 128
    const sx = NODE_WIDTH / 2;
    const sy = NODE_HEIGHT;
    const tx = NODE_WIDTH / 2;
    const ty = 200;
    const midY = (sy + ty) / 2;

    expect(path).toBe(`M ${sx} ${sy} C ${sx} ${midY}, ${tx} ${midY}, ${tx} ${ty}`);
  });
});
