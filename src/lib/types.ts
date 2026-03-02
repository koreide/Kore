export type ResourceKind =
  | "pods"
  | "deployments"
  | "services"
  | "nodes"
  | "events"
  | "configmaps"
  | "secrets"
  | "ingresses"
  | "jobs"
  | "cronjobs"
  | "namespaces";

export type AppView =
  | "table"
  | "details"
  | "dashboard"
  | "graph"
  | "crds"
  | "helm"
  | "helm-detail"
  | "settings"
  | "chat"
  | "network-policies";

export interface ResourceItem {
  name: string;
  namespace?: string;
  status?: string;
  age?: string;
  ready?: string;
  restarts?: number;
  node?: string;
  ip?: string;
  upToDate?: number;
  available?: number;
  type?: string;
  clusterIp?: string;
  externalIp?: string;
  ports?: string;
  roles?: string;
  version?: string;
  // Event fields
  reason?: string;
  message?: string;
  count?: number;
  lastSeen?: string;
  involvedObject?: string;
  eventType?: string;
  // Job fields
  completions?: string;
  duration?: string;
  schedule?: string;
  lastSchedule?: string;
  active?: number;
  // ConfigMap/Secret fields
  dataKeys?: number;
  // Ingress fields
  hosts?: string;
  ingressClass?: string;
  // Search result kind marker
  _kind?: string;
  // Multi-cluster context marker
  _context?: string;
}

export type WatchEventPayload = {
  action: "applied" | "deleted" | "error";
  kind: string;
  object: KubernetesObject;
};

export interface KubernetesObject {
  metadata?: ObjectMeta;
  status?: Record<string, unknown>;
  spec?: Record<string, unknown>;
  [key: string]: unknown;
}

export interface ObjectMeta {
  name?: string;
  namespace?: string;
  creationTimestamp?: string;
  labels?: Record<string, string>;
  annotations?: Record<string, string>;
  uid?: string;
  resourceVersion?: string;
  [key: string]: unknown;
}

export interface ContainerStatus {
  name: string;
  ready: boolean;
  restartCount: number;
  state?: Record<string, unknown>;
  image?: string;
}

export interface PodCondition {
  type: string;
  status: string;
  lastTransitionTime?: string;
  reason?: string;
  message?: string;
}

export interface ContainerResource {
  cpu?: string;
  memory?: string;
}

export interface Container {
  name: string;
  image?: string;
  resources?: {
    limits?: ContainerResource;
    requests?: ContainerResource;
  };
}

export interface NodeInfo {
  kubeletVersion?: string;
  osImage?: string;
  architecture?: string;
  containerRuntimeVersion?: string;
}

export interface ConnectionStatus {
  connected: boolean;
  error: string | null;
  error_kind:
    | "no_kubeconfig"
    | "invalid_context"
    | "cluster_unreachable"
    | "auth_failed"
    | "unknown"
    | null;
  kubeconfig_path: string | null;
  contexts_available: string[];
  current_context: string | null;
}

export interface PortForwardInfo {
  id: string;
  localPort: number;
  podPort: number;
  status: "connecting" | "active" | "error";
  statusMessage?: string;
  localAddress: string;
}
