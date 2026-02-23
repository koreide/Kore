import { useEffect, useState, useRef, useCallback, useMemo } from "react";
import { motion, AnimatePresence } from "framer-motion";
import {
  Copy,
  RotateCcw,
  Pencil,
  Lock,
  Save,
  GitCompareArrows,
  Loader2,
  X,
} from "lucide-react";
import { getResourceYaml, applyResourceYaml, diffResourceYaml } from "@/lib/api";
import type { DiffLine } from "@/lib/api";
import { formatError } from "@/lib/errors";
import { ConfirmDialog } from "./confirm-dialog";
import { useToast } from "./toast";
import { cn } from "@/lib/utils";

interface YamlEditorProps {
  kind: string;
  namespace: string;
  name: string;
}

function YamlSkeleton() {
  const widths = ["85%", "60%", "75%", "40%", "90%", "50%", "70%", "55%", "80%", "45%", "65%", "35%"];
  return (
    <div className="p-4 space-y-2.5">
      {widths.map((w, i) => (
        <div key={i} className="skeleton h-3" style={{ width: w, opacity: 1 - i * 0.06 }} />
      ))}
    </div>
  );
}

export function YamlEditor({ kind, namespace, name }: YamlEditorProps) {
  const [originalYaml, setOriginalYaml] = useState("");
  const [yaml, setYaml] = useState("");
  const [loading, setLoading] = useState(true);
  const [readOnly, setReadOnly] = useState(true);
  const [applying, setApplying] = useState(false);
  const [showApplyConfirm, setShowApplyConfirm] = useState(false);
  const [diffLines, setDiffLines] = useState<DiffLine[] | null>(null);
  const [diffLoading, setDiffLoading] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const lineNumbersRef = useRef<HTMLDivElement>(null);
  const toast = useToast();

  const hasChanges = yaml !== originalYaml;

  // Fetch YAML on mount or when resource identity changes
  useEffect(() => {
    setLoading(true);
    setDiffLines(null);
    setReadOnly(true);
    getResourceYaml(kind, namespace, name)
      .then((data) => {
        setOriginalYaml(data);
        setYaml(data);
      })
      .catch((err) => {
        toast(formatError(err), "error");
        setYaml(`# Error fetching YAML: ${formatError(err)}`);
      })
      .finally(() => setLoading(false));
  }, [kind, namespace, name]);

  // Sync line number scroll with textarea
  const handleScroll = useCallback(() => {
    if (textareaRef.current && lineNumbersRef.current) {
      lineNumbersRef.current.scrollTop = textareaRef.current.scrollTop;
    }
  }, []);

  const lineCount = useMemo(() => {
    return yaml.split("\n").length;
  }, [yaml]);

  const handleCopy = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(yaml);
      toast("YAML copied to clipboard", "success");
    } catch {
      toast("Failed to copy YAML", "error");
    }
  }, [yaml, toast]);

  const handleReset = useCallback(() => {
    setYaml(originalYaml);
    setDiffLines(null);
  }, [originalYaml]);

  const handleToggleEdit = useCallback(() => {
    if (!readOnly) {
      // Switching back to read-only -- reset changes
      setYaml(originalYaml);
      setDiffLines(null);
    }
    setReadOnly((prev) => !prev);
  }, [readOnly, originalYaml]);

  const handleDiff = useCallback(async () => {
    if (!hasChanges) return;
    setDiffLoading(true);
    try {
      const diff = await diffResourceYaml(kind, namespace, name, yaml);
      setDiffLines(diff);
    } catch (err) {
      toast(formatError(err), "error");
    } finally {
      setDiffLoading(false);
    }
  }, [kind, namespace, name, yaml, hasChanges, toast]);

  const handleApply = useCallback(async () => {
    setShowApplyConfirm(false);
    setApplying(true);
    try {
      const result = await applyResourceYaml(kind, namespace, name, yaml);
      toast(result || "YAML applied successfully", "success");
      setOriginalYaml(yaml);
      setDiffLines(null);
      setReadOnly(true);
    } catch (err) {
      toast(formatError(err), "error");
    } finally {
      setApplying(false);
    }
  }, [kind, namespace, name, yaml, toast]);

  // Handle Tab key inside textarea for indentation
  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (e.key === "Tab") {
        e.preventDefault();
        const ta = e.currentTarget;
        const start = ta.selectionStart;
        const end = ta.selectionEnd;
        const newValue = yaml.substring(0, start) + "  " + yaml.substring(end);
        setYaml(newValue);
        // Restore cursor position after React re-render
        requestAnimationFrame(() => {
          ta.selectionStart = start + 2;
          ta.selectionEnd = start + 2;
        });
      }
    },
    [yaml],
  );

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      className="h-full flex flex-col"
    >
      {/* Toolbar */}
      <div className="flex items-center justify-between px-4 py-2 border-b border-slate-800 bg-surface/50">
        <div className="flex items-center gap-1.5">
          <span className="text-[10px] uppercase tracking-wider text-slate-500 mr-2">YAML</span>
          <button
            onClick={handleToggleEdit}
            className={cn(
              "flex items-center gap-1.5 px-2.5 py-1.5 rounded-md border text-xs transition",
              readOnly
                ? "border-slate-800 text-slate-400 hover:border-accent/50 hover:text-accent"
                : "border-accent/50 text-accent bg-accent/10",
            )}
          >
            {readOnly ? <Lock className="w-3.5 h-3.5" /> : <Pencil className="w-3.5 h-3.5" />}
            {readOnly ? "Read-only" : "Editing"}
          </button>
        </div>

        <div className="flex items-center gap-1.5">
          {/* Diff button -- only when editing and changes exist */}
          {!readOnly && hasChanges && (
            <button
              onClick={handleDiff}
              disabled={diffLoading}
              className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-md border border-slate-800 hover:border-accent/50 hover:bg-muted/30 transition text-xs text-slate-300 disabled:opacity-50"
            >
              {diffLoading ? (
                <Loader2 className="w-3.5 h-3.5 animate-spin" />
              ) : (
                <GitCompareArrows className="w-3.5 h-3.5" />
              )}
              Diff
            </button>
          )}

          {/* Reset button -- only when changes exist */}
          {!readOnly && hasChanges && (
            <button
              onClick={handleReset}
              className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-md border border-slate-800 hover:border-amber-500/50 hover:bg-amber-500/10 transition text-xs text-slate-300"
            >
              <RotateCcw className="w-3.5 h-3.5" />
              Reset
            </button>
          )}

          {/* Copy */}
          <button
            onClick={handleCopy}
            className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-md border border-slate-800 hover:border-accent/50 hover:bg-muted/30 transition text-xs text-slate-300"
          >
            <Copy className="w-3.5 h-3.5" />
            Copy
          </button>

          {/* Apply button -- only when editing and changes exist */}
          {!readOnly && hasChanges && (
            <button
              onClick={() => setShowApplyConfirm(true)}
              disabled={applying}
              className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-md border border-emerald-700/50 bg-emerald-500/10 hover:bg-emerald-500/20 hover:border-emerald-500 transition text-xs text-emerald-400 disabled:opacity-50"
            >
              {applying ? (
                <Loader2 className="w-3.5 h-3.5 animate-spin" />
              ) : (
                <Save className="w-3.5 h-3.5" />
              )}
              Apply
            </button>
          )}
        </div>
      </div>

      {/* Editor / Diff view */}
      <div className="flex-1 overflow-hidden relative">
        <AnimatePresence mode="wait">
          {loading ? (
            <motion.div
              key="skeleton"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              className="h-full bg-black/40 border border-slate-800 rounded-lg m-4"
            >
              <YamlSkeleton />
            </motion.div>
          ) : diffLines ? (
            /* Diff view */
            <motion.div
              key="diff"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              className="h-full overflow-auto bg-black/40 border border-slate-800 rounded-lg m-4"
            >
              {/* Diff header */}
              <div className="flex items-center justify-between px-4 py-2 border-b border-slate-800/50">
                <span className="text-xs text-slate-400">
                  Diff: {diffLines.filter((l) => l.tag === "insert").length} additions,{" "}
                  {diffLines.filter((l) => l.tag === "delete").length} deletions
                </span>
                <button
                  onClick={() => setDiffLines(null)}
                  className="flex items-center gap-1 text-xs text-slate-500 hover:text-slate-300 transition"
                >
                  <X className="w-3.5 h-3.5" />
                  Close diff
                </button>
              </div>

              <div className="font-mono text-xs leading-relaxed">
                {diffLines.map((line, i) => (
                  <div
                    key={i}
                    className={cn(
                      "flex group",
                      line.tag === "insert" && "bg-emerald-500/10",
                      line.tag === "delete" && "bg-red-500/10",
                    )}
                  >
                    <span
                      className={cn(
                        "select-none text-right w-12 shrink-0 pr-3 py-px border-r border-slate-800/50",
                        line.tag === "insert"
                          ? "text-emerald-600"
                          : line.tag === "delete"
                            ? "text-red-600"
                            : "text-slate-600",
                      )}
                    >
                      {i + 1}
                    </span>
                    <span
                      className={cn(
                        "select-none w-5 shrink-0 text-center py-px",
                        line.tag === "insert"
                          ? "text-emerald-400"
                          : line.tag === "delete"
                            ? "text-red-400"
                            : "text-slate-700",
                      )}
                    >
                      {line.tag === "insert" ? "+" : line.tag === "delete" ? "-" : " "}
                    </span>
                    <span
                      className={cn(
                        "pl-2 py-px whitespace-pre-wrap break-all flex-1",
                        line.tag === "insert"
                          ? "text-emerald-300"
                          : line.tag === "delete"
                            ? "text-red-300"
                            : "text-slate-400",
                      )}
                    >
                      {line.value}
                    </span>
                  </div>
                ))}
              </div>
            </motion.div>
          ) : (
            /* Editor view */
            <motion.div
              key="editor"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              className="h-full flex bg-black/40 border border-slate-800 rounded-lg m-4 overflow-hidden"
            >
              {/* Line numbers */}
              <div
                ref={lineNumbersRef}
                className="select-none text-right text-slate-600 font-mono text-xs leading-relaxed py-4 pr-3 border-r border-slate-800/50 overflow-hidden shrink-0"
                aria-hidden="true"
              >
                {Array.from({ length: lineCount }, (_, i) => (
                  <div key={i} className="px-2 h-[1.625rem]">
                    {i + 1}
                  </div>
                ))}
              </div>

              {/* Textarea */}
              <textarea
                ref={textareaRef}
                value={yaml}
                onChange={(e) => setYaml(e.target.value)}
                onScroll={handleScroll}
                onKeyDown={handleKeyDown}
                readOnly={readOnly}
                spellCheck={false}
                className={cn(
                  "flex-1 bg-transparent text-slate-300 font-mono text-xs leading-relaxed p-4 resize-none outline-none",
                  "whitespace-pre overflow-auto",
                  readOnly && "cursor-default text-slate-400",
                  !readOnly && "caret-accent",
                )}
              />
            </motion.div>
          )}
        </AnimatePresence>

        {/* Change indicator */}
        {!readOnly && hasChanges && !diffLines && (
          <div className="absolute bottom-6 right-6">
            <span className="text-[10px] text-amber-400 bg-amber-500/10 border border-amber-500/30 rounded-full px-2.5 py-1">
              Unsaved changes
            </span>
          </div>
        )}
      </div>

      {/* Apply confirmation */}
      <ConfirmDialog
        open={showApplyConfirm}
        title="Apply YAML Changes"
        message={`Are you sure you want to apply the modified YAML to ${kind.replace(/s$/, "")} "${name}" in namespace "${namespace}"? This will update the live resource.`}
        confirmText="Apply"
        cancelText="Cancel"
        onConfirm={handleApply}
        onCancel={() => setShowApplyConfirm(false)}
        variant="warning"
      />
    </motion.div>
  );
}
