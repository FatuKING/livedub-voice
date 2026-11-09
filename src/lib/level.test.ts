import { describe, expect, it } from "vitest";
import { amplitudeToDb, amplitudeToMeterPercent } from "./level";

describe("amplitudeToDb", () => {
  it("converts linear amplitude to dBFS", () => {
    expect(amplitudeToDb(1)).toBe(0);
    expect(amplitudeToDb(0.1)).toBeCloseTo(-20);
    expect(amplitudeToDb(0)).toBe(-Infinity);
  });
});

describe("amplitudeToMeterPercent", () => {
  it("maps -60..0 dBFS to 0..100", () => {
    expect(amplitudeToMeterPercent(0)).toBe(0);
    expect(amplitudeToMeterPercent(0.001)).toBe(0); // -60 dBFS
    expect(amplitudeToMeterPercent(0.01)).toBe(33); // -40 dBFS
    expect(amplitudeToMeterPercent(0.1)).toBe(67); // -20 dBFS
    expect(amplitudeToMeterPercent(1)).toBe(100);
  });

  it("clamps out-of-range values", () => {
    expect(amplitudeToMeterPercent(2)).toBe(100);
    expect(amplitudeToMeterPercent(-1)).toBe(0);
  });
});
