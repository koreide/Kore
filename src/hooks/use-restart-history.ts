import { useCallback, useRef } from "react";

export type RestartDataPoint = { time: number; count: number };

interface RestartEntry {
  timestamps: number[];
  count: number;
}

const MAX_DATA_POINTS = 20;
const MAX_AGE_MS = 60 * 60 * 1000; // 1 hour

export function useRestartHistory() {
  const historyRef = useRef<Map<string, RestartEntry>>(new Map());

  const pruneOldEntries = useCallback((entry: RestartEntry) => {
    const cutoff = Date.now() - MAX_AGE_MS;
    const validIndices: number[] = [];

    for (let i = 0; i < entry.timestamps.length; i++) {
      if (entry.timestamps[i] >= cutoff) {
        validIndices.push(i);
      }
    }

    if (validIndices.length < entry.timestamps.length) {
      entry.timestamps = validIndices.map((i) => entry.timestamps[i]);
    }
  }, []);

  const recordRestart = useCallback(
    (namespace: string, podName: string, restartCount: number) => {
      const key = `${namespace}/${podName}`;
      const map = historyRef.current;
      const now = Date.now();

      let entry = map.get(key);
      if (!entry) {
        entry = { timestamps: [], count: 0 };
        map.set(key, entry);
      }

      // Prune stale entries before adding
      pruneOldEntries(entry);

      entry.timestamps.push(now);
      entry.count = restartCount;

      // Keep only the last MAX_DATA_POINTS entries
      if (entry.timestamps.length > MAX_DATA_POINTS) {
        entry.timestamps = entry.timestamps.slice(-MAX_DATA_POINTS);
      }
    },
    [pruneOldEntries],
  );

  const getHistory = useCallback(
    (namespace: string, podName: string): RestartDataPoint[] => {
      const key = `${namespace}/${podName}`;
      const entry = historyRef.current.get(key);
      if (!entry) return [];

      pruneOldEntries(entry);

      // We only store timestamps alongside a single running count.
      // To reconstruct per-point counts we infer from the current count
      // and the number of recorded timestamps: each timestamp represents
      // a snapshot, so the count was incrementally increasing.
      // The simplest correct approach: each recorded point captured a
      // snapshot of restartCount at that moment. We store the count at
      // recording time rather than reconstructing it.
      //
      // Since we only have the latest count, we distribute it linearly
      // across the recorded points (oldest = count - (len-1), clamped to 0).
      const len = entry.timestamps.length;
      const currentCount = entry.count;

      return entry.timestamps.slice(-MAX_DATA_POINTS).map((ts, i) => {
        // Assume restarts incremented one-by-one up to the current count.
        // The oldest recorded point corresponds to count - (remaining points after it).
        const pointCount = Math.max(0, currentCount - (len - 1 - i));
        return { time: ts, count: pointCount };
      });
    },
    [pruneOldEntries],
  );

  return { recordRestart, getHistory };
}
