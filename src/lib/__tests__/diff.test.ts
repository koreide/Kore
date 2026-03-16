import { describe, it, expect } from "vitest";
import { computeDiff, findCollapsibleSections } from "../diff";

describe("computeDiff", () => {
  it("marks identical lines as unchanged", () => {
    const lines = ["a", "b", "c"];
    const result = computeDiff(lines, lines);
    expect(result).toHaveLength(3);
    expect(result.every((l) => l.type === "unchanged")).toBe(true);
    expect(result[0]).toMatchObject({ leftLineNo: 1, rightLineNo: 1, leftContent: "a" });
    expect(result[2]).toMatchObject({ leftLineNo: 3, rightLineNo: 3, leftContent: "c" });
  });

  it("marks all as added when left is empty", () => {
    const result = computeDiff([], ["x", "y"]);
    expect(result).toHaveLength(2);
    expect(result.every((l) => l.type === "added")).toBe(true);
    expect(result[0]).toMatchObject({ leftLineNo: null, rightLineNo: 1, rightContent: "x" });
    expect(result[1]).toMatchObject({ leftLineNo: null, rightLineNo: 2, rightContent: "y" });
  });

  it("marks all as removed when right is empty", () => {
    const result = computeDiff(["x", "y"], []);
    expect(result).toHaveLength(2);
    expect(result.every((l) => l.type === "removed")).toBe(true);
    expect(result[0]).toMatchObject({ leftLineNo: 1, rightLineNo: null, leftContent: "x" });
    expect(result[1]).toMatchObject({ leftLineNo: 2, rightLineNo: null, leftContent: "y" });
  });

  it("returns empty result for both sides empty", () => {
    const result = computeDiff([], []);
    expect(result).toHaveLength(0);
  });

  it("detects a single line added in the middle", () => {
    const result = computeDiff(["a", "c"], ["a", "b", "c"]);
    expect(result).toHaveLength(3);
    expect(result[0]).toMatchObject({ type: "unchanged", leftContent: "a" });
    expect(result[1]).toMatchObject({ type: "added", rightContent: "b", leftLineNo: null });
    expect(result[2]).toMatchObject({ type: "unchanged", leftContent: "c" });
  });

  it("detects a single line removed in the middle", () => {
    const result = computeDiff(["a", "b", "c"], ["a", "c"]);
    expect(result).toHaveLength(3);
    expect(result[0]).toMatchObject({ type: "unchanged", leftContent: "a" });
    expect(result[1]).toMatchObject({ type: "removed", leftContent: "b", rightLineNo: null });
    expect(result[2]).toMatchObject({ type: "unchanged", leftContent: "c" });
  });

  it("handles multi-line interleaved changes with correct line numbers", () => {
    const left = ["a", "b", "c", "d"];
    const right = ["a", "x", "c", "y"];
    const result = computeDiff(left, right);

    const unchanged = result.filter((l) => l.type === "unchanged");
    const added = result.filter((l) => l.type === "added");
    const removed = result.filter((l) => l.type === "removed");

    // "a" and "c" are common
    expect(unchanged).toHaveLength(2);
    expect(unchanged[0].leftContent).toBe("a");
    expect(unchanged[1].leftContent).toBe("c");

    // "b" and "d" removed, "x" and "y" added
    expect(removed).toHaveLength(2);
    expect(added).toHaveLength(2);
  });
});

describe("findCollapsibleSections", () => {
  it("returns no sections when changes are close together", () => {
    const lines = computeDiff(["a", "b", "c"], ["a", "x", "c"]);
    const sections = findCollapsibleSections(lines);
    expect(sections).toHaveLength(0);
  });

  it("collapses large unchanged blocks with default context=3", () => {
    // 10 unchanged, 1 changed, 10 unchanged
    const left = Array.from({ length: 21 }, (_, i) => (i === 10 ? "old" : `line${i}`));
    const right = Array.from({ length: 21 }, (_, i) => (i === 10 ? "new" : `line${i}`));
    const diff = computeDiff(left, right);
    const sections = findCollapsibleSections(diff);

    // Should have 2 collapsible sections: the leading block and the trailing block
    expect(sections.length).toBe(2);

    // Leading block: 10 unchanged lines, context=3 → collapse starts at index 3, count = 10-6 = 4
    expect(sections[0].startIndex).toBe(3);
    expect(sections[0].count).toBe(4);
  });

  it("preserves context lines around changes", () => {
    // 12 unchanged, 1 change, 12 unchanged
    const left = Array.from({ length: 25 }, (_, i) => (i === 12 ? "old" : `line${i}`));
    const right = Array.from({ length: 25 }, (_, i) => (i === 12 ? "new" : `line${i}`));
    const diff = computeDiff(left, right);
    const sections = findCollapsibleSections(diff, 3);

    // Both leading and trailing blocks should be collapsible
    expect(sections.length).toBe(2);

    // Verify context is preserved (first section starts after 3 context lines)
    expect(sections[0].startIndex).toBe(3);
    // Verify trailing section: 12 unchanged lines after change, context=3
    // starts at change_index + 1 + 3, count = 12 - 6 = 6
    expect(sections[1].count).toBe(6);
  });

  it("handles trailing unchanged lines", () => {
    // 1 change followed by 10 unchanged
    const left = ["old", ...Array.from({ length: 10 }, (_, i) => `line${i}`)];
    const right = ["new", ...Array.from({ length: 10 }, (_, i) => `line${i}`)];
    const diff = computeDiff(left, right);
    const sections = findCollapsibleSections(diff, 3);

    // The trailing 10 unchanged lines should produce a collapsible section
    expect(sections.length).toBe(1);
    expect(sections[0].count).toBe(4); // 10 - 6 = 4
  });
});
