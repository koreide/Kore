import { useCallback, useEffect, useRef, useState } from "react";
import {
  buildNetworkPolicyGraph,
  simulateNetworkTraffic,
} from "@/lib/api";
import type {
  NetworkPolicyGraph,
  TrafficSimulationResult,
} from "@/lib/api";
import { listen } from "@tauri-apps/api/event";
import type { WatchEventPayload } from "@/lib/types";

export function useNetworkPolicyGraph(
  namespace: string | undefined,
  currentContext: string | undefined,
) {
  const [graph, setGraph] = useState<NetworkPolicyGraph | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const fetchGraph = useCallback(async () => {
    try {
      setLoading(true);
      setError(null);
      const data = await buildNetworkPolicyGraph(namespace);
      setGraph(data);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, [namespace]);

  // Fetch on mount and when namespace/context changes
  useEffect(() => {
    fetchGraph();
  }, [fetchGraph, currentContext]);

  // Listen for pod watch events to trigger debounced refresh
  useEffect(() => {
    let unlistenFn: (() => void) | null = null;
    let isMounted = true;

    const setup = async () => {
      const unlisten = await listen<WatchEventPayload>("resource://event", (event) => {
        if (!isMounted) return;
        if (event.payload.kind === "pods") {
          if (debounceRef.current) clearTimeout(debounceRef.current);
          debounceRef.current = setTimeout(() => {
            fetchGraph();
          }, 2000);
        }
      });
      if (!isMounted) {
        unlisten();
        return;
      }
      unlistenFn = unlisten;
    };
    setup();

    return () => {
      isMounted = false;
      if (unlistenFn) unlistenFn();
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, [fetchGraph]);

  // Poll every 30s to catch NP changes (no dedicated watcher)
  useEffect(() => {
    const interval = setInterval(fetchGraph, 30000);
    return () => clearInterval(interval);
  }, [fetchGraph]);

  const simulate = useCallback(
    async (
      sourceNamespace: string,
      sourcePod: string,
      destNamespace: string,
      destPod: string,
      port?: number,
      protocol?: string,
    ): Promise<TrafficSimulationResult> => {
      return simulateNetworkTraffic(
        sourceNamespace,
        sourcePod,
        destNamespace,
        destPod,
        port,
        protocol,
      );
    },
    [],
  );

  return { graph, loading, error, refresh: fetchGraph, simulate };
}
