// @vitest-environment node
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { renderMotionCss } from "./motion-css";
import { springPosition, springs, springToCss } from "./motion-tokens";

describe("spring tokens", () => {
  it("start at 0 and settle at 1", () => {
    for (const token of Object.values(springs)) {
      expect(springPosition(token, 0)).toBeCloseTo(0, 6);
      expect(springPosition(token, 3)).toBeCloseTo(1, 3);
    }
  });

  it("only bouncy springs overshoot", () => {
    const peak = (name: keyof typeof springs) =>
      Math.max(...Array.from({ length: 300 }, (_, i) => springPosition(springs[name], i / 100)));
    expect(peak("smooth")).toBeLessThanOrEqual(1.0001);
    expect(peak("bouncy")).toBeGreaterThan(1.01);
  });

  it("produce a valid linear() easing", () => {
    const { easing, durationMs } = springToCss(springs.snappy);
    expect(easing.startsWith("linear(0, ")).toBe(true);
    expect(easing.endsWith(", 1)")).toBe(true);
    expect(durationMs).toBeGreaterThan(springs.snappy.visualDuration * 1000);
  });
});

describe("styles/motion.css", () => {
  it("is up to date with the tokens (run `node scripts/gen-motion-css.ts`)", () => {
    const onDisk = readFileSync(new URL("../styles/motion.css", import.meta.url), "utf8");
    expect(onDisk).toBe(renderMotionCss());
  });
});
