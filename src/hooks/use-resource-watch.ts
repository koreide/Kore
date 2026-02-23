import { useCallback, useEffect, useRef, useState } from "react";
import { listResources, listResourcesMultiCluster, startWatch } from "@/lib/api";
import { toResourceItem } from "@/lib/transforms";
import type { ResourceItem, ResourceKind, WatchEventPayload } from "@/lib/types";
import { listen } from "@tauri-apps/api/event";

export function useResourceWatch(
  currentContext: string | undefined,
  kind: ResourceKind,
  namespace: string,
  labelSelector?: string,
  multiCluster?: boolean,
) {
  const [resources, setResources] = useState<ResourceItem[]>([]);
  const [loading, setLoading] = useState(false);
  const currentKindRef = useRef<ResourceKind>(kind);
  const watchInitializedRef = useRef(false);

  useEffect(() => {
    currentKindRef.current = kind;
  }, [kind]);

  const refresh = useCallback(async () => {
    try {
      setLoading(true);
      setResources([]);
      watchInitializedRef.current = false;
      const apiNamespace = namespace === "*" ? undefined : namespace;
      const apiLabelSelector = labelSelector || undefined;

      // Small delay for event listener readiness
      await new Promise((resolve) => setTimeout(resolve, 500));

      let items: Record<string, unknown>[];
      if (multiCluster) {
        items = await listResourcesMultiCluster(kind, apiNamespace, apiLabelSelector);
      } else {
        items = await listResources(kind, apiNamespace, apiLabelSelector);
      }
      const converted = items
        .map((obj) => toResourceItem(obj as import("@/lib/types").KubernetesObject))
        .filter((item): item is ResourceItem => item !== null);

      setResources(converted);

      // Only start watch for single-cluster mode
      if (!multiCluster) {
        await new Promise((resolve) => setTimeout(resolve, 100));
        await startWatch(kind, apiNamespace, apiLabelSelector);
      }
    } catch (err) {
      console.error("Failed to refresh resources", err);
    } finally {
      setLoading(false);
    }
  }, [kind, namespace, labelSelector, multiCluster]);

  // Set up event listener once — uses ref for current kind
  useEffect(() => {
    let unlistenFn: (() => void) | null = null;
    let isMounted = true;

    const setupListener = async () => {
      const unlisten = await listen<WatchEventPayload>("resource://event", (event) => {
        if (!isMounted) return;

        const payload = event.payload;
        const currentKind = currentKindRef.current;

        if (payload.kind !== currentKind) return;

        if (!watchInitializedRef.current) {
          watchInitializedRef.current = true;
        }

        setResources((prevResources) => {
          const next = [...prevResources];
          const item = toResourceItem(payload.object);
          if (!item) return prevResources;

          const existing = next.findIndex(
            (r) => r.name === item.name && r.namespace === item.namespace,
          );
          if (payload.action === "deleted") {
            if (existing >= 0) {
              next.splice(existing, 1);
            }
          } else {
            if (existing >= 0) {
              next[existing] = item;
            } else {
              next.push(item);
            }
          }
          return next;
        });
      });

      // Fix: if component unmounted before listen resolved, immediately unlisten
      if (!isMounted) {
        unlisten();
        return;
      }
      unlistenFn = unlisten;
    };

    setupListener().catch((err) => {
      console.error("Failed to set up event listener:", err);
    });

    return () => {
      isMounted = false;
      if (unlistenFn) {
        unlistenFn();
      }
    };
  }, []);

  // Refresh when context, kind, namespace, or labelSelector changes
  useEffect(() => {
    if (currentContext) {
      setResources([]);
      watchInitializedRef.current = false;
      refresh();
    }
  }, [kind, namespace, labelSelector, refresh, currentContext]);

  const filtered = useCallback(
    (search: string) => {
      if (!search) return resources;
      const lower = search.toLowerCase();
      return resources.filter(
        (r) =>
          r.name.toLowerCase().includes(lower) ||
          (r.namespace && r.namespace.toLowerCase().includes(lower)) ||
          (r.status && r.status.toLowerCase().includes(lower)),
      );
    },
    [resources],
  );

  return { resources, loading, refresh, filtered };
}
