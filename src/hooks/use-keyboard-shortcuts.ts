import { useEffect, RefObject } from "react";

interface KeyboardShortcutsOptions {
  onTogglePalette: () => void;
  onRefresh: () => void;
  searchRef: RefObject<HTMLInputElement>;
}

export function useKeyboardShortcuts({
  onTogglePalette,
  onRefresh,
  searchRef,
}: KeyboardShortcutsOptions) {
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        onTogglePalette();
      }
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "r") {
        e.preventDefault();
        onRefresh();
      }
      if (e.key === "/") {
        e.preventDefault();
        searchRef.current?.focus();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onTogglePalette, onRefresh, searchRef]);
}
