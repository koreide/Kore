import { describe, it, expect } from "vitest";
import { toResourceItem } from "../transforms";
import type { KubernetesObject } from "../types";

describe("toResourceItem — events", () => {
  it("extracts event fields correctly", () => {
    const event: KubernetesObject & Record<string, unknown> = {
      metadata: {
        name: "my-event.abc123",
        namespace: "default",
        creationTimestamp: "2024-06-01T10:00:00Z",
      },
      reason: "BackOff",
      message: "Back-off restarting failed container",
      count: 5,
      lastTimestamp: "2024-06-01T10:05:00Z",
      type: "Warning",
      involvedObject: { kind: "Pod", name: "my-pod" },
    };

    const item = toResourceItem(event);
    expect(item).not.toBeNull();
    expect(item!.reason).toBe("BackOff");
    expect(item!.message).toBe("Back-off restarting failed container");
    expect(item!.count).toBe(5);
    expect(item!.lastSeen).toBe("2024-06-01T10:05:00Z");
    expect(item!.eventType).toBe("Warning");
    expect(item!.involvedObject).toBe("Pod/my-pod");
  });

  it("falls back to creationTimestamp for lastSeen", () => {
    const event: KubernetesObject & Record<string, unknown> = {
      metadata: {
        name: "evt-no-ts",
        namespace: "default",
        creationTimestamp: "2024-01-01T00:00:00Z",
      },
      reason: "Scheduled",
    };

    const item = toResourceItem(event);
    expect(item!.lastSeen).toBe("2024-01-01T00:00:00Z");
  });
});

describe("toResourceItem — jobs", () => {
  it("extracts completed job fields", () => {
    const job: KubernetesObject = {
      metadata: {
        name: "my-job",
        namespace: "batch",
        creationTimestamp: "2024-06-01T00:00:00Z",
      },
      spec: { completions: 3 },
      status: {
        conditions: [{ type: "Complete", status: "True" }],
        succeeded: 3,
        startTime: "2024-06-01T00:00:00Z",
        completionTime: "2024-06-01T00:01:30Z",
      },
    };

    const item = toResourceItem(job);
    expect(item).not.toBeNull();
    expect(item!.status).toBe("Complete");
    expect(item!.completions).toBe("3/3");
    expect(item!.duration).toBe("1m30s");
  });

  it("extracts failed job status", () => {
    const job: KubernetesObject = {
      metadata: { name: "fail-job", namespace: "default" },
      spec: { completions: 1 },
      status: {
        conditions: [{ type: "Failed", status: "True" }],
        succeeded: 0,
      },
    };

    const item = toResourceItem(job);
    expect(item!.status).toBe("Failed");
    expect(item!.completions).toBe("0/1");
  });

  it("computes short duration in seconds", () => {
    const job: KubernetesObject = {
      metadata: { name: "quick-job", namespace: "default" },
      status: {
        startTime: "2024-06-01T00:00:00Z",
        completionTime: "2024-06-01T00:00:45Z",
      },
    };

    const item = toResourceItem(job);
    expect(item!.duration).toBe("45s");
  });

  it("computes long duration in hours", () => {
    const job: KubernetesObject = {
      metadata: { name: "long-job", namespace: "default" },
      status: {
        startTime: "2024-06-01T00:00:00Z",
        completionTime: "2024-06-01T02:30:00Z",
      },
    };

    const item = toResourceItem(job);
    expect(item!.duration).toBe("2h30m");
  });
});

describe("toResourceItem — cronjobs", () => {
  it("extracts cronjob fields", () => {
    const cj: KubernetesObject = {
      metadata: {
        name: "my-cronjob",
        namespace: "default",
        creationTimestamp: "2024-01-01T00:00:00Z",
      },
      spec: { schedule: "*/5 * * * *" },
      status: {
        lastScheduleTime: "2024-06-01T10:00:00Z",
        active: [{ name: "job-1" }, { name: "job-2" }],
      },
    };

    const item = toResourceItem(cj);
    expect(item).not.toBeNull();
    expect(item!.schedule).toBe("*/5 * * * *");
    expect(item!.lastSchedule).toBe("2024-06-01T10:00:00Z");
    expect(item!.active).toBe(2);
  });
});

describe("toResourceItem — configmaps and secrets", () => {
  it("counts configmap data keys", () => {
    const cm: KubernetesObject & Record<string, unknown> = {
      metadata: { name: "my-config", namespace: "default" },
      data: { key1: "val1", key2: "val2", key3: "val3" },
    };

    const item = toResourceItem(cm);
    expect(item).not.toBeNull();
    expect(item!.dataKeys).toBe(3);
  });

  it("returns undefined dataKeys when no data", () => {
    const secret: KubernetesObject = {
      metadata: { name: "my-secret", namespace: "default" },
    };

    const item = toResourceItem(secret);
    expect(item!.dataKeys).toBeUndefined();
  });

  it("counts zero data keys for empty data", () => {
    const cm: KubernetesObject & Record<string, unknown> = {
      metadata: { name: "empty-config", namespace: "default" },
      data: {},
    };

    const item = toResourceItem(cm);
    expect(item!.dataKeys).toBe(0);
  });
});

describe("toResourceItem — ingresses", () => {
  it("extracts ingress fields", () => {
    const ingress: KubernetesObject = {
      metadata: { name: "my-ingress", namespace: "default" },
      spec: {
        ingressClassName: "nginx",
        rules: [{ host: "example.com" }, { host: "api.example.com" }],
      },
    };

    const item = toResourceItem(ingress);
    expect(item).not.toBeNull();
    expect(item!.ingressClass).toBe("nginx");
    expect(item!.hosts).toBe("example.com, api.example.com");
  });

  it("uses * for rules without host", () => {
    const ingress: KubernetesObject = {
      metadata: { name: "wildcard-ingress", namespace: "default" },
      spec: {
        rules: [{ http: { paths: [] } }],
      },
    };

    const item = toResourceItem(ingress);
    expect(item!.hosts).toBe("*");
  });
});

describe("toResourceItem — multi-cluster and search markers", () => {
  it("extracts _context field", () => {
    const pod: KubernetesObject & Record<string, unknown> = {
      metadata: { name: "pod-1", namespace: "default" },
      _context: "staging-cluster",
    };

    const item = toResourceItem(pod);
    expect(item!._context).toBe("staging-cluster");
  });

  it("extracts _kind field", () => {
    const obj: KubernetesObject & Record<string, unknown> = {
      metadata: { name: "some-resource", namespace: "default" },
      _kind: "pods",
    };

    const item = toResourceItem(obj);
    expect(item!._kind).toBe("pods");
  });

  it("returns undefined for missing markers", () => {
    const obj: KubernetesObject = {
      metadata: { name: "plain", namespace: "default" },
    };

    const item = toResourceItem(obj);
    expect(item!._context).toBeUndefined();
    expect(item!._kind).toBeUndefined();
  });
});

describe("toResourceItem — service external IP edge cases", () => {
  it("uses hostname from LoadBalancer ingress", () => {
    const svc: KubernetesObject = {
      metadata: { name: "lb-hostname", namespace: "default" },
      spec: { type: "LoadBalancer" },
      status: {
        loadBalancer: {
          ingress: [{ hostname: "abc.elb.amazonaws.com" }],
        },
      },
    };

    const item = toResourceItem(svc);
    expect(item!.externalIp).toBe("abc.elb.amazonaws.com");
  });

  it("uses externalIPs from spec", () => {
    const svc: KubernetesObject = {
      metadata: { name: "external-svc", namespace: "default" },
      spec: {
        type: "ClusterIP",
        externalIPs: ["203.0.113.1"],
      },
    };

    const item = toResourceItem(svc);
    expect(item!.externalIp).toBe("203.0.113.1");
  });
});
