import { useCallback, useEffect, useState } from "react";

export type PinnedResource = {
  kind: string;
  name: string;
  namespace: string;
};

const STORAGE_KEY = "kore-pinned-resources";
const MAX_PINNED = 50;

function loadPinned(): PinnedResource[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.slice(0, MAX_PINNED);
  } catch {
    return [];
  }
}

function savePinned(items: PinnedResource[]) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(items.slice(0, MAX_PINNED)));
}

function matches(a: PinnedResource, kind: string, name: string, namespace: string): boolean {
  return a.kind === kind && a.name === name && a.namespace === namespace;
}

export function usePinnedResources() {
  const [pinned, setPinned] = useState<PinnedResource[]>(loadPinned);

  // Sync to localStorage whenever pinned changes
  useEffect(() => {
    savePinned(pinned);
  }, [pinned]);

  const isPinned = useCallback(
    (kind: string, name: string, namespace: string): boolean => {
      return pinned.some((p) => matches(p, kind, name, namespace));
    },
    [pinned],
  );

  const addPin = useCallback((kind: string, name: string, namespace: string) => {
    setPinned((prev) => {
      if (prev.some((p) => matches(p, kind, name, namespace))) return prev;
      return [{ kind, name, namespace }, ...prev].slice(0, MAX_PINNED);
    });
  }, []);

  const removePin = useCallback((kind: string, name: string, namespace: string) => {
    setPinned((prev) => {
      const next = prev.filter((p) => !matches(p, kind, name, namespace));
      if (next.length === prev.length) return prev;
      return next;
    });
  }, []);

  const togglePin = useCallback(
    (kind: string, name: string, namespace: string) => {
      if (isPinned(kind, name, namespace)) {
        removePin(kind, name, namespace);
      } else {
        addPin(kind, name, namespace);
      }
    },
    [isPinned, addPin, removePin],
  );

  return { pinned, addPin, removePin, isPinned, togglePin };
}
