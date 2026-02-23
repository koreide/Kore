import { useState, useRef, useCallback, useMemo } from "react";
import { motion } from "framer-motion";
import { ChevronDown, ChevronRight, Columns2 } from "lucide-react";
import { cn } from "@/lib/utils";

interface DiffViewerProps {
  leftYaml: string;
  rightYaml: string;
  leftTitle?: string;
  rightTitle?: string;
}

interface DiffLine {
  type: "unchanged" | "added" | "removed";
  leftLineNo: number | null;
  rightLineNo: number | null;
  leftContent: string;
  rightContent: string;
}

interface CollapsedSection {
  startIndex: number;
  count: number;
}

/** Simple LCS-based diff algorithm for line-by-line comparison */
function computeDiff(leftLines: string[], rightLines: string[]): DiffLine[] {
  const m = leftLines.length;
  const n = rightLines.length;

  // Build LCS table
  const dp: number[][] = Array.from({ length: m + 1 }, () =>
    Array(n + 1).fill(0),
  );

  for (let i = 1; i <= m; i++) {
    for (let j = 1; j <= n; j++) {
      if (leftLines[i - 1] === rightLines[j - 1]) {
        dp[i][j] = dp[i - 1][j - 1] + 1;
      } else {
        dp[i][j] = Math.max(dp[i - 1][j], dp[i][j - 1]);
      }
    }
  }

  // Backtrack to build diff
  const result: DiffLine[] = [];
  let i = m;
  let j = n;

  while (i > 0 || j > 0) {
    if (i > 0 && j > 0 && leftLines[i - 1] === rightLines[j - 1]) {
      result.unshift({
        type: "unchanged",
        leftLineNo: i,
        rightLineNo: j,
        leftContent: leftLines[i - 1],
        rightContent: rightLines[j - 1],
      });
      i--;
      j--;
    } else if (j > 0 && (i === 0 || dp[i][j - 1] >= dp[i - 1][j])) {
      result.unshift({
        type: "added",
        leftLineNo: null,
        rightLineNo: j,
        leftContent: "",
        rightContent: rightLines[j - 1],
      });
      j--;
    } else {
      result.unshift({
        type: "removed",
        leftLineNo: i,
        rightLineNo: null,
        leftContent: leftLines[i - 1],
        rightContent: "",
      });
      i--;
    }
  }

  return result;
}

/** Identify collapsible sections of unchanged lines */
function findCollapsibleSections(
  lines: DiffLine[],
  contextLines: number = 3,
): CollapsedSection[] {
  const sections: CollapsedSection[] = [];
  let unchangedStart: number | null = null;
  let unchangedCount = 0;

  for (let i = 0; i < lines.length; i++) {
    if (lines[i].type === "unchanged") {
      if (unchangedStart === null) {
        unchangedStart = i;
      }
      unchangedCount++;
    } else {
      if (unchangedStart !== null && unchangedCount > contextLines * 2 + 1) {
        sections.push({
          startIndex: unchangedStart + contextLines,
          count: unchangedCount - contextLines * 2,
        });
      }
      unchangedStart = null;
      unchangedCount = 0;
    }
  }

  // Handle trailing unchanged lines
  if (unchangedStart !== null && unchangedCount > contextLines * 2 + 1) {
    sections.push({
      startIndex: unchangedStart + contextLines,
      count: unchangedCount - contextLines * 2,
    });
  }

  return sections;
}

export function DiffViewer({
  leftYaml,
  rightYaml,
  leftTitle = "Previous",
  rightTitle = "Current",
}: DiffViewerProps) {
  const leftRef = useRef<HTMLDivElement>(null);
  const rightRef = useRef<HTMLDivElement>(null);
  const isSyncing = useRef(false);
  const [expandedSections, setExpandedSections] = useState<Set<number>>(
    new Set(),
  );

  const leftLines = useMemo(() => leftYaml.split("\n"), [leftYaml]);
  const rightLines = useMemo(() => rightYaml.split("\n"), [rightYaml]);
  const diffLines = useMemo(
    () => computeDiff(leftLines, rightLines),
    [leftLines, rightLines],
  );
  const collapsibleSections = useMemo(
    () => findCollapsibleSections(diffLines),
    [diffLines],
  );

  const stats = useMemo(() => {
    let added = 0;
    let removed = 0;
    for (const line of diffLines) {
      if (line.type === "added") added++;
      if (line.type === "removed") removed++;
    }
    return { added, removed, total: diffLines.length };
  }, [diffLines]);

  const toggleSection = useCallback((sectionIndex: number) => {
    setExpandedSections((prev) => {
      const next = new Set(prev);
      if (next.has(sectionIndex)) {
        next.delete(sectionIndex);
      } else {
        next.add(sectionIndex);
      }
      return next;
    });
  }, []);

  const handleScroll = useCallback(
    (source: "left" | "right") => {
      if (isSyncing.current) return;
      isSyncing.current = true;

      const sourceEl = source === "left" ? leftRef.current : rightRef.current;
      const targetEl = source === "left" ? rightRef.current : leftRef.current;

      if (sourceEl && targetEl) {
        targetEl.scrollTop = sourceEl.scrollTop;
      }

      requestAnimationFrame(() => {
        isSyncing.current = false;
      });
    },
    [],
  );

  /** Check if a line index falls inside a collapsed section */
  const getCollapsedInfo = useCallback(
    (lineIndex: number) => {
      for (let si = 0; si < collapsibleSections.length; si++) {
        const section = collapsibleSections[si];
        if (
          lineIndex >= section.startIndex &&
          lineIndex < section.startIndex + section.count
        ) {
          return {
            sectionIndex: si,
            isFirst: lineIndex === section.startIndex,
            isCollapsed: !expandedSections.has(si),
            count: section.count,
          };
        }
      }
      return null;
    },
    [collapsibleSections, expandedSections],
  );

  const renderLineContent = useCallback(
    (
      content: string,
      lineNo: number | null,
      type: "unchanged" | "added" | "removed",
      side: "left" | "right",
    ) => {
      const bgClass =
        type === "removed" && side === "left"
          ? "bg-red-500/15"
          : type === "added" && side === "right"
            ? "bg-green-500/15"
            : "";

      const textClass =
        type === "removed" && side === "left"
          ? "text-red-300"
          : type === "added" && side === "right"
            ? "text-green-300"
            : "text-slate-300";

      const gutterBg =
        type === "removed" && side === "left"
          ? "bg-red-500/10 text-red-400/70"
          : type === "added" && side === "right"
            ? "bg-green-500/10 text-green-400/70"
            : "text-slate-600";

      const prefix =
        type === "removed" && side === "left"
          ? "-"
          : type === "added" && side === "right"
            ? "+"
            : " ";

      return (
        <div className={cn("flex min-h-[20px] group", bgClass)}>
          <span
            className={cn(
              "select-none text-right w-10 shrink-0 pr-2 py-px text-[10px] font-mono border-r border-slate-800/50",
              gutterBg,
            )}
          >
            {lineNo ?? ""}
          </span>
          <span
            className={cn(
              "select-none w-4 shrink-0 text-center py-px text-[10px] font-mono",
              type === "removed" && side === "left"
                ? "text-red-400/60"
                : type === "added" && side === "right"
                  ? "text-green-400/60"
                  : "text-transparent",
            )}
          >
            {prefix}
          </span>
          <span
            className={cn(
              "flex-1 py-px whitespace-pre font-mono text-xs",
              textClass,
            )}
          >
            {content}
          </span>
        </div>
      );
    },
    [],
  );

  const renderPanel = useCallback(
    (side: "left" | "right") => {
      const elements: React.ReactNode[] = [];

      for (let i = 0; i < diffLines.length; i++) {
        const line = diffLines[i];
        const collapsedInfo = getCollapsedInfo(i);

        if (collapsedInfo && collapsedInfo.isCollapsed) {
          if (collapsedInfo.isFirst) {
            elements.push(
              <button
                key={`collapse-${collapsedInfo.sectionIndex}`}
                onClick={() => toggleSection(collapsedInfo.sectionIndex)}
                className="flex items-center gap-2 w-full px-3 py-1 bg-slate-800/30 hover:bg-slate-800/50 transition text-[10px] text-slate-500 hover:text-slate-400 border-y border-slate-800/30"
              >
                <ChevronRight className="w-3 h-3" />
                <span>... {collapsedInfo.count} unchanged lines ...</span>
              </button>,
            );
          }
          continue;
        }

        const lineNo = side === "left" ? line.leftLineNo : line.rightLineNo;
        const content = side === "left" ? line.leftContent : line.rightContent;

        // For added lines, left side shows empty placeholder
        if (line.type === "added" && side === "left") {
          elements.push(
            <div
              key={`${side}-${i}`}
              className="flex min-h-[20px] bg-green-500/[0.03]"
            >
              <span className="select-none text-right w-10 shrink-0 pr-2 py-px text-[10px] font-mono border-r border-slate-800/50 text-slate-700" />
              <span className="w-4 shrink-0" />
              <span className="flex-1 py-px" />
            </div>,
          );
          continue;
        }

        // For removed lines, right side shows empty placeholder
        if (line.type === "removed" && side === "right") {
          elements.push(
            <div
              key={`${side}-${i}`}
              className="flex min-h-[20px] bg-red-500/[0.03]"
            >
              <span className="select-none text-right w-10 shrink-0 pr-2 py-px text-[10px] font-mono border-r border-slate-800/50 text-slate-700" />
              <span className="w-4 shrink-0" />
              <span className="flex-1 py-px" />
            </div>,
          );
          continue;
        }

        elements.push(
          <div key={`${side}-${i}`}>
            {renderLineContent(content, lineNo, line.type, side)}
          </div>,
        );
      }

      return elements;
    },
    [diffLines, getCollapsedInfo, toggleSection, renderLineContent],
  );

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      className="flex flex-col h-full"
    >
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-2.5 border-b border-slate-800 bg-surface/50">
        <div className="flex items-center gap-2 text-sm text-slate-300">
          <Columns2 className="w-4 h-4 text-accent" />
          <span className="font-medium">YAML Diff</span>
        </div>
        <div className="flex items-center gap-3 text-xs">
          {stats.removed > 0 && (
            <span className="text-red-400">-{stats.removed}</span>
          )}
          {stats.added > 0 && (
            <span className="text-green-400">+{stats.added}</span>
          )}
          <span className="text-slate-500">
            {stats.total} line{stats.total !== 1 ? "s" : ""}
          </span>
        </div>
      </div>

      {/* Panel titles */}
      <div className="flex border-b border-slate-800">
        <div className="flex-1 px-4 py-1.5 text-[10px] uppercase tracking-wider text-slate-500 font-medium bg-red-500/[0.03] border-r border-slate-800">
          {leftTitle}
        </div>
        <div className="flex-1 px-4 py-1.5 text-[10px] uppercase tracking-wider text-slate-500 font-medium bg-green-500/[0.03]">
          {rightTitle}
        </div>
      </div>

      {/* Diff panels */}
      <div className="flex flex-1 overflow-hidden">
        {/* Left panel */}
        <div
          ref={leftRef}
          onScroll={() => handleScroll("left")}
          className="flex-1 overflow-auto bg-black/40 border-r border-slate-800"
        >
          {renderPanel("left")}
        </div>

        {/* Right panel */}
        <div
          ref={rightRef}
          onScroll={() => handleScroll("right")}
          className="flex-1 overflow-auto bg-black/40"
        >
          {renderPanel("right")}
        </div>
      </div>

      {/* Expand/collapse all */}
      {collapsibleSections.length > 0 && (
        <div className="flex items-center justify-center gap-4 px-4 py-1.5 border-t border-slate-800 bg-surface/30">
          <button
            onClick={() => {
              setExpandedSections(
                new Set(collapsibleSections.map((_, i) => i)),
              );
            }}
            className="text-[10px] text-slate-500 hover:text-slate-300 transition flex items-center gap-1"
          >
            <ChevronDown className="w-3 h-3" />
            Expand all
          </button>
          <button
            onClick={() => setExpandedSections(new Set())}
            className="text-[10px] text-slate-500 hover:text-slate-300 transition flex items-center gap-1"
          >
            <ChevronRight className="w-3 h-3" />
            Collapse all
          </button>
        </div>
      )}
    </motion.div>
  );
}
