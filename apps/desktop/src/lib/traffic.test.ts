import { describe, expect, it } from "vitest";
import type { TrafficSeries } from "@/lib/ipc/bindings";
import {
  appendSeries,
  classShares,
  emptySeries,
  formatRate,
  formatShare,
  perSecond,
  recentRate,
  withGaps,
} from "./traffic";

/** A series with one sample per `at`, each with `requests` = its index + 1. */
function series(at: number[], span = 1): TrafficSeries {
  const s = emptySeries();
  at.forEach((t, i) => {
    s.at.push(t);
    s.span.push(span);
    s.requests.push(i + 1);
    s.errors.push(0);
    s.ok.push(i + 1);
    s.redirects.push(0);
    s.clientErrors.push(0);
    s.serverErrors.push(0);
    s.concurrent.push(0);
    s.connections.push(4);
    s.rttMs.push(20);
  });
  return s;
}

describe("appendSeries", () => {
  it("appends only newer samples and keeps every column aligned", () => {
    const merged = appendSeries(series([1000, 2000]), series([2000, 3000, 4000]));
    expect(merged.at).toEqual([1000, 2000, 3000, 4000]);
    expect(merged.requests).toEqual([1, 2, 2, 3]);
    expect(merged.rttMs).toHaveLength(4);
  });

  it("returns the same object when nothing is new", () => {
    const older = series([1000, 2000]);
    expect(appendSeries(older, series([2000]))).toBe(older);
    expect(appendSeries(older, emptySeries())).toBe(older);
  });

  it("drops the oldest samples beyond the capacity", () => {
    const merged = appendSeries(series([1, 2, 3]), series([4, 5]), 4);
    expect(merged.at).toEqual([2, 3, 4, 5]);
    expect(merged.span).toHaveLength(4);
  });
});

describe("withGaps", () => {
  it("converts to seconds and breaks lines across missing time", () => {
    const [xs, ys] = withGaps([0, 1000, 2000, 60_000], [[1, 2, 3, 4]], 25);
    expect(xs).toEqual([0, 1, 2, 14.5, 60]);
    expect(ys).toEqual([1, 2, 3, null, 4]);
  });

  it("handles several series and no samples", () => {
    expect(withGaps([], [[], []], 10)).toEqual([[], [], []]);
    const [, a, b] = withGaps(
      [0, 1000],
      [
        [1, 2],
        [null, 5],
      ],
      10,
    );
    expect([a, b]).toEqual([
      [1, 2],
      [null, 5],
    ]);
  });
});

describe("rates", () => {
  it("divides counts by their interval", () => {
    expect(perSecond([10, 5, 3], [10, 0, 1])).toEqual([1, null, 3]);
  });

  it("averages the recent window", () => {
    const s = series([1000, 2000, 3000], 1); // requests 1, 2, 3
    expect(recentRate(s, 1.5, 3000)).toBe(2.5);
    expect(recentRate(s, 10, 3000)).toBe(2);
    expect(recentRate(emptySeries(), 10)).toBeNull();
  });
});

describe("classShares", () => {
  it("orders classes by share and skips empty ones", () => {
    const s = series([1, 2]);
    s.ok = [90, 5];
    s.serverErrors = [5, 0];
    expect(classShares(s)).toEqual([
      { label: "2xx", share: 0.95 },
      { label: "5xx", share: 0.05 },
    ]);
    expect(classShares(emptySeries())).toEqual([]);
  });
});

describe("formatting", () => {
  it("formats rates compactly", () => {
    expect([null, 0, 0.25, 0.5, 12.34, 250, 1_234].map(formatRate)).toEqual([
      "–",
      "0",
      "0.25",
      "0.5",
      "12.3",
      "250",
      "1.2K",
    ]);
  });

  it("formats shares", () => {
    expect([0.98, 0.004, 0.0004, 0.055, 1, 0.9995, 0.99999, 0.02000001].map(formatShare)).toEqual([
      "98%",
      "0.4%",
      "<0.1%",
      "5.5%",
      "100%",
      "99.9%",
      "99.9%",
      "2%",
    ]);
  });
});
