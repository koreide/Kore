import { describe, it, expect, vi, afterEach } from "vitest";
import {
  parseKubeQuantity,
  parseUsagePercent,
  scoreColor,
  scoreBorderColor,
  scoreGlowColor,
  scoreLabel,
  scoreBgColor,
  formatRelativeAge,
  getStatusVariant,
} from "../k8s-utils";

describe("parseKubeQuantity", () => {
  it("parses Ki suffix", () => {
    expect(parseKubeQuantity("100Ki")).toBe(100 * 1024);
  });

  it("parses Mi suffix", () => {
    expect(parseKubeQuantity("2Mi")).toBe(2 * 1024 * 1024);
  });

  it("parses Gi suffix", () => {
    expect(parseKubeQuantity("4Gi")).toBe(4 * 1024 * 1024 * 1024);
  });

  it("parses Ti suffix", () => {
    expect(parseKubeQuantity("1Ti")).toBe(1024 * 1024 * 1024 * 1024);
  });

  it("parses m (milli) suffix", () => {
    expect(parseKubeQuantity("500m")).toBe(0.5);
  });

  it("parses n (nano) suffix", () => {
    expect(parseKubeQuantity("100n")).toBe(100 / 1_000_000_000);
  });

  it("parses plain number", () => {
    expect(parseKubeQuantity("4")).toBe(4);
  });

  it("returns 0 for invalid input", () => {
    expect(parseKubeQuantity("abc")).toBe(0);
  });

  it("parses floating point with suffix", () => {
    expect(parseKubeQuantity("1.5Gi")).toBe(1.5 * 1024 * 1024 * 1024);
  });

  it("trims whitespace", () => {
    expect(parseKubeQuantity("  500m  ")).toBe(0.5);
  });
});

describe("parseUsagePercent", () => {
  it("calculates normal percentage", () => {
    expect(parseUsagePercent("500m", "1")).toBe(50);
  });

  it("returns 0 for zero capacity", () => {
    expect(parseUsagePercent("500m", "0")).toBe(0);
  });

  it("caps at 100 when usage exceeds capacity", () => {
    expect(parseUsagePercent("2", "1")).toBe(100);
  });

  it("returns 0 for missing values", () => {
    expect(parseUsagePercent(undefined, "1")).toBe(0);
    expect(parseUsagePercent("500m", undefined)).toBe(0);
    expect(parseUsagePercent(undefined, undefined)).toBe(0);
  });
});

describe("score functions", () => {
  describe("scoreColor", () => {
    it("returns emerald for score >= 80", () => {
      expect(scoreColor(80)).toBe("text-emerald-400");
      expect(scoreColor(100)).toBe("text-emerald-400");
    });

    it("returns amber for score 50-79", () => {
      expect(scoreColor(79)).toBe("text-amber-400");
      expect(scoreColor(50)).toBe("text-amber-400");
    });

    it("returns red for score < 50", () => {
      expect(scoreColor(49)).toBe("text-red-400");
      expect(scoreColor(0)).toBe("text-red-400");
    });
  });

  describe("scoreLabel", () => {
    it("returns Healthy for >= 80", () => {
      expect(scoreLabel(80)).toBe("Healthy");
      expect(scoreLabel(100)).toBe("Healthy");
    });

    it("returns Degraded for 50-79", () => {
      expect(scoreLabel(79)).toBe("Degraded");
      expect(scoreLabel(50)).toBe("Degraded");
    });

    it("returns Critical for < 50", () => {
      expect(scoreLabel(49)).toBe("Critical");
      expect(scoreLabel(0)).toBe("Critical");
    });
  });

  describe("scoreBorderColor", () => {
    it("returns emerald border for >= 80", () => {
      expect(scoreBorderColor(80)).toContain("emerald");
    });

    it("returns amber border for 50-79", () => {
      expect(scoreBorderColor(50)).toContain("amber");
    });

    it("returns red border for < 50", () => {
      expect(scoreBorderColor(49)).toContain("red");
    });
  });

  describe("scoreGlowColor", () => {
    it("returns emerald glow for >= 80", () => {
      expect(scoreGlowColor(80)).toContain("emerald");
    });

    it("returns amber glow for 50-79", () => {
      expect(scoreGlowColor(50)).toContain("amber");
    });

    it("returns red glow for < 50", () => {
      expect(scoreGlowColor(49)).toContain("red");
    });
  });

  describe("scoreBgColor", () => {
    it("returns emerald bg for >= 80", () => {
      expect(scoreBgColor(80)).toContain("emerald");
    });

    it("returns amber bg for 50-79", () => {
      expect(scoreBgColor(50)).toContain("amber");
    });

    it("returns red bg for < 50", () => {
      expect(scoreBgColor(49)).toContain("red");
    });
  });
});

describe("formatRelativeAge", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("returns days for timestamps days ago", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-03-16T12:00:00Z"));
    expect(formatRelativeAge("2026-03-14T12:00:00Z")).toBe("2d");
  });

  it("returns hours for timestamps hours ago", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-03-16T12:00:00Z"));
    expect(formatRelativeAge("2026-03-16T09:00:00Z")).toBe("3h");
  });

  it("returns minutes for timestamps minutes ago", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-03-16T12:00:00Z"));
    expect(formatRelativeAge("2026-03-16T11:15:00Z")).toBe("45m");
  });

  it("returns seconds for timestamps seconds ago", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-03-16T12:00:30Z"));
    expect(formatRelativeAge("2026-03-16T12:00:00Z")).toBe("30s");
  });

  it('returns "-" for missing/undefined', () => {
    expect(formatRelativeAge(undefined)).toBe("-");
    expect(formatRelativeAge("")).toBe("-");
  });

  it('returns "-" for invalid date', () => {
    expect(formatRelativeAge("not-a-date")).toBe("-");
  });
});

describe("getStatusVariant", () => {
  it('returns "running" for running states', () => {
    for (const s of ["Running", "Ready", "Succeeded", "Completed", "Complete"]) {
      expect(getStatusVariant(s)).toBe("running");
    }
  });

  it('returns "pending" for pending states', () => {
    for (const s of ["Pending", "ContainerCreating", "Init", "Waiting"]) {
      expect(getStatusVariant(s)).toBe("pending");
    }
  });

  it('returns "failed" for failed states', () => {
    for (const s of [
      "Failed",
      "CrashLoopBackOff",
      "Error",
      "ImagePullBackOff",
      "Evicted",
      "OOMKilled",
    ]) {
      expect(getStatusVariant(s)).toBe("failed");
    }
  });

  it('returns "terminating" for Terminating', () => {
    expect(getStatusVariant("Terminating")).toBe("terminating");
  });

  it('returns "default" for unknown/undefined', () => {
    expect(getStatusVariant(undefined)).toBe("default");
    expect(getStatusVariant("SomethingElse")).toBe("default");
  });
});
