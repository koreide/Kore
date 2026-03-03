import { useState, useRef, useEffect } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { Copy } from "lucide-react";
import type { PermissionMatrix, PermissionCell, PermissionStatus } from "@/lib/api";
import { RbacRuleChain } from "@/components/rbac-rule-chain";

interface RbacMatrixProps {
  matrix: PermissionMatrix;
}

const VERBS = ["get", "list", "watch", "create", "update", "patch", "delete"];

function statusColor(status: PermissionStatus) {
  switch (status) {
    case "allowed":
      return {
        bg: "bg-emerald-500/20",
        ring: "ring-emerald-500/50",
        hover: "hover:bg-emerald-500/30",
      };
    case "denied":
      return {
        bg: "bg-red-500/20",
        ring: "ring-red-500/50",
        hover: "hover:bg-red-500/30",
      };
    case "conditional":
      return {
        bg: "bg-amber-500/20",
        ring: "ring-amber-500/50",
        hover: "hover:bg-amber-500/30",
      };
  }
}

function statusLabel(status: PermissionStatus) {
  switch (status) {
    case "allowed":
      return "Allowed";
    case "denied":
      return "Denied";
    case "conditional":
      return "Conditional";
  }
}

function CellPopover({
  cell,
  verb,
  resource,
  position,
  onClose,
}: {
  cell: PermissionCell;
  verb: string;
  resource: string;
  position: { top: number; left: number };
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    function handleClick(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        onClose();
      }
    }
    function handleKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    document.addEventListener("mousedown", handleClick);
    document.addEventListener("keydown", handleKey);
    return () => {
      document.removeEventListener("mousedown", handleClick);
      document.removeEventListener("keydown", handleKey);
    };
  }, [onClose]);

  return (
    <motion.div
      ref={ref}
      initial={{ opacity: 0, y: 4 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: 4 }}
      transition={{ duration: 0.15 }}
      className="fixed z-[60] w-[340px] max-h-[320px] overflow-y-auto glass rounded-lg border border-slate-700/50 shadow-xl p-3"
      style={{ top: position.top, left: position.left }}
    >
      <div className="flex items-center justify-between mb-2">
        <span className="text-xs font-mono text-slate-400">
          {resource} / {verb}
        </span>
        <span
          className={`text-[10px] px-1.5 py-0.5 rounded font-medium ${
            cell.status === "allowed"
              ? "bg-emerald-500/15 text-emerald-400"
              : cell.status === "denied"
                ? "bg-red-500/15 text-red-400"
                : "bg-amber-500/15 text-amber-400"
          }`}
        >
          {statusLabel(cell.status)}
        </span>
      </div>
      <RbacRuleChain chain={cell.rule_chain} allowed={cell.status !== "denied"} />
    </motion.div>
  );
}

export function RbacMatrix({ matrix }: RbacMatrixProps) {
  const [activeCell, setActiveCell] = useState<{
    resource: string;
    verb: string;
    cell: PermissionCell;
    position: { top: number; left: number };
  } | null>(null);

  function handleCellClick(
    resource: string,
    verb: string,
    cell: PermissionCell,
    e: React.MouseEvent,
  ) {
    const rect = (e.target as HTMLElement).getBoundingClientRect();
    setActiveCell({
      resource,
      verb,
      cell,
      position: {
        top: rect.bottom + 8,
        left: Math.min(rect.left, window.innerWidth - 360),
      },
    });
  }

  function handleCopyCSV() {
    const header = ["Resource", ...VERBS].join(",");
    const rows = matrix.rows.map((row) => {
      const cells = VERBS.map((v) => {
        const cell = row.verbs[v];
        return cell ? cell.status : "denied";
      });
      return [row.resource, ...cells].join(",");
    });
    navigator.clipboard.writeText([header, ...rows].join("\n"));
  }

  return (
    <div className="flex flex-col h-full gap-3">
      <div className="flex items-center justify-between shrink-0">
        <div className="flex items-center gap-4 text-[10px] text-slate-500">
          <div className="flex items-center gap-1.5">
            <div className="w-3 h-3 rounded-sm bg-emerald-500/20 border border-emerald-500/30" />
            Allowed
          </div>
          <div className="flex items-center gap-1.5">
            <div className="w-3 h-3 rounded-sm bg-red-500/20 border border-red-500/30" />
            Denied
          </div>
          <div className="flex items-center gap-1.5">
            <div className="w-3 h-3 rounded-sm bg-amber-500/20 border border-amber-500/30" />
            Conditional
          </div>
        </div>
        <button
          onClick={handleCopyCSV}
          className="flex items-center gap-1.5 px-2 py-1 rounded text-[10px] text-slate-500 hover:text-slate-300 hover:bg-slate-700/40 transition-colors"
        >
          <Copy className="w-3 h-3" />
          Copy CSV
        </button>
      </div>

      <div className="flex-1 min-h-0 overflow-auto rounded-lg border border-slate-800">
        <table className="w-full">
          <thead className="sticky top-0 z-20 bg-surface">
            <tr className="border-b border-slate-800">
              <th className="text-left px-3 py-2 text-[10px] uppercase tracking-wider text-slate-500 font-medium sticky left-0 bg-surface z-30 min-w-[180px]">
                Resource
              </th>
              {VERBS.map((verb) => (
                <th
                  key={verb}
                  className="px-2 py-2 text-center text-[10px] uppercase tracking-wider text-slate-500 font-medium min-w-[60px]"
                >
                  {verb}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {matrix.rows.map((row, i) => (
              <tr
                key={row.resource}
                className={`border-b border-slate-800/50 ${i % 2 === 1 ? "bg-white/[0.01]" : ""}`}
              >
                <td className="px-3 py-1.5 text-xs font-mono text-slate-300 sticky left-0 bg-surface z-10">
                  {row.resource}
                  {row.api_group && (
                    <span className="text-slate-600 ml-1 text-[10px]">{row.api_group}</span>
                  )}
                </td>
                {VERBS.map((verb) => {
                  const cell = row.verbs[verb];
                  if (!cell) {
                    return (
                      <td key={verb} className="px-2 py-1.5 text-center">
                        <div className="w-5 h-5 mx-auto rounded-sm bg-slate-800/50" />
                      </td>
                    );
                  }
                  const colors = statusColor(cell.status);
                  return (
                    <td key={verb} className="px-2 py-1.5 text-center">
                      <button
                        onClick={(e) => handleCellClick(row.resource, verb, cell, e)}
                        className={`w-5 h-5 mx-auto rounded-sm ${colors.bg} ${colors.hover} transition-all duration-150 hover:scale-125 hover:ring-1 ${colors.ring}`}
                        title={`${row.resource} ${verb}: ${statusLabel(cell.status)}`}
                      />
                    </td>
                  );
                })}
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <AnimatePresence>
        {activeCell && (
          <CellPopover
            cell={activeCell.cell}
            verb={activeCell.verb}
            resource={activeCell.resource}
            position={activeCell.position}
            onClose={() => setActiveCell(null)}
          />
        )}
      </AnimatePresence>
    </div>
  );
}
