export interface DiffLine {
  type: "unchanged" | "added" | "removed";
  leftLineNo: number | null;
  rightLineNo: number | null;
  leftContent: string;
  rightContent: string;
}

export interface CollapsedSection {
  startIndex: number;
  count: number;
}

/** Simple LCS-based diff algorithm for line-by-line comparison */
export function computeDiff(leftLines: string[], rightLines: string[]): DiffLine[] {
  const m = leftLines.length;
  const n = rightLines.length;

  // Build LCS table
  const dp: number[][] = Array.from({ length: m + 1 }, () => Array(n + 1).fill(0));

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
export function findCollapsibleSections(
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
