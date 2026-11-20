import { describe, expect, it } from "vitest";
import { createTranslator } from "../i18n/translate";
import {
  capText,
  formatCost,
  formatMinutes,
  localDay,
  parseCap,
} from "./usage";

describe("parseCap", () => {
  it("treats an empty field as no cap", () => {
    expect(parseCap("")).toBeNull();
    expect(parseCap("   ")).toBeNull();
  });

  it("accepts positive amounts with a point, a comma or a dollar sign", () => {
    expect(parseCap("5")).toBe(5);
    expect(parseCap("0.01")).toBe(0.01);
    expect(parseCap("2,50")).toBe(2.5);
    expect(parseCap(" $10 ")).toBe(10);
    expect(parseCap("10000")).toBe(10_000);
  });

  it("rejects zero, negatives, text and amounts over the maximum", () => {
    for (const bad of ["0", "0.00", "-1", "abc", "1e3", "1.2.3", "10000.01"]) {
      expect(parseCap(bad), bad).toBeUndefined();
    }
  });

  it("round-trips through the field text", () => {
    expect(capText(null)).toBe("");
    expect(parseCap(capText(12.5))).toBe(12.5);
  });
});

describe("usage formatting", () => {
  const { format } = createTranslator("en", "en-US");

  it("shows minutes with one decimal", () => {
    expect(formatMinutes(format, 90)).toBe("1.5");
    expect(formatMinutes(format, 0)).toBe("0.0");
  });

  it("shows a tenth of a cent under a dollar only", () => {
    expect(formatCost(format, 0.034)).toBe("$0.034");
    expect(formatCost(format, 0)).toBe("$0.00");
    expect(formatCost(format, 0.5)).toBe("$0.50");
    expect(formatCost(format, 12.345)).toBe("$12.35");
  });

  it("reads a history day as local midnight", () => {
    const day = localDay("2026-03-01");
    expect([day.getFullYear(), day.getMonth(), day.getDate()]).toEqual([
      2026, 2, 1,
    ]);
    expect(day.getHours()).toBe(0);
  });
});
