import { describe, expect, it } from "vitest";
import type { AnalyticsSummary, HostSummary } from "@/lib/ipc/bindings";
import { combinedSeries, totals } from "./components/analytics-overview";
import type { Row } from "./model";

const host = (hostname: string, over: Partial<HostSummary> = {}): HostSummary => ({
  hostname,
  requests: 0,
  bytes: 0,
  errorRate: null,
  p95Ms: null,
  spark: [],
  sparkErrors: [],
  ...over,
});

const summary = (hosts: HostSummary[]): AnalyticsSummary => ({
  range: "day",
  hosts,
  availableFrom: null,
  unavailable: [],
  endsAt: 10_000_000,
  bucketSeconds: 60,
  fetchedAt: 10_000_000,
});

const row = (key: string, requests: number | null, uptime: number | null): Row =>
  ({
    key,
    hostname: key,
    path: null,
    local: true,
    requests,
    errorRate: null,
    p95: null,
    uptime,
    spark: [],
    up: null,
  }) as Row;

describe("analytics totals", () => {
  it("adds up every route and weighs error rates by requests", () => {
    const s = summary([
      host("a.xyz.com", { requests: 900, bytes: 1_000, errorRate: 0, p95Ms: 80 }),
      host("b.xyz.com", { requests: 100, bytes: 500, errorRate: 0.1, p95Ms: 300 }),
    ]);
    const sum = totals([row("a.xyz.com", 900, 0.999), row("b.xyz.com", 100, 0.95)], s);
    expect(sum.requests).toBe(1000);
    expect(sum.bytes).toBe(1500);
    expect(sum.errorRate).toBeCloseTo(0.01);
    expect(sum.slowestP95).toEqual({ hostname: "b.xyz.com", ms: 300 });
    expect(sum.lowestUptime).toEqual({ key: "b.xyz.com", share: 0.95 });
    expect(sum.busiest).toEqual({ key: "a.xyz.com", requests: 900 });
  });

  it("knows nothing before the summary arrives", () => {
    const sum = totals([row("a.xyz.com", null, null)], undefined);
    expect(sum).toMatchObject({ requests: 0, errorRate: null, busiest: null, lowestUptime: null });
  });

  it("sums the routes' sparklines bucket by bucket, aligned at their end", () => {
    const data = combinedSeries(
      summary([
        host("a", { spark: [1, 2, 3], sparkErrors: [0, 1, 0] }),
        host("b", { spark: [10, 20], sparkErrors: [0, 0] }),
      ]),
    );
    expect(data).toEqual([
      [9_880, 9_940, 10_000],
      [1, 12, 23],
      [null, 1, null],
    ]);
    expect(combinedSeries(summary([host("a", { spark: [5] })]))).toBeNull();
  });
});
