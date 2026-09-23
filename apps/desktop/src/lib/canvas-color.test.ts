import { describe, expect, it } from "vitest";
import { parseColor, toRgba } from "./canvas-color";

describe("parseColor", () => {
  it("reads the formats WebKit computes", () => {
    expect(parseColor("rgb(0, 122, 255)")).toEqual({ r: 0, g: 122, b: 255, a: 1 });
    expect(parseColor("rgba(0, 0, 0, 0.5)")).toEqual({ r: 0, g: 0, b: 0, a: 0.5 });
    expect(parseColor("rgb(255 255 255 / 55%)")).toEqual({ r: 255, g: 255, b: 255, a: 0.55 });
    expect(parseColor("color(srgb 0 0.5 1 / 0.3)")).toEqual({ r: 0, g: 128, b: 255, a: 0.3 });
    expect(parseColor("color(srgb 1.2 -0.1 0)")).toEqual({ r: 255, g: 0, b: 0, a: 1 });
  });

  it("treats anything else as transparent", () => {
    expect(parseColor("AccentColor").a).toBe(0);
    expect(parseColor("").a).toBe(0);
  });
});

describe("toRgba", () => {
  it("multiplies the alpha", () => {
    expect(toRgba({ r: 1, g: 2, b: 3, a: 0.5 }, 0.5)).toBe("rgba(1, 2, 3, 0.25)");
    expect(toRgba({ r: 1, g: 2, b: 3, a: 1 })).toBe("rgba(1, 2, 3, 1)");
  });
});
