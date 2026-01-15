import { invoke } from "@tauri-apps/api/core";

export type ResourceKind = "pods" | "deployments" | "services" | "nodes";

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
}

export async function listContexts(): Promise<string[]> {
  return invoke("list_contexts");
}

export async function listNamespaces(): Promise<string[]> {
  return invoke("list_namespaces");
}

export async function switchContext(name: string): Promise<void> {
  return invoke("switch_context", { name });
}

export async function fetchLogs(params: {
  namespace: string;
  pod: string;
  container?: string;
  tailLines?: number;
}): Promise<string> {
  return invoke("fetch_logs", {
    ...params,
    tail_lines: params.tailLines
  });
}

export async function deleteResource(params: {
  kind: ResourceKind;
  namespace: string;
  name: string;
}): Promise<void> {
  return invoke("delete_resource", params);
}

export async function listResources(kind: ResourceKind, namespace?: string): Promise<Record<string, unknown>[]> {
  return invoke("list_resources", { kind, namespace });
}

export async function startWatch(kind: ResourceKind, namespace?: string): Promise<void> {
  return invoke("start_watch", { kind, namespace });
}

export async function describePod(namespace: string, podName: string): Promise<Record<string, unknown>> {
  return invoke("describe_pod", { namespace, podName });
}

export async function startPodLogsStream(
  namespace: string,
  podName: string,
  container?: string
): Promise<void> {
  return invoke("start_pod_logs_stream", { namespace, podName, container });
}

export async function getPodMetrics(
  namespace: string,
  podName: string
): Promise<Record<string, unknown>> {
  return invoke("get_pod_metrics", { namespace, podName });
}

export interface PortForwardRequest {
  namespace: string;
  podName: string;
  localPort: number;
  podPort: number;
}

export interface PortForwardResponse {
  localPort: number;
  podPort: number;
  status: string;
}

export async function startPortForward(
  request: PortForwardRequest
): Promise<PortForwardResponse> {
  return invoke("start_port_forward", {
    namespace: request.namespace,
    podName: request.podName,
    localPort: request.localPort,
    podPort: request.podPort,
  });
}

export async function stopPortForward(
  request: PortForwardRequest
): Promise<void> {
  return invoke("stop_port_forward", {
    namespace: request.namespace,
    podName: request.podName,
    localPort: request.localPort,
    podPort: request.podPort,
  });
}
