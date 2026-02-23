import { describe, it, expect } from "vitest";
import { formatError } from "../errors";

describe("formatError", () => {
  it("returns string errors as-is", () => {
    expect(formatError("something went wrong")).toBe("something went wrong");
  });

  it("extracts message from Error objects", () => {
    expect(formatError(new Error("bad request"))).toBe("bad request");
  });

  it("extracts message from objects with message property", () => {
    expect(formatError({ message: "custom error" })).toBe("custom error");
  });

  it("stringifies unknown values", () => {
    expect(formatError(42)).toBe("42");
  });

  it("handles null/undefined gracefully", () => {
    expect(formatError(null)).toBe("Unknown error");
    expect(formatError(undefined)).toBe("Unknown error");
  });
});
