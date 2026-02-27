import { useEffect, useState, useRef, useCallback, useMemo } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { Copy, RotateCcw, Pencil, Lock, Save, GitCompareArrows, Loader2, X } from "lucide-react";
import { getResourceYaml, applyResourceYaml, diffResourceYaml } from "@/lib/api";
import type { DiffLine } from "@/lib/api";
import { formatError } from "@/lib/errors";
import { ConfirmDialog } from "./confirm-dialog";
import { TextSearchBar } from "./text-search-bar";
import { useToast } from "./toast";
import { cn } from "@/lib/utils";

interface YamlEditorProps {
  kind: string;
  namespace: string;
  name: string;
}

function YamlSkeleton() {
  const widths = [
    "85%",
    "60%",
    "75%",
    "40%",
    "90%",
    "50%",
    "70%",
    "55%",
    "80%",
    "45%",
    "65%",
    "35%",
  ];
  return (
    <div className="p-4 space-y-2.5">
      {widths.map((w, i) => (
        <div key={i} className="skeleton h-3" style={{ width: w, opacity: 1 - i * 0.06 }} />
      ))}
    </div>
  );
}

function renderYamlHighlights(
  text: string,
  query: string,
  currentIndex: number,
  matches: number[],
): React.ReactNode[] {
  if (matches.length === 0) return [text];

  const parts: React.ReactNode[] = [];
  let lastEnd = 0;

  matches.forEach((pos, i) => {
    if (pos > lastEnd) {
      parts.push(text.substring(lastEnd, pos));
    }
    const isCurrent = i === currentIndex;
    parts.push(
      <mark
        key={i}
        style={{
          color: "transparent",
          background: isCurrent ? "rgba(251,191,36,0.35)" : "rgba(251,191,36,0.15)",
          boxShadow: isCurrent ? "0 0 0 1px rgba(251,191,36,0.5)" : "none",
          borderRadius: "2px",
        }}
        data-current-match={isCurrent || undefined}
      >
        {text.substring(pos, pos + query.length)}
      </mark>,
    );
    lastEnd = pos + query.length;
  });

  if (lastEnd < text.length) {
    parts.push(text.substring(lastEnd));
  }

  return parts;
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
  const highlightOverlayRef = useRef<HTMLPreElement>(null);
  const toast = useToast();

  // Search state
  const [searchVisible, setSearchVisible] = useState(false);
  const [searchInput, setSearchInput] = useState("");
  const [searchQuery, setSearchQuery] = useState("");
  const [searchIndex, setSearchIndex] = useState(0);

  // Debounce search input to avoid expensive highlighting on every keystroke
  useEffect(() => {
    const timer = setTimeout(() => setSearchQuery(searchInput), 200);
    return () => clearTimeout(timer);
  }, [searchInput]);

  const hasChanges = yaml !== originalYaml;

  // Find match positions in yaml text
  const searchMatches = useMemo(() => {
    if (!searchQuery) return [];
    const matches: number[] = [];
    const q = searchQuery.toLowerCase();
    const text = yaml.toLowerCase();
    let pos = 0;
    for (;;) {
      const idx = text.indexOf(q, pos);
      if (idx === -1) break;
      matches.push(idx);
      pos = idx + 1;
    }
    return matches;
  }, [yaml, searchQuery]);

  // Fetch YAML on mount or when resource identity changes
  useEffect(() => {
    setLoading(true);
    setDiffLines(null);
    setReadOnly(true);
    setSearchVisible(false);
    setSearchInput("");
    setSearchQuery("");
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
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [kind, namespace, name]);

  // Sync line number + overlay scroll with textarea
  const handleScroll = useCallback(() => {
    if (textareaRef.current) {
      if (lineNumbersRef.current) {
        lineNumbersRef.current.scrollTop = textareaRef.current.scrollTop;
      }
      if (highlightOverlayRef.current) {
        const { scrollTop, scrollLeft } = textareaRef.current;
        highlightOverlayRef.current.style.transform = `translate(${-scrollLeft}px, ${-scrollTop}px)`;
      }
    }
  }, []);

  // Scroll textarea to a character position
  const scrollToPosition = useCallback(
    (pos: number) => {
      if (!textareaRef.current) return;
      const ta = textareaRef.current;
      const linesBefore = yaml.substring(0, pos).split("\n").length - 1;
      const totalLines = Math.max(yaml.split("\n").length, 1);
      const lineHeight = ta.scrollHeight / totalLines;
      ta.scrollTop = Math.max(0, linesBefore * lineHeight - ta.clientHeight / 3);
    },
    [yaml],
  );

  // Scroll to current match when index changes
  useEffect(() => {
    if (searchMatches.length > 0) {
      scrollToPosition(searchMatches[searchIndex]);
    }
  }, [searchIndex, searchMatches, scrollToPosition]);

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

  // Cmd+F handler
  useEffect(() => {
    const handleCmdF = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "f") {
        e.preventDefault();
        setSearchVisible(true);
      }
    };
    window.addEventListener("keydown", handleCmdF);
    return () => window.removeEventListener("keydown", handleCmdF);
  }, []);

  // Search navigation
  const handleSearchNext = useCallback(() => {
    if (searchMatches.length === 0) return;
    setSearchIndex((prev) => (prev + 1) % searchMatches.length);
    if (diffLines) setDiffLines(null);
  }, [searchMatches, diffLines]);

  const handleSearchPrev = useCallback(() => {
    if (searchMatches.length === 0) return;
    setSearchIndex((prev) => (prev - 1 + searchMatches.length) % searchMatches.length);
    if (diffLines) setDiffLines(null);
  }, [searchMatches, diffLines]);

  const handleSearchClose = useCallback(() => {
    setSearchVisible(false);
    setSearchInput("");
    setSearchQuery("");
    setSearchIndex(0);
  }, []);

  const handleSearchQueryChange = useCallback((q: string) => {
    setSearchInput(q);
    setSearchIndex(0);
  }, []);

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
        {/* Search bar */}
        {searchVisible && (
          <TextSearchBar
            query={searchInput}
            onQueryChange={handleSearchQueryChange}
            matchCount={searchMatches.length}
            currentMatch={searchIndex}
            onNext={handleSearchNext}
            onPrev={handleSearchPrev}
            onClose={handleSearchClose}
          />
        )}

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

              {/* Textarea with search highlight overlay */}
              <div className="relative flex-1 overflow-hidden">
                {/* Search highlight overlay */}
                {searchVisible && searchQuery && searchMatches.length > 0 && (
                  <div
                    className="absolute inset-0 overflow-hidden pointer-events-none"
                    aria-hidden="true"
                  >
                    <pre
                      ref={highlightOverlayRef}
                      className="font-mono text-xs leading-relaxed p-4 whitespace-pre text-transparent m-0"
                    >
                      {renderYamlHighlights(yaml, searchQuery, searchIndex, searchMatches)}
                    </pre>
                  </div>
                )}

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
                    "absolute inset-0 w-full h-full bg-transparent text-slate-300 font-mono text-xs leading-relaxed p-4 resize-none outline-none",
                    "whitespace-pre overflow-auto",
                    readOnly && "cursor-default text-slate-400",
                    !readOnly && "caret-accent",
                  )}
                />
              </div>
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
