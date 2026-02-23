import type {
  ContainerStatus,
  KubernetesObject,
  NodeInfo,
  PodCondition,
  ResourceItem,
} from "./types";

export function toResourceItem(obj: KubernetesObject): ResourceItem | null {
  const metadata = obj.metadata ?? {};
  const status = obj.status as Record<string, unknown> | undefined;
  const spec = obj.spec as Record<string, unknown> | undefined;
  const name = (metadata.name as string) ?? "";
  if (!name) return null;

  // Calculate ready status — for pods, count ready containers
  let ready: string | undefined;
  const containerStatuses = status?.containerStatuses as ContainerStatus[] | undefined;
  if (containerStatuses && Array.isArray(containerStatuses)) {
    const readyCount = containerStatuses.filter((cs) => cs.ready === true).length;
    const totalCount = containerStatuses.length;
    ready = `${readyCount}/${totalCount}`;
  } else {
    const readyReplicas = status?.readyReplicas as number | undefined;
    const replicas = status?.replicas as number | undefined;
    if (readyReplicas !== undefined && replicas !== undefined) {
      ready = `${readyReplicas}/${replicas}`;
    } else if (readyReplicas !== undefined) {
      ready = readyReplicas.toString();
    }
  }

  // Deployment-specific fields
  const upToDate = status?.updatedReplicas as number | undefined;
  const available = status?.availableReplicas as number | undefined;

  // Service-specific fields
  const svcType = spec?.type as string | undefined;
  const clusterIp = spec?.clusterIP as string | undefined;

  // External IP
  let externalIp: string | undefined;
  const loadBalancerIngress = status?.loadBalancer as Record<string, unknown> | undefined;
  const ingress = loadBalancerIngress?.ingress as Array<Record<string, string>> | undefined;
  if (ingress && Array.isArray(ingress) && ingress.length > 0) {
    externalIp = ingress[0]?.ip || ingress[0]?.hostname || undefined;
  }
  if (!externalIp) {
    const externalIPs = spec?.externalIPs as string[] | undefined;
    if (externalIPs && Array.isArray(externalIPs) && externalIPs.length > 0) {
      externalIp = externalIPs[0];
    }
  }
  if (!externalIp && svcType === "LoadBalancer") {
    externalIp = "<pending>";
  }
  if (!externalIp) {
    externalIp = "<none>";
  }

  // Format ports
  let ports: string | undefined;
  const specPorts = spec?.ports as Array<Record<string, unknown>> | undefined;
  if (specPorts && Array.isArray(specPorts)) {
    ports = specPorts
      .map((port) => {
        const portNum = port.port;
        const nodePort = port.nodePort;
        const protocol = port.protocol || "TCP";
        if (nodePort) {
          return `${nodePort}:${portNum}/${protocol}`;
        }
        return `${portNum}/${protocol}`;
      })
      .join(",");
  }

  // Node-specific fields
  let nodeStatus: string | undefined;
  const conditions = status?.conditions as PodCondition[] | undefined;
  if (conditions && Array.isArray(conditions)) {
    const readyCondition = conditions.find((c) => c.type === "Ready");
    if (readyCondition) {
      nodeStatus = readyCondition.status === "True" ? "Ready" : "NotReady";
    }
  }
  if (!nodeStatus) {
    nodeStatus = status?.phase as string | undefined;
  }

  // Roles from labels
  let roles: string | undefined;
  const labels = metadata.labels;
  if (labels) {
    const roleLabels: string[] = [];
    for (const [key, value] of Object.entries(labels)) {
      if (key.startsWith("node-role.kubernetes.io/")) {
        const role = key.replace("node-role.kubernetes.io/", "");
        roleLabels.push(role);
      } else if (key === "kubernetes.io/role") {
        roleLabels.push(value);
      }
    }
    if (roleLabels.length > 0) {
      roles = roleLabels.join(",");
    }
  }
  if (!roles) {
    roles = "<none>";
  }

  // Version from nodeInfo
  const nodeInfo = status?.nodeInfo as NodeInfo | undefined;
  const version = nodeInfo?.kubeletVersion;

  // Pod IP
  const podIp = status?.podIP as string | undefined;

  // Event-specific fields
  const reason = (obj as Record<string, unknown>).reason as string | undefined;
  const message = (obj as Record<string, unknown>).message as string | undefined;
  const count = (obj as Record<string, unknown>).count as number | undefined;
  const lastTimestamp = (obj as Record<string, unknown>).lastTimestamp as string | undefined;
  const eventType = (obj as Record<string, unknown>).type as string | undefined;
  const involvedObject = (obj as Record<string, unknown>).involvedObject as
    | Record<string, unknown>
    | undefined;
  const involvedName = involvedObject
    ? `${involvedObject.kind ?? ""}/${involvedObject.name ?? ""}`
    : undefined;

  // Job-specific fields
  const jobConditions = status?.conditions as Array<Record<string, unknown>> | undefined;
  let jobStatus: string | undefined;
  if (jobConditions && Array.isArray(jobConditions)) {
    const complete = jobConditions.find((c) => c.type === "Complete" && c.status === "True");
    const failed = jobConditions.find((c) => c.type === "Failed" && c.status === "True");
    if (complete) jobStatus = "Complete";
    else if (failed) jobStatus = "Failed";
  }

  const succeeded = status?.succeeded as number | undefined;
  const jobActive = status?.active as number | undefined;
  const specCompletions = spec?.completions as number | undefined;
  let completions: string | undefined;
  if (succeeded !== undefined || specCompletions !== undefined) {
    completions = `${succeeded ?? 0}/${specCompletions ?? 1}`;
  }

  const startTime = status?.startTime as string | undefined;
  const completionTime = status?.completionTime as string | undefined;
  let duration: string | undefined;
  if (startTime && completionTime) {
    const diffMs = new Date(completionTime).getTime() - new Date(startTime).getTime();
    const diffS = Math.floor(diffMs / 1000);
    if (diffS < 60) duration = `${diffS}s`;
    else if (diffS < 3600) duration = `${Math.floor(diffS / 60)}m${diffS % 60}s`;
    else duration = `${Math.floor(diffS / 3600)}h${Math.floor((diffS % 3600) / 60)}m`;
  }

  // CronJob-specific fields
  const schedule = spec?.schedule as string | undefined;
  const lastScheduleTime = status?.lastScheduleTime as string | undefined;
  const activeJobs = status?.active as Array<unknown> | undefined;
  const activeCount = Array.isArray(activeJobs) ? activeJobs.length : undefined;

  // ConfigMap/Secret — count data keys
  const data = (obj as Record<string, unknown>).data as Record<string, unknown> | undefined;
  const dataKeys = data ? Object.keys(data).length : undefined;

  // Ingress-specific fields
  const rules = spec?.rules as Array<Record<string, unknown>> | undefined;
  let hosts: string | undefined;
  if (rules && Array.isArray(rules)) {
    hosts = rules.map((r) => (r.host as string) || "*").join(", ");
  }
  const ingressClassName = spec?.ingressClassName as string | undefined;

  // Search result kind marker
  const _kind = (obj as Record<string, unknown>)._kind as string | undefined;
  // Multi-cluster context marker
  const _context = (obj as Record<string, unknown>)._context as string | undefined;

  return {
    name,
    namespace: (metadata.namespace as string) ?? "",
    status:
      jobStatus ??
      nodeStatus ??
      (status?.phase as string) ??
      (conditions as PodCondition[] | undefined)?.[0]?.type,
    age: metadata.creationTimestamp as string,
    ready,
    restarts: (containerStatuses as ContainerStatus[] | undefined)?.[0]?.restartCount,
    node: (status?.hostIP as string) ?? undefined,
    ip: podIp,
    upToDate,
    available,
    type: svcType,
    clusterIp,
    externalIp,
    ports,
    roles,
    version,
    // Event fields
    reason,
    message,
    count,
    lastSeen: lastTimestamp ?? (metadata.creationTimestamp as string),
    involvedObject: involvedName,
    eventType,
    // Job fields
    completions,
    duration,
    schedule,
    lastSchedule: lastScheduleTime,
    active: activeCount ?? jobActive,
    // ConfigMap/Secret fields
    dataKeys,
    // Ingress fields
    hosts,
    ingressClass: ingressClassName,
    // Search marker
    _kind,
    // Multi-cluster
    _context,
  };
}
