import { useEffect } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { X, Keyboard } from "lucide-react";
import { cn } from "@/lib/utils";

interface ShortcutOverlayProps {
  open: boolean;
  onClose: () => void;
}

interface ShortcutItem {
  keys: string[];
  description: string;
}

interface ShortcutGroup {
  category: string;
  items: ShortcutItem[];
}

const SHORTCUT_GROUPS: ShortcutGroup[] = [
  {
    category: "Navigation",
    items: [
      { keys: ["j"], description: "Move down" },
      { keys: ["k"], description: "Move up" },
      { keys: ["l"], description: "Enter detail view" },
      { keys: ["h"], description: "Go back" },
      { keys: ["1", "2", "3", "4", "5"], description: "Switch tabs" },
    ],
  },
  {
    category: "Actions",
    items: [
      { keys: ["d"], description: "Delete resource" },
      { keys: ["\u2318", "K"], description: "Command palette" },
      { keys: ["\u2318", "R"], description: "Refresh resources" },
      { keys: ["/"], description: "Search / filter" },
      { keys: ["\u2318", "F"], description: "Search logs" },
    ],
  },
  {
    category: "General",
    items: [
      { keys: ["Esc"], description: "Close / go back" },
      { keys: ["Enter"], description: "Select / confirm" },
      { keys: ["?"], description: "Toggle this overlay" },
    ],
  },
];

function Kbd({ children, className }: { children: React.ReactNode; className?: string }) {
  return (
    <kbd
      className={cn(
        "inline-flex items-center justify-center min-w-[24px] h-6 px-1.5 bg-slate-800/80 rounded text-[11px] text-slate-300 font-mono border border-slate-700/50 shadow-[0_1px_0_0_rgba(51,65,85,0.5)]",
        className,
      )}
    >
      {children}
    </kbd>
  );
}

function ShortcutRow({ item }: { item: ShortcutItem }) {
  return (
    <div className="flex items-center justify-between py-1.5 px-2 rounded hover:bg-white/[0.02] transition">
      <span className="text-xs text-slate-400">{item.description}</span>
      <div className="flex items-center gap-0.5 ml-4 shrink-0">
        {item.keys.map((key, i) => (
          <span key={i} className="flex items-center">
            {i > 0 && key.length > 1 && (
              <span className="text-slate-700 text-[10px] mx-0.5">+</span>
            )}
            <Kbd>{key}</Kbd>
          </span>
        ))}
      </div>
    </div>
  );
}

export function ShortcutOverlay({ open, onClose }: ShortcutOverlayProps) {
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape" || e.key === "?") {
        e.preventDefault();
        onClose();
      }
    };

    if (open) {
      window.addEventListener("keydown", handleKeyDown);
      return () => window.removeEventListener("keydown", handleKeyDown);
    }
  }, [open, onClose]);

  return (
    <AnimatePresence>
      {open && (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.15 }}
          className="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-start justify-center pt-[15vh] z-50"
          onClick={onClose}
        >
          <motion.div
            initial={{ opacity: 0, y: -20, scale: 0.96 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: -20, scale: 0.96 }}
            transition={{ type: "spring", stiffness: 400, damping: 30 }}
            onClick={(e) => e.stopPropagation()}
            className="w-full max-w-lg glass rounded-xl shadow-2xl overflow-hidden"
          >
            {/* Header */}
            <div className="flex items-center justify-between px-5 py-3.5 border-b border-slate-800/50">
              <div className="flex items-center gap-2.5">
                <div className="w-7 h-7 rounded-lg bg-accent/15 flex items-center justify-center">
                  <Keyboard className="w-4 h-4 text-accent" />
                </div>
                <div>
                  <h2 className="text-sm font-semibold text-slate-100">Keyboard Shortcuts</h2>
                  <p className="text-[10px] text-slate-500">Quick reference for all shortcuts</p>
                </div>
              </div>
              <button
                onClick={onClose}
                className="p-1.5 rounded-md hover:bg-slate-800/50 transition text-slate-400 hover:text-slate-200"
              >
                <X className="w-4 h-4" />
              </button>
            </div>

            {/* Shortcut grid */}
            <div className="grid grid-cols-2 gap-px bg-slate-800/30">
              {SHORTCUT_GROUPS.map((group, groupIdx) => (
                <div
                  key={group.category}
                  className={cn(
                    "p-4",
                    // Last group spans full width if odd number
                    groupIdx === SHORTCUT_GROUPS.length - 1 &&
                      SHORTCUT_GROUPS.length % 2 !== 0 &&
                      "col-span-2",
                  )}
                >
                  <p className="text-[10px] uppercase tracking-wider text-accent/70 mb-2 font-medium">
                    {group.category}
                  </p>
                  <div className="space-y-0.5">
                    {group.items.map((item) => (
                      <ShortcutRow key={item.description} item={item} />
                    ))}
                  </div>
                </div>
              ))}
            </div>

            {/* Footer */}
            <div className="px-5 py-2.5 border-t border-slate-800/50 flex items-center justify-center">
              <span className="text-[10px] text-slate-600 flex items-center gap-1.5">
                Press <Kbd className="text-[9px]">?</Kbd> or <Kbd className="text-[9px]">Esc</Kbd>{" "}
                to close
              </span>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
