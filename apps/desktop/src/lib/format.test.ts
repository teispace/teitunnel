import { describe, expect, it } from "vitest";
import { formatCount, formatDuration, stripScheme } from "./format";

describe("format", () => {
  it("formats durations compactly", () => {
    expect(formatDuration(0)).toBe("0 s");
    expect(formatDuration(59_999)).toBe("59 s");
    expect(formatDuration(60_000)).toBe("1 min");
    expect(formatDuration(3_600_000)).toBe("1 h");
    expect(formatDuration(3_900_000)).toBe("1 h 5 min");
    expect(formatDuration(-5)).toBe("0 s");
  });

  it("pluralises counts", () => {
    expect(formatCount(1, "request")).toBe("1 request");
    expect(formatCount(1234, "request")).toBe("1,234 requests");
  });

  it("strips schemes", () => {
    expect(stripScheme("https://x.trycloudflare.com")).toBe("x.trycloudflare.com");
    expect(stripScheme("http://localhost:3000")).toBe("localhost:3000");
  });
});
