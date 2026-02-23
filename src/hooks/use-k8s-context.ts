import { useCallback, useEffect, useState } from "react";
import { listContexts, listNamespaces, switchContext } from "@/lib/api";

const DEFAULT_NAMESPACES = ["default", "kube-system", "kube-public", "kube-node-lease"];

export function useK8sContext() {
  const [contexts, setContexts] = useState<string[]>([]);
  const [currentContext, setCurrentContext] = useState<string>();
  const [namespaces, setNamespaces] = useState<string[]>(DEFAULT_NAMESPACES);
  const [namespace, setNamespace] = useState<string>("default");

  const handleContextChange = useCallback(async (newContext: string) => {
    setNamespaces([]);

    try {
      await switchContext(newContext);
      setCurrentContext(newContext);
    } catch (err) {
      console.error("Failed to switch context", err);
    }
  }, []);

  // Load contexts on mount
  useEffect(() => {
    let cancelled = false;
    listContexts()
      .then(async (ctxs) => {
        if (cancelled) return;
        if (ctxs && ctxs.length > 0) {
          setContexts(ctxs);
          await handleContextChange(ctxs[0]);
        } else {
          setContexts([]);
        }
      })
      .catch((err) => {
        if (cancelled) return;
        console.error("Failed to load contexts", err);
        setContexts([]);
      });
    return () => {
      cancelled = true;
    };
  }, [handleContextChange]);

  // Fetch namespaces when context changes — with race condition protection
  useEffect(() => {
    if (!currentContext) return;
    let cancelled = false;

    setNamespaces([]);

    listNamespaces()
      .then((ns) => {
        if (cancelled) return;
        if (ns && ns.length > 0) {
          setNamespaces(ns);
          setNamespace((currentNs) => {
            if (currentNs === "*" || ns.includes(currentNs)) {
              return currentNs;
            }
            return "default";
          });
        } else {
          setNamespaces(DEFAULT_NAMESPACES);
        }
      })
      .catch((err) => {
        if (cancelled) return;
        console.error("Failed to load namespaces", err);
        setNamespaces(DEFAULT_NAMESPACES);
      });

    return () => {
      cancelled = true;
    };
  }, [currentContext]);

  const refreshNamespaces = useCallback(async () => {
    if (!currentContext) return;
    try {
      const ns = await listNamespaces();
      if (ns && ns.length > 0) {
        setNamespaces(ns);
      }
    } catch (err) {
      console.error("Failed to refresh namespaces", err);
    }
  }, [currentContext]);

  return {
    contexts,
    currentContext,
    namespaces,
    namespace,
    setNamespace,
    handleContextChange,
    refreshNamespaces,
  };
}
