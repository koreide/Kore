import { useState, useEffect, useCallback } from "react";
import { checkForUpdates, performUpdate as apiPerformUpdate } from "@/lib/api";
import type { UpdateInfo } from "@/lib/types";

const STORAGE_KEY = "kore-update-check";
const CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000; // 24 hours

interface StoredCheck {
  timestamp: number;
  info: UpdateInfo | null;
}

export function useUpdateCheck() {
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [checking, setChecking] = useState(false);
  const [updating, setUpdating] = useState(false);
  const [updateError, setUpdateError] = useState<string | null>(null);
  const [updateSuccess, setUpdateSuccess] = useState<string | null>(null);

  const doCheck = useCallback(async () => {
    setChecking(true);
    try {
      const info = await checkForUpdates();
      setUpdateInfo(info);
      const stored: StoredCheck = { timestamp: Date.now(), info };
      localStorage.setItem(STORAGE_KEY, JSON.stringify(stored));
    } catch {
      // Silently fail — update checks are non-critical
    } finally {
      setChecking(false);
    }
  }, []);

  const doUpdate = useCallback(async () => {
    setUpdating(true);
    setUpdateError(null);
    setUpdateSuccess(null);
    try {
      const version = await apiPerformUpdate();
      setUpdateSuccess(version);
      // Clear cached check so it re-checks after restart
      localStorage.removeItem(STORAGE_KEY);
    } catch (e) {
      setUpdateError(typeof e === "string" ? e : (e as Error).message ?? "Update failed");
    } finally {
      setUpdating(false);
    }
  }, []);

  useEffect(() => {
    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      if (raw) {
        const stored: StoredCheck = JSON.parse(raw);
        if (Date.now() - stored.timestamp < CHECK_INTERVAL_MS && stored.info) {
          setUpdateInfo(stored.info);
          return;
        }
      }
    } catch {
      // ignore parse errors
    }
    doCheck();
  }, [doCheck]);

  return {
    updateAvailable: updateInfo?.has_update ?? false,
    latestVersion: updateInfo?.latest_version ?? null,
    currentVersion: updateInfo?.current_version ?? null,
    releaseUrl: updateInfo?.release_url ?? null,
    releaseNotes: updateInfo?.release_notes ?? null,
    checking,
    checkNow: doCheck,
    performUpdate: doUpdate,
    updating,
    updateError,
    updateSuccess,
  };
}
