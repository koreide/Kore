import { describe, it, expect } from "vitest";
import { toResourceItem } from "../transforms";
import type { KubernetesObject } from "../types";

describe("toResourceItem", () => {
  it("returns null for objects with no name", () => {
    const obj: KubernetesObject = { metadata: {} };
    expect(toResourceItem(obj)).toBeNull();
  });

  it("returns null for objects with empty name", () => {
    const obj: KubernetesObject = { metadata: { name: "" } };
    expect(toResourceItem(obj)).toBeNull();
  });

  it("extracts pod fields correctly", () => {
    const pod: KubernetesObject = {
      metadata: {
        name: "my-pod",
        namespace: "default",
        creationTimestamp: "2024-01-01T00:00:00Z",
      },
      status: {
        phase: "Running",
        podIP: "10.0.0.1",
        hostIP: "192.168.1.1",
        containerStatuses: [
          { name: "app", ready: true, restartCount: 3 },
          { name: "sidecar", ready: false, restartCount: 0 },
        ],
      },
    };

    const item = toResourceItem(pod);
    expect(item).not.toBeNull();
    expect(item!.name).toBe("my-pod");
    expect(item!.namespace).toBe("default");
    expect(item!.ready).toBe("1/2");
    expect(item!.restarts).toBe(3);
    expect(item!.ip).toBe("10.0.0.1");
    expect(item!.node).toBe("192.168.1.1");
    expect(item!.age).toBe("2024-01-01T00:00:00Z");
  });

  it("extracts deployment fields correctly", () => {
    const deployment: KubernetesObject = {
      metadata: {
        name: "my-deploy",
        namespace: "prod",
        creationTimestamp: "2024-06-01T12:00:00Z",
      },
      status: {
        readyReplicas: 3,
        replicas: 5,
        updatedReplicas: 3,
        availableReplicas: 3,
      },
    };

    const item = toResourceItem(deployment);
    expect(item).not.toBeNull();
    expect(item!.name).toBe("my-deploy");
    expect(item!.ready).toBe("3/5");
    expect(item!.upToDate).toBe(3);
    expect(item!.available).toBe(3);
  });

  it("extracts service fields correctly", () => {
    const service: KubernetesObject = {
      metadata: {
        name: "my-service",
        namespace: "default",
        creationTimestamp: "2024-01-15T00:00:00Z",
      },
      spec: {
        type: "ClusterIP",
        clusterIP: "10.96.0.1",
        ports: [
          { port: 80, protocol: "TCP" },
          { port: 443, nodePort: 30443, protocol: "TCP" },
        ],
      },
    };

    const item = toResourceItem(service);
    expect(item).not.toBeNull();
    expect(item!.name).toBe("my-service");
    expect(item!.type).toBe("ClusterIP");
    expect(item!.clusterIp).toBe("10.96.0.1");
    expect(item!.ports).toBe("80/TCP,30443:443/TCP");
    expect(item!.externalIp).toBe("<none>");
  });

  it("extracts LoadBalancer external IP correctly", () => {
    const svc: KubernetesObject = {
      metadata: { name: "lb-svc", namespace: "default" },
      spec: { type: "LoadBalancer", clusterIP: "10.0.0.1" },
      status: {
        loadBalancer: {
          ingress: [{ ip: "34.120.0.1" }],
        },
      },
    };

    const item = toResourceItem(svc);
    expect(item!.externalIp).toBe("34.120.0.1");
  });

  it("shows <pending> for LoadBalancer without ingress", () => {
    const svc: KubernetesObject = {
      metadata: { name: "lb-pending", namespace: "default" },
      spec: { type: "LoadBalancer", clusterIP: "10.0.0.1" },
      status: {},
    };

    const item = toResourceItem(svc);
    expect(item!.externalIp).toBe("<pending>");
  });

  it("extracts node fields correctly", () => {
    const node: KubernetesObject = {
      metadata: {
        name: "node-1",
        creationTimestamp: "2024-01-01T00:00:00Z",
        labels: {
          "node-role.kubernetes.io/control-plane": "",
          "node-role.kubernetes.io/master": "",
        },
      },
      status: {
        conditions: [{ type: "Ready", status: "True" }],
        nodeInfo: { kubeletVersion: "v1.29.0" },
      },
    };

    const item = toResourceItem(node);
    expect(item).not.toBeNull();
    expect(item!.name).toBe("node-1");
    expect(item!.status).toBe("Ready");
    expect(item!.roles).toBe("control-plane,master");
    expect(item!.version).toBe("v1.29.0");
  });

  it("shows NotReady for node with false Ready condition", () => {
    const node: KubernetesObject = {
      metadata: { name: "bad-node" },
      status: {
        conditions: [{ type: "Ready", status: "False" }],
      },
    };

    const item = toResourceItem(node);
    expect(item!.status).toBe("NotReady");
  });

  it("shows <none> for roles when no role labels exist", () => {
    const node: KubernetesObject = {
      metadata: { name: "worker", labels: { app: "test" } },
      status: {},
    };

    const item = toResourceItem(node);
    expect(item!.roles).toBe("<none>");
  });

  it("handles missing metadata gracefully", () => {
    const obj: KubernetesObject = {};
    expect(toResourceItem(obj)).toBeNull();
  });

  it("extracts kubernetes.io/role label", () => {
    const node: KubernetesObject = {
      metadata: {
        name: "legacy-node",
        labels: { "kubernetes.io/role": "worker" },
      },
      status: {},
    };

    const item = toResourceItem(node);
    expect(item!.roles).toBe("worker");
  });
});
