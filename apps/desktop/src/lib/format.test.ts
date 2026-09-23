import { describe, expect, it } from "vitest";
import { formatDuration, stripScheme } from "./format";

describe("format", () => {
  it("formats durations compactly", () => {
    expect(formatDuration(0)).toBe("0 sec");
    expect(formatDuration(59_999)).toBe("59 sec");
    expect(formatDuration(60_000)).toBe("1 min");
    expect(formatDuration(3_600_000)).toBe("1 hr");
    expect(formatDuration(3_900_000)).toBe("1 hr 5 min");
    expect(formatDuration(-5)).toBe("0 sec");
  });

  it("strips schemes", () => {
    expect(stripScheme("https://x.trycloudflare.com")).toBe("x.trycloudflare.com");
    expect(stripScheme("http://localhost:3000")).toBe("localhost:3000");
  });
});
