import { useEffect, useState, useMemo } from "react";
import {
  ColumnDef,
  getCoreRowModel,
  getSortedRowModel,
  SortingState,
  flexRender,
  useReactTable,
} from "@tanstack/react-table";
import { ChevronUp, ChevronDown, ChevronsUpDown, AlertCircle, Package } from "lucide-react";
import { motion } from "framer-motion";
import { cn } from "@/lib/utils";
import { helmAvailable, listHelmReleases } from "@/lib/api";
import type { HelmRelease } from "@/lib/api";

interface HelmReleasesProps {
  namespace?: string;
  onSelectRelease: (release: { name: string; namespace: string }) => void;
}

type HelmStatusVariant =
  | "deployed"
  | "failed"
  | "pending"
  | "superseded"
  | "uninstalling"
  | "default";

function getHelmStatusVariant(status: string): HelmStatusVariant {
  const s = status.toLowerCase();
  if (s === "deployed") return "deployed";
  if (s === "failed") return "failed";
  if (s.startsWith("pending")) return "pending";
  if (s === "superseded") return "superseded";
  if (s === "uninstalling") return "uninstalling";
  return "default";
}

const helmStatusStyles: Record<
  HelmStatusVariant,
  { dot: string; bg: string; text: string; pulse?: boolean }
> = {
  deployed: {
    dot: "bg-emerald-400",
    bg: "bg-emerald-500/10",
    text: "text-emerald-400",
    pulse: true,
  },
  failed: { dot: "bg-red-400", bg: "bg-red-500/10", text: "text-red-400" },
  pending: { dot: "bg-amber-400", bg: "bg-amber-500/10", text: "text-amber-400" },
  superseded: { dot: "bg-slate-400", bg: "bg-slate-500/10", text: "text-slate-400" },
  uninstalling: { dot: "bg-orange-400", bg: "bg-orange-500/10", text: "text-orange-400" },
  default: { dot: "bg-slate-400", bg: "bg-slate-500/10", text: "text-slate-300" },
};

function HelmStatusBadge({ status }: { status: string }) {
  const variant = getHelmStatusVariant(status);
  const style = helmStatusStyles[variant];
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1.5 px-2 py-0.5 rounded-full text-xs font-medium",
        style.bg,
        style.text,
      )}
    >
      <span className="relative flex h-1.5 w-1.5">
        {style.pulse && (
          <span
            className={cn(
              "absolute inline-flex h-full w-full rounded-full opacity-75 animate-ping",
              style.dot,
            )}
          />
        )}
        <span className={cn("relative inline-flex rounded-full h-1.5 w-1.5", style.dot)} />
      </span>
      {status}
    </span>
  );
}

function SortIcon({ sorted }: { sorted: false | "asc" | "desc" }) {
  if (sorted === "asc") return <ChevronUp className="w-3.5 h-3.5 text-accent" />;
  if (sorted === "desc") return <ChevronDown className="w-3.5 h-3.5 text-accent" />;
  return <ChevronsUpDown className="w-3.5 h-3.5 text-slate-600" />;
}

function TableSkeleton() {
  return (
    <div className="p-4 space-y-3">
      {Array.from({ length: 8 }).map((_, i) => (
        <div key={i} className="flex gap-4">
          <div className="skeleton h-4 w-32" style={{ opacity: 1 - i * 0.08 }} />
          <div className="skeleton h-4 w-24" style={{ opacity: 1 - i * 0.08 }} />
          <div className="skeleton h-4 w-12" style={{ opacity: 1 - i * 0.08 }} />
          <div className="skeleton h-4 w-20" style={{ opacity: 1 - i * 0.08 }} />
          <div className="skeleton h-4 w-28" style={{ opacity: 1 - i * 0.08 }} />
          <div className="skeleton h-4 w-16" style={{ opacity: 1 - i * 0.08 }} />
          <div className="skeleton h-4 flex-1" style={{ opacity: 1 - i * 0.08 }} />
        </div>
      ))}
    </div>
  );
}

export function HelmReleases({ namespace, onSelectRelease }: HelmReleasesProps) {
  const [releases, setReleases] = useState<HelmRelease[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [helmReady, setHelmReady] = useState<boolean | null>(null);
  const [sorting, setSorting] = useState<SortingState>([{ id: "name", desc: false }]);
  const [selectedIndex, setSelectedIndex] = useState(-1);

  // Check helm availability
  useEffect(() => {
    helmAvailable()
      .then((available) => {
        setHelmReady(available);
        if (!available) setLoading(false);
      })
      .catch(() => {
        setHelmReady(false);
        setLoading(false);
      });
  }, []);

  // Fetch releases
  useEffect(() => {
    if (helmReady !== true) return;

    setLoading(true);
    setError(null);
    listHelmReleases(namespace)
      .then((data) => {
        setReleases(data);
        setLoading(false);
      })
      .catch((err) => {
        setError(typeof err === "string" ? err : String(err));
        setLoading(false);
      });
  }, [helmReady, namespace]);

  const columns = useMemo<ColumnDef<HelmRelease>[]>(
    () => [
      {
        accessorKey: "name",
        header: "Name",
        cell: ({ getValue }) => <span className="font-mono">{getValue() as string}</span>,
      },
      {
        accessorKey: "namespace",
        header: "Namespace",
        cell: ({ getValue }) => <span className="font-mono">{getValue() as string}</span>,
      },
      {
        accessorKey: "revision",
        header: "Revision",
      },
      {
        accessorKey: "status",
        header: "Status",
        cell: ({ getValue }) => <HelmStatusBadge status={getValue() as string} />,
      },
      {
        accessorKey: "chart",
        header: "Chart",
        cell: ({ getValue }) => (
          <span className="font-mono text-slate-300">{getValue() as string}</span>
        ),
      },
      {
        accessorKey: "app_version",
        header: "App Version",
        cell: ({ getValue }) => (
          <span className="font-mono text-slate-400">{(getValue() as string) || "-"}</span>
        ),
      },
      {
        accessorKey: "updated",
        header: "Updated",
        cell: ({ getValue }) => {
          const raw = getValue() as string;
          if (!raw) return "-";
          try {
            const date = new Date(raw);
            if (isNaN(date.getTime())) return raw;
            return date.toLocaleString(undefined, {
              month: "short",
              day: "numeric",
              hour: "2-digit",
              minute: "2-digit",
            });
          } catch {
            return raw;
          }
        },
      },
    ],
    [],
  );

  const table = useReactTable({
    data: releases,
    columns,
    state: { sorting },
    enableSortingRemoval: false,
    onSortingChange: setSorting,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
  });

  const rows = table.getRowModel().rows;

  // Keyboard navigation
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) {
        return;
      }

      if (e.key === "ArrowDown") {
        e.preventDefault();
        const next = selectedIndex < rows.length - 1 ? selectedIndex + 1 : selectedIndex;
        setSelectedIndex(next);
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        const next = selectedIndex > 0 ? selectedIndex - 1 : 0;
        setSelectedIndex(next);
      } else if (e.key === "Enter" && selectedIndex >= 0 && selectedIndex < rows.length) {
        e.preventDefault();
        const release = rows[selectedIndex].original;
        onSelectRelease({ name: release.name, namespace: release.namespace });
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [rows, selectedIndex, onSelectRelease]);

  // Helm not available
  if (helmReady === false) {
    return (
      <motion.div
        initial={{ opacity: 0, y: 10 }}
        animate={{ opacity: 1, y: 0 }}
        className="w-full h-full flex flex-col items-center justify-center text-slate-400 gap-4"
      >
        <div className="p-4 rounded-full bg-slate-800/50">
          <AlertCircle className="w-12 h-12 text-slate-500" />
        </div>
        <div className="text-center">
          <p className="text-sm font-medium text-slate-300 mb-1">Helm Not Available</p>
          <p className="text-xs text-slate-500 max-w-xs">
            The Helm CLI was not found on your system. Install Helm to manage releases.
          </p>
        </div>
      </motion.div>
    );
  }

  // Loading
  if (loading) {
    return (
      <div className="w-full h-full overflow-auto rounded-lg border border-slate-800 glass">
        <TableSkeleton />
      </div>
    );
  }

  // Error
  if (error) {
    return (
      <div className="w-full h-full flex flex-col items-center justify-center text-slate-400 gap-3">
        <AlertCircle className="w-12 h-12 text-red-400" />
        <p className="text-sm font-medium text-red-400">Failed to load Helm releases</p>
        <p className="text-xs text-slate-500 max-w-sm text-center">{error}</p>
      </div>
    );
  }

  // Empty state
  if (rows.length === 0) {
    return (
      <motion.div
        initial={{ opacity: 0, y: 10 }}
        animate={{ opacity: 1, y: 0 }}
        className="w-full h-full flex flex-col items-center justify-center text-slate-400 gap-3"
      >
        <div className="p-4 rounded-full bg-slate-800/50">
          <Package className="w-12 h-12 text-slate-600" />
        </div>
        <p className="text-sm font-medium">No Helm releases found</p>
        <p className="text-xs text-slate-500">
          {namespace
            ? `No releases in namespace "${namespace}"`
            : "Try selecting a namespace or deploying a chart"}
        </p>
      </motion.div>
    );
  }

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      className="w-full h-full overflow-auto rounded-lg border border-slate-800 glass"
    >
      <table className="min-w-full text-sm">
        <thead className="bg-surface/80 sticky top-0 z-10">
          {table.getHeaderGroups().map((headerGroup) => (
            <tr key={headerGroup.id} className="border-b border-slate-800">
              {headerGroup.headers.map((header) => {
                const sorted = header.column.getIsSorted();
                return (
                  <th
                    key={header.id}
                    className="px-3 py-2 text-left text-xs font-medium uppercase tracking-wider text-slate-400 select-none cursor-pointer hover:text-accent transition"
                    onClick={header.column.getToggleSortingHandler()}
                    aria-sort={
                      sorted === "asc" ? "ascending" : sorted === "desc" ? "descending" : undefined
                    }
                  >
                    <div className="flex items-center gap-1">
                      {flexRender(header.column.columnDef.header, header.getContext())}
                      {header.column.getCanSort() && <SortIcon sorted={sorted} />}
                    </div>
                  </th>
                );
              })}
            </tr>
          ))}
        </thead>
        <tbody className="divide-y divide-slate-800/50">
          {rows.map((row, index) => (
            <tr
              key={row.id}
              data-row-index={index}
              aria-selected={index === selectedIndex}
              className={cn(
                "transition cursor-pointer",
                index === selectedIndex
                  ? "bg-accent/10 border-l-2 border-l-accent"
                  : "hover:bg-muted/30",
              )}
              onClick={() => {
                setSelectedIndex(index);
                onSelectRelease({
                  name: row.original.name,
                  namespace: row.original.namespace,
                });
              }}
            >
              {row.getVisibleCells().map((cell) => (
                <td key={cell.id} className="px-3 py-2 text-slate-100 text-xs">
                  {flexRender(cell.column.columnDef.cell, cell.getContext())}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </motion.div>
  );
}
