import { useMemo, useEffect, useRef, useState, useCallback } from "react";
import {
  ColumnDef,
  getCoreRowModel,
  getSortedRowModel,
  SortingState,
  OnChangeFn,
  flexRender,
  useReactTable,
  Row,
} from "@tanstack/react-table";
import {
  ChevronUp,
  ChevronDown,
  ChevronsUpDown,
  Inbox,
  MoreHorizontal,
  Terminal,
  ScrollText,
  Trash2,
  ArrowUpDown,
  RefreshCw,
  Network,
  FileSearch,
  Star,
} from "lucide-react";
import { cn } from "@/lib/utils";
import type { ResourceItem, ResourceKind } from "@/lib/types";
import { RestartSparkline } from "./restart-sparkline";
import type { RestartDataPoint } from "@/hooks/use-restart-history";

interface ResourceTableProps {
  data: ResourceItem[];
  sorting: SortingState;
  onSortingChange: OnChangeFn<SortingState>;
  onRowSelect?: (row: ResourceItem) => void;
  kind?: ResourceKind;
  selectedRowIndex?: number;
  onSelectedRowIndexChange?: (index: number) => void;
  onRowAction?: (action: string, row: ResourceItem) => void;
  onTogglePin?: (kind: string, name: string, namespace: string) => void;
  isPinned?: (kind: string, name: string, namespace: string) => boolean;
  getRestartHistory?: (namespace: string, podName: string) => RestartDataPoint[];
  multiCluster?: boolean;
}

function formatRelativeAge(timestamp: string | undefined): string {
  if (!timestamp) return "-";

  try {
    const created = new Date(timestamp);
    const now = new Date();
    const diffMs = now.getTime() - created.getTime();
    const diffSeconds = Math.floor(diffMs / 1000);
    const diffMinutes = Math.floor(diffSeconds / 60);
    const diffHours = Math.floor(diffMinutes / 60);
    const diffDays = Math.floor(diffHours / 24);

    if (diffDays > 0) return `${diffDays}d`;
    if (diffHours > 0) return `${diffHours}h`;
    if (diffMinutes > 0) return `${diffMinutes}m`;
    return `${diffSeconds}s`;
  } catch {
    return "-";
  }
}

/** Shared age sorting function — deduplicated from 4 copies. */
function ageSortingFn(rowA: Row<ResourceItem>, rowB: Row<ResourceItem>): number {
  const timestampA = rowA.original.age;
  const timestampB = rowB.original.age;
  if (!timestampA && !timestampB) return 0;
  if (!timestampA) return 1;
  if (!timestampB) return -1;
  return new Date(timestampA).getTime() - new Date(timestampB).getTime();
}

type StatusVariant = "running" | "pending" | "failed" | "terminating" | "default";

function getStatusVariant(status: string | undefined): StatusVariant {
  if (!status) return "default";
  const s = status.toLowerCase();
  if (s === "running" || s === "ready" || s === "succeeded" || s === "completed" || s === "complete")
    return "running";
  if (s === "pending" || s === "containercreating" || s === "init" || s === "waiting")
    return "pending";
  if (
    s === "failed" ||
    s === "crashloopbackoff" ||
    s === "error" ||
    s === "imagepullbackoff" ||
    s === "evicted" ||
    s === "oomkilled"
  )
    return "failed";
  if (s === "terminating") return "terminating";
  return "default";
}

const statusStyles: Record<
  StatusVariant,
  { dot: string; bg: string; text: string; pulse?: boolean }
> = {
  running: {
    dot: "bg-emerald-400",
    bg: "bg-emerald-500/10",
    text: "text-emerald-400",
    pulse: true,
  },
  pending: { dot: "bg-amber-400", bg: "bg-amber-500/10", text: "text-amber-400" },
  failed: { dot: "bg-red-400", bg: "bg-red-500/10", text: "text-red-400" },
  terminating: { dot: "bg-orange-400", bg: "bg-orange-500/10", text: "text-orange-400" },
  default: { dot: "bg-slate-400", bg: "bg-slate-500/10", text: "text-slate-300" },
};

function StatusBadge({ status }: { status: string | undefined }) {
  const variant = getStatusVariant(status);
  const style = statusStyles[variant];
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
      {status || "-"}
    </span>
  );
}

function SortIcon({ sorted }: { sorted: false | "asc" | "desc" }) {
  if (sorted === "asc") return <ChevronUp className="w-3.5 h-3.5 text-accent" />;
  if (sorted === "desc") return <ChevronDown className="w-3.5 h-3.5 text-accent" />;
  return <ChevronsUpDown className="w-3.5 h-3.5 text-slate-600" />;
}

interface RowActionsProps {
  kind: ResourceKind;
  row: ResourceItem;
  onAction: (action: string, row: ResourceItem) => void;
}

function RowActions({ kind, row, onAction }: RowActionsProps) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const handleClick = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, [open]);

  const actions: { label: string; icon: typeof Terminal; action: string }[] = [];

  if (kind === "pods") {
    actions.push(
      { label: "View Logs", icon: ScrollText, action: "logs" },
      { label: "Exec Shell", icon: Terminal, action: "exec" },
      { label: "Port Forward", icon: Network, action: "port-forward" },
      { label: "Delete", icon: Trash2, action: "delete" },
    );
  } else if (kind === "deployments") {
    actions.push(
      { label: "Scale", icon: ArrowUpDown, action: "scale" },
      { label: "Restart", icon: RefreshCw, action: "restart" },
      { label: "Delete", icon: Trash2, action: "delete" },
    );
  } else {
    actions.push(
      { label: "Describe", icon: FileSearch, action: "describe" },
      { label: "Delete", icon: Trash2, action: "delete" },
    );
  }

  return (
    <div ref={ref} className="relative">
      <button
        onClick={(e) => {
          e.stopPropagation();
          setOpen(!open);
        }}
        className="p-1 rounded hover:bg-muted/40 transition text-slate-500 hover:text-slate-300"
      >
        <MoreHorizontal className="w-4 h-4" />
      </button>
      {open && (
        <div className="absolute right-0 top-full mt-1 z-50 min-w-[160px] bg-surface border border-slate-800 rounded-lg shadow-xl py-1 glass">
          {actions.map(({ label, icon: Icon, action }) => (
            <button
              key={action}
              onClick={(e) => {
                e.stopPropagation();
                setOpen(false);
                onAction(action, row);
              }}
              className={cn(
                "w-full px-3 py-1.5 text-left text-xs flex items-center gap-2 transition",
                action === "delete"
                  ? "text-red-400 hover:bg-red-500/10"
                  : "text-slate-300 hover:bg-muted/40",
              )}
            >
              <Icon className="w-3.5 h-3.5" />
              {label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

export function ResourceTable({
  data,
  sorting,
  onSortingChange,
  onRowSelect,
  kind,
  selectedRowIndex = -1,
  onSelectedRowIndexChange,
  onRowAction,
  onTogglePin,
  isPinned,
  getRestartHistory,
  multiCluster,
}: ResourceTableProps) {
  const tableRef = useRef<HTMLDivElement>(null);
  const [internalSelectedIndex, setInternalSelectedIndex] = useState(-1);

  const selectedIndex = selectedRowIndex >= 0 ? selectedRowIndex : internalSelectedIndex;
  const setSelectedIndex = onSelectedRowIndexChange || setInternalSelectedIndex;

  const handleAction = useCallback(
    (action: string, row: ResourceItem) => {
      if (onRowAction) {
        onRowAction(action, row);
      }
    },
    [onRowAction],
  );

  const columns = useMemo<ColumnDef<ResourceItem>[]>(() => {
    let cols: ColumnDef<ResourceItem>[];

    if (kind === "deployments") {
      cols = [
        { accessorKey: "namespace", header: "Namespace" },
        { accessorKey: "name", header: "Name" },
        { accessorKey: "ready", header: "Ready" },
        {
          accessorKey: "upToDate",
          header: "Up-to-date",
          cell: ({ getValue }) => getValue() ?? "-",
        },
        {
          accessorKey: "available",
          header: "Available",
          cell: ({ getValue }) => getValue() ?? "-",
        },
        {
          accessorKey: "age",
          header: "Age",
          cell: ({ getValue }) => formatRelativeAge(getValue() as string),
          sortingFn: ageSortingFn,
        },
      ];
    } else if (kind === "services") {
      cols = [
        { accessorKey: "namespace", header: "Namespace" },
        { accessorKey: "name", header: "Name" },
        {
          accessorKey: "type",
          header: "Type",
          cell: ({ getValue }) => getValue() ?? "-",
        },
        {
          accessorKey: "clusterIp",
          header: "Cluster-IP",
          cell: ({ getValue }) => getValue() ?? "-",
        },
        {
          accessorKey: "externalIp",
          header: "External-IP",
          cell: ({ getValue }) => getValue() ?? "-",
        },
        {
          accessorKey: "ports",
          header: "Port(s)",
          cell: ({ getValue }) => getValue() ?? "-",
        },
        {
          accessorKey: "age",
          header: "Age",
          cell: ({ getValue }) => formatRelativeAge(getValue() as string),
          sortingFn: ageSortingFn,
        },
      ];
    } else if (kind === "nodes") {
      cols = [
        { accessorKey: "name", header: "Name" },
        {
          accessorKey: "status",
          header: "Status",
          cell: ({ getValue }) => <StatusBadge status={getValue() as string} />,
        },
        {
          accessorKey: "roles",
          header: "Roles",
          cell: ({ getValue }) => getValue() ?? "-",
        },
        {
          accessorKey: "age",
          header: "Age",
          cell: ({ getValue }) => formatRelativeAge(getValue() as string),
          sortingFn: ageSortingFn,
        },
        {
          accessorKey: "version",
          header: "Version",
          cell: ({ getValue }) => getValue() ?? "-",
        },
      ];
    } else if (kind === "events") {
      cols = [
        {
          accessorKey: "lastSeen",
          header: "Last Seen",
          cell: ({ getValue }) => formatRelativeAge(getValue() as string),
          sortingFn: (rowA, rowB) => {
            const a = rowA.original.lastSeen;
            const b = rowB.original.lastSeen;
            if (!a && !b) return 0;
            if (!a) return 1;
            if (!b) return -1;
            return new Date(a).getTime() - new Date(b).getTime();
          },
        },
        {
          accessorKey: "eventType",
          header: "Type",
          cell: ({ getValue }) => {
            const t = getValue() as string;
            return (
              <span
                className={cn(
                  "text-xs",
                  t === "Warning" ? "text-amber-400" : "text-blue-400",
                )}
              >
                {t || "-"}
              </span>
            );
          },
        },
        {
          accessorKey: "reason",
          header: "Reason",
          cell: ({ getValue }) => getValue() ?? "-",
        },
        { accessorKey: "involvedObject", header: "Object" },
        {
          accessorKey: "message",
          header: "Message",
          cell: ({ getValue }) => (
            <span className="truncate max-w-[300px] block">{(getValue() as string) ?? "-"}</span>
          ),
        },
        {
          accessorKey: "count",
          header: "Count",
          cell: ({ getValue }) => getValue() ?? "-",
        },
      ];
    } else if (kind === "configmaps") {
      cols = [
        { accessorKey: "namespace", header: "Namespace" },
        { accessorKey: "name", header: "Name" },
        {
          accessorKey: "dataKeys",
          header: "Data Keys",
          cell: ({ getValue }) => getValue() ?? 0,
        },
        {
          accessorKey: "age",
          header: "Age",
          cell: ({ getValue }) => formatRelativeAge(getValue() as string),
          sortingFn: ageSortingFn,
        },
      ];
    } else if (kind === "secrets") {
      cols = [
        { accessorKey: "namespace", header: "Namespace" },
        { accessorKey: "name", header: "Name" },
        {
          accessorKey: "type",
          header: "Type",
          cell: ({ getValue }) => getValue() ?? "-",
        },
        {
          accessorKey: "dataKeys",
          header: "Data Keys",
          cell: ({ getValue }) => getValue() ?? 0,
        },
        {
          accessorKey: "age",
          header: "Age",
          cell: ({ getValue }) => formatRelativeAge(getValue() as string),
          sortingFn: ageSortingFn,
        },
      ];
    } else if (kind === "ingresses") {
      cols = [
        { accessorKey: "namespace", header: "Namespace" },
        { accessorKey: "name", header: "Name" },
        {
          accessorKey: "ingressClass",
          header: "Class",
          cell: ({ getValue }) => getValue() ?? "-",
        },
        {
          accessorKey: "hosts",
          header: "Hosts",
          cell: ({ getValue }) => getValue() ?? "-",
        },
        {
          accessorKey: "ports",
          header: "Ports",
          cell: ({ getValue }) => getValue() ?? "-",
        },
        {
          accessorKey: "age",
          header: "Age",
          cell: ({ getValue }) => formatRelativeAge(getValue() as string),
          sortingFn: ageSortingFn,
        },
      ];
    } else if (kind === "jobs") {
      cols = [
        { accessorKey: "namespace", header: "Namespace" },
        { accessorKey: "name", header: "Name" },
        {
          accessorKey: "status",
          header: "Status",
          cell: ({ getValue }) => <StatusBadge status={getValue() as string} />,
        },
        {
          accessorKey: "completions",
          header: "Completions",
          cell: ({ getValue }) => getValue() ?? "-",
        },
        {
          accessorKey: "duration",
          header: "Duration",
          cell: ({ getValue }) => getValue() ?? "-",
        },
        {
          accessorKey: "age",
          header: "Age",
          cell: ({ getValue }) => formatRelativeAge(getValue() as string),
          sortingFn: ageSortingFn,
        },
      ];
    } else if (kind === "cronjobs") {
      cols = [
        { accessorKey: "namespace", header: "Namespace" },
        { accessorKey: "name", header: "Name" },
        {
          accessorKey: "schedule",
          header: "Schedule",
          cell: ({ getValue }) => (
            <span className="font-mono">{(getValue() as string) ?? "-"}</span>
          ),
        },
        {
          accessorKey: "lastSchedule",
          header: "Last Schedule",
          cell: ({ getValue }) => formatRelativeAge(getValue() as string),
        },
        {
          accessorKey: "active",
          header: "Active",
          cell: ({ getValue }) => getValue() ?? 0,
        },
        {
          accessorKey: "age",
          header: "Age",
          cell: ({ getValue }) => formatRelativeAge(getValue() as string),
          sortingFn: ageSortingFn,
        },
      ];
    } else {
      // Pods (default)
      cols = [
        { accessorKey: "name", header: "Name" },
        { accessorKey: "namespace", header: "Namespace" },
        {
          accessorKey: "status",
          header: "Status",
          cell: ({ getValue }) => <StatusBadge status={getValue() as string} />,
        },
        { accessorKey: "ready", header: "Ready" },
        {
          accessorKey: "restarts",
          header: "Restarts",
          cell: ({ row }) => {
            const restarts = row.original.restarts ?? 0;
            if (getRestartHistory) {
              const history = getRestartHistory(
                row.original.namespace || "default",
                row.original.name,
              );
              if (history.length > 0) {
                return <RestartSparkline data={history} />;
              }
            }
            return <span className="font-mono text-xs tabular-nums">{restarts}</span>;
          },
        },
        {
          accessorKey: "age",
          header: "Age",
          cell: ({ getValue }) => formatRelativeAge(getValue() as string),
          sortingFn: ageSortingFn,
        },
      ];
    }

    // Add cluster column at the beginning if multi-cluster mode
    if (multiCluster) {
      cols.unshift({
        accessorKey: "_context",
        header: "Cluster",
        cell: ({ getValue }) => (
          <span className="font-mono text-xs text-accent/80 truncate max-w-[120px] block">
            {(getValue() as string) || "-"}
          </span>
        ),
      });
    }

    // Add pin column
    if (onTogglePin && kind) {
      cols.push({
        id: "pin",
        header: "",
        cell: ({ row }) => {
          const r = row.original;
          const pinned = isPinned?.(kind, r.name, r.namespace || "default") ?? false;
          return (
            <button
              onClick={(e) => {
                e.stopPropagation();
                onTogglePin(kind, r.name, r.namespace || "default");
              }}
              className={cn(
                "p-1 rounded transition",
                pinned
                  ? "text-amber-400 hover:text-amber-300"
                  : "text-slate-600 hover:text-slate-400",
              )}
              title={pinned ? "Unpin" : "Pin"}
            >
              <Star className={cn("w-3.5 h-3.5", pinned && "fill-current")} />
            </button>
          );
        },
        enableSorting: false,
      });
    }

    // Add row actions column if handler provided
    if (onRowAction && kind) {
      cols.push({
        id: "actions",
        header: "",
        cell: ({ row }) => (
          <RowActions kind={kind} row={row.original} onAction={handleAction} />
        ),
        enableSorting: false,
      });
    }

    return cols;
  }, [kind, onRowAction, handleAction, onTogglePin, isPinned, getRestartHistory, multiCluster]);

  const table = useReactTable({
    data,
    columns,
    state: { sorting },
    enableSortingRemoval: false,
    onSortingChange,
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
        if (next >= 0 && tableRef.current) {
          const rowElement = tableRef.current.querySelector(
            `[data-row-index="${next}"]`,
          ) as HTMLElement;
          rowElement?.scrollIntoView({ behavior: "smooth", block: "nearest" });
        }
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        const next = selectedIndex > 0 ? selectedIndex - 1 : 0;
        setSelectedIndex(next);
        if (next >= 0 && tableRef.current) {
          const rowElement = tableRef.current.querySelector(
            `[data-row-index="${next}"]`,
          ) as HTMLElement;
          rowElement?.scrollIntoView({ behavior: "smooth", block: "nearest" });
        }
      } else if (e.key === "Enter" && selectedIndex >= 0 && selectedIndex < rows.length) {
        e.preventDefault();
        onRowSelect?.(rows[selectedIndex].original);
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [rows, selectedIndex, setSelectedIndex, onRowSelect]);

  // Reset selection when data changes
  useEffect(() => {
    if (selectedIndex >= rows.length) {
      setSelectedIndex(Math.max(0, rows.length - 1));
    }
  }, [data.length, rows.length, selectedIndex, setSelectedIndex]);

  if (rows.length === 0) {
    return (
      <div className="w-full h-full flex flex-col items-center justify-center text-slate-400 gap-3">
        <Inbox className="w-12 h-12 text-slate-600" />
        <p className="text-sm font-medium">No resources found</p>
        <p className="text-xs text-slate-500">Try switching namespace or adjusting your search</p>
      </div>
    );
  }

  return (
    <div
      ref={tableRef}
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
                      sorted === "asc"
                        ? "ascending"
                        : sorted === "desc"
                          ? "descending"
                          : undefined
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
                onRowSelect?.(row.original);
              }}
              onDoubleClick={() => onRowSelect?.(row.original)}
            >
              {row.getVisibleCells().map((cell) => (
                <td key={cell.id} className="px-3 py-2 text-slate-100 font-mono text-xs">
                  {flexRender(cell.column.columnDef.cell, cell.getContext())}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
