import { invoke } from "@tauri-apps/api/core";

// Re-export types from centralized types module
export type { ResourceKind, ResourceItem, WatchEventPayload, KubernetesObject } from "./types";

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
  previous?: boolean;
}): Promise<string> {
  return invoke("fetch_logs", {
    ...params,
    tail_lines: params.tailLines,
  });
}

export async function deleteResource(params: {
  kind: string;
  namespace: string;
  name: string;
}): Promise<void> {
  return invoke("delete_resource", params);
}

export async function listResources(
  kind: string,
  namespace?: string,
  labelSelector?: string,
): Promise<Record<string, unknown>[]> {
  return invoke("list_resources", { kind, namespace, labelSelector });
}

export async function startWatch(
  kind: string,
  namespace?: string,
  labelSelector?: string,
): Promise<void> {
  return invoke("start_watch", { kind, namespace, labelSelector });
}

export async function describePod(
  namespace: string,
  podName: string,
): Promise<Record<string, unknown>> {
  return invoke("describe_pod", { namespace, podName });
}

export async function describeResource(
  kind: string,
  namespace: string,
  name: string,
): Promise<Record<string, unknown>> {
  return invoke("describe_resource", { kind, namespace, name });
}

export async function listEventsForResource(
  kind: string,
  namespace: string,
  name: string,
): Promise<Record<string, unknown>[]> {
  return invoke("list_events_for_resource", { kind, namespace, name });
}

export async function startPodLogsStream(
  namespace: string,
  podName: string,
  container?: string,
  previous?: boolean,
): Promise<void> {
  return invoke("start_pod_logs_stream", { namespace, podName, container, previous });
}

export async function stopPodLogsStream(): Promise<void> {
  return invoke("stop_pod_logs_stream");
}

export async function getPodMetrics(
  namespace: string,
  podName: string,
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

export async function startPortForward(request: PortForwardRequest): Promise<PortForwardResponse> {
  return invoke("start_port_forward", {
    namespace: request.namespace,
    podName: request.podName,
    localPort: request.localPort,
    podPort: request.podPort,
  });
}

export async function stopPortForward(request: PortForwardRequest): Promise<void> {
  return invoke("stop_port_forward", {
    namespace: request.namespace,
    podName: request.podName,
    localPort: request.localPort,
    podPort: request.podPort,
  });
}

export async function scaleDeployment(
  namespace: string,
  name: string,
  replicas: number,
): Promise<void> {
  return invoke("scale_deployment", { namespace, name, replicas });
}

export async function restartDeployment(
  namespace: string,
  name: string,
): Promise<void> {
  return invoke("restart_deployment", { namespace, name });
}

export async function searchResources(
  query: string,
  namespace?: string,
): Promise<Record<string, unknown>[]> {
  return invoke("search_resources", { query, namespace });
}

export async function execIntoPod(
  namespace: string,
  podName: string,
  container?: string,
  shell?: string,
): Promise<string> {
  return invoke("exec_into_pod", { namespace, podName, container, shell });
}

export async function sendExecInput(
  sessionId: string,
  data: string,
): Promise<void> {
  return invoke("send_exec_input", { sessionId, data });
}

export async function resizeExec(
  sessionId: string,
  cols: number,
  rows: number,
): Promise<void> {
  return invoke("resize_exec", { sessionId, cols, rows });
}

export async function stopExec(sessionId: string): Promise<void> {
  return invoke("stop_exec", { sessionId });
}

// ── Phase 1: YAML Editor ──────────────────────────────────────────────

export async function getResourceYaml(
  kind: string,
  namespace: string,
  name: string,
): Promise<string> {
  return invoke("get_resource_yaml", { kind, namespace, name });
}

export async function applyResourceYaml(
  kind: string,
  namespace: string,
  name: string,
  yaml: string,
): Promise<string> {
  return invoke("apply_resource_yaml", { kind, namespace, name, yaml });
}

export interface DiffLine {
  tag: "equal" | "insert" | "delete";
  value: string;
}

export async function diffResourceYaml(
  kind: string,
  namespace: string,
  name: string,
  newYaml: string,
): Promise<DiffLine[]> {
  return invoke("diff_resource_yaml", { kind, namespace, name, newYaml });
}

// ── Phase 1: Deployment Rollback ──────────────────────────────────────

export interface DeploymentRevision {
  revision: number;
  name: string;
  created: string;
  image: string;
  change_cause: string;
  replicas: number;
}

export async function listDeploymentRevisions(
  namespace: string,
  name: string,
): Promise<DeploymentRevision[]> {
  return invoke("list_deployment_revisions", { namespace, name });
}

export async function getRevisionYaml(
  namespace: string,
  rsName: string,
): Promise<string> {
  return invoke("get_revision_yaml", { namespace, rsName });
}

export async function rollbackDeployment(
  namespace: string,
  deploymentName: string,
  rsName: string,
): Promise<void> {
  return invoke("rollback_deployment", { namespace, deploymentName, rsName });
}

// ── Phase 2: Cluster Dashboard ────────────────────────────────────────

export interface PodHealth {
  running: number;
  pending: number;
  failed: number;
  succeeded: number;
  crash_looping: number;
  total: number;
}

export interface NodeHealth {
  name: string;
  status: string;
  cpu_capacity: string;
  memory_capacity: string;
  cpu_usage: string;
  memory_usage: string;
}

export interface RestartHotItem {
  name: string;
  namespace: string;
  restarts: number;
  status: string;
}

export interface PendingPod {
  name: string;
  namespace: string;
  reason: string;
  age: string;
}

export interface WarningEvent {
  reason: string;
  message: string;
  involved_object: string;
  namespace: string;
  count: number;
  last_seen: string;
}

export interface ClusterHealth {
  score: number;
  pods: PodHealth;
  nodes: NodeHealth[];
  restart_hotlist: RestartHotItem[];
  pending_pods: PendingPod[];
  recent_warnings: WarningEvent[];
}

export async function getClusterHealth(): Promise<ClusterHealth> {
  return invoke("get_cluster_health");
}

// ── Phase 2: Event Store ──────────────────────────────────────────────

export interface StoredEvent {
  uid: string;
  kind: string;
  name: string;
  namespace: string;
  reason: string;
  message: string;
  event_type: string;
  involved_object: string;
  count: number;
  first_seen: string;
  last_seen: string;
  context: string;
}

export async function queryStoredEvents(
  since: string,
  until: string,
  namespace?: string,
): Promise<StoredEvent[]> {
  return invoke("query_stored_events", { since, until, namespace });
}

// ── Phase 2: Multi-Pod Logs ───────────────────────────────────────────

export async function streamMultiPodLogs(
  namespace: string,
  labelSelector: string,
  tailLines?: number,
): Promise<void> {
  return invoke("stream_multi_pod_logs", { namespace, labelSelector, tailLines });
}

// ── Phase 3: CRD Browser ─────────────────────────────────────────────

export interface CrdInfo {
  name: string;
  group: string;
  version: string;
  kind: string;
  scope: string;
  plural: string;
}

export async function listCrds(): Promise<CrdInfo[]> {
  return invoke("list_crds");
}

export async function listCrdInstances(
  group: string,
  version: string,
  plural: string,
  namespace?: string,
): Promise<Record<string, unknown>[]> {
  return invoke("list_crd_instances", { group, version, plural, namespace });
}

export async function getCrdInstance(
  group: string,
  version: string,
  plural: string,
  namespace: string,
  name: string,
): Promise<Record<string, unknown>> {
  return invoke("get_crd_instance", { group, version, plural, namespace, name });
}

export async function deleteCrdInstance(
  group: string,
  version: string,
  plural: string,
  namespace: string,
  name: string,
): Promise<void> {
  return invoke("delete_crd_instance", { group, version, plural, namespace, name });
}

// ── Phase 3: Resource Graph ───────────────────────────────────────────

export interface GraphNode {
  id: string;
  name: string;
  namespace: string;
  kind: string;
  status: string;
}

export interface GraphEdge {
  source: string;
  target: string;
  relation: string;
}

export interface ResourceGraph {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

export async function buildResourceGraph(namespace?: string): Promise<ResourceGraph> {
  return invoke("build_resource_graph", { namespace });
}

// ── Phase 4: Helm Management ──────────────────────────────────────────

export interface HelmRelease {
  name: string;
  namespace: string;
  revision: string;
  updated: string;
  status: string;
  chart: string;
  app_version: string;
}

export interface HelmRevision {
  revision: string;
  updated: string;
  status: string;
  chart: string;
  app_version: string;
  description: string;
}

export async function helmAvailable(): Promise<boolean> {
  return invoke("helm_available");
}

export async function listHelmReleases(namespace?: string): Promise<HelmRelease[]> {
  return invoke("list_helm_releases", { namespace });
}

export async function getHelmValues(release: string, namespace: string): Promise<string> {
  return invoke("get_helm_values", { release, namespace });
}

export async function getHelmManifest(release: string, namespace: string): Promise<string> {
  return invoke("get_helm_manifest", { release, namespace });
}

export async function getHelmHistory(release: string, namespace: string): Promise<HelmRevision[]> {
  return invoke("get_helm_history", { release, namespace });
}

export async function rollbackHelmRelease(
  release: string,
  namespace: string,
  revision: string,
): Promise<string> {
  return invoke("rollback_helm_release", { release, namespace, revision });
}

// ── Phase 4: Multi-Cluster ────────────────────────────────────────────

export async function listResourcesMultiCluster(
  kind: string,
  namespace?: string,
  labelSelector?: string,
): Promise<Record<string, unknown>[]> {
  return invoke("list_resources_multi_cluster", { kind, namespace, labelSelector });
}

// ── Phase 4: AI Troubleshooting ──────────────────────────────────────

export interface AIConfig {
  provider: "openai" | "anthropic" | "ollama";
  api_key?: string;
  model: string;
  base_url?: string;
}

export interface DiagnoseRequest {
  kind: string;
  namespace: string;
  name: string;
}

export async function aiDiagnose(
  config: AIConfig,
  request: DiagnoseRequest,
): Promise<void> {
  return invoke("ai_diagnose", { config, request });
}

export async function aiTestConnection(
  config: AIConfig,
): Promise<boolean> {
  return invoke("ai_test_connection", { config });
}
