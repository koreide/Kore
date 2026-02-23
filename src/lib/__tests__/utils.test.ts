import { describe, it, expect } from "vitest";
import { cn } from "../utils";

describe("cn", () => {
  it("merges class names", () => {
    expect(cn("foo", "bar")).toBe("foo bar");
  });

  it("filters out falsy values", () => {
    expect(cn("foo", false, null, undefined, "bar")).toBe("foo bar");
  });

  it("resolves tailwind conflicts (last wins)", () => {
    expect(cn("px-2", "px-4")).toBe("px-4");
  });

  it("returns empty string for no input", () => {
    expect(cn()).toBe("");
  });

  it("handles single class", () => {
    expect(cn("text-sm")).toBe("text-sm");
  });
});
