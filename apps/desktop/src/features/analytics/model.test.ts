import { describe, expect, it } from "vitest";
import type { AnalyticsSummary, RouteView, UptimeSummary } from "@/lib/ipc/bindings";
import {
  buildRows,
  formatBytes,
  formatMs,
  formatPercent,
  pathPrefix,
  routeKey,
  sortRows,
  unavailableNote,
} from "./model";

const route = (hostname: string, path: string | null = null, local = true): RouteView =>
  ({
    hostname,
    path,
    origin: "http://localhost:3000",
    local,
    zone: "xyz.com",
    dns: { state: "ok" },
    access: null,
    client: null,
    tunnelId: "t",
    temporary: false,
    balanced: false,
    options: {},
  }) as unknown as RouteView;

const uptime = (hostname: string, path: string | null, day: number | null): UptimeSummary => ({
  accountId: "acc",
  route: { hostname, path },
  up: day === null ? null : day > 0.5,
  lastChecked: day === null ? null : 1,
  lastLatencyMs: 50,
  lastCause: null,
  uptimeDay: day,
  uptimeWeek: day,
  uptimeMonth: day,
  p95Ms: 120,
  openIncident: null,
});

describe("paths and keys", () => {
  it("reads the literal prefix of a path rule like the core", () => {
    expect(pathPrefix("^/api/.*")).toBe("/api/");
    expect(pathPrefix("^/")).toBeNull();
    expect(pathPrefix("\\.(png|jpg)$")).toBeNull();
    expect(pathPrefix(null)).toBeNull();
    expect(routeKey("a.xyz.com", "^/v1/")).toBe("a.xyz.com/v1/");
    expect(routeKey("a.xyz.com", null)).toBe("a.xyz.com");
  });
});

describe("formatting", () => {
  it("never rounds a partial outage to 100%", () => {
    expect(formatPercent(1)).toBe("100%");
    expect(formatPercent(0.99996)).toBe("99.9%");
    expect(formatPercent(0.9965)).toBe("99.7%");
    expect(formatPercent(0.0004)).toBe("<0.1%");
    expect(formatPercent(0.052)).toBe("5.2%");
    expect(formatPercent(0)).toBe("0%");
    expect(formatPercent(null)).toBe("–");
  });

  it("formats times and sizes", () => {
    expect(formatMs(184.4)).toBe("184 ms");
    expect(formatMs(12_500)).toBe("12.5 s");
    expect(formatMs(null)).toBe("–");
    expect(formatBytes(999)).toBe("999 B");
    expect(formatBytes(2_500_000)).toBe("2.5 MB");
  });

  it("names what the plan doesn't offer", () => {
    expect(unavailableNote([])).toBeNull();
    expect(unavailableNote(["latency", "paths"])).toBe(
      "Not on this domain's plan: response times, paths.",
    );
  });
});

describe("rows", () => {
  const summary: AnalyticsSummary = {
    range: "day",
    hosts: [
      {
        hostname: "a.xyz.com",
        requests: 50,
        bytes: 1,
        errorRate: 0.1,
        p95Ms: 200,
        spark: [1, 2],
        sparkErrors: [0, 1],
      },
      {
        hostname: "b.xyz.com",
        requests: 500,
        bytes: 1,
        errorRate: 0,
        p95Ms: null,
        spark: [5, 6],
        sparkErrors: [0, 0],
      },
    ],
    availableFrom: null,
    unavailable: [],
    endsAt: 0,
    bucketSeconds: 900,
    fetchedAt: 0,
  };

  it("joins edge numbers by hostname and uptime by route", () => {
    const rows = buildRows(
      [route("a.xyz.com"), route("b.xyz.com", "^/api/"), route("c.xyz.com", null, false)],
      summary,
      [uptime("a.xyz.com", null, 0.99), uptime("b.xyz.com", "/api/", 1)],
      "day",
    );
    expect(rows.map((r) => [r.key, r.requests, r.uptime])).toEqual([
      ["a.xyz.com", 50, 0.99],
      ["b.xyz.com/api/", 500, 1],
      ["c.xyz.com", null, null],
    ]);
  });

  it("sorts either way with missing values last", () => {
    const rows = buildRows(
      [route("a.xyz.com"), route("b.xyz.com"), route("c.xyz.com")],
      summary,
      [],
      "day",
    );
    expect(sortRows(rows, "requests", "descending").map((r) => r.hostname)).toEqual([
      "b.xyz.com",
      "a.xyz.com",
      "c.xyz.com",
    ]);
    expect(sortRows(rows, "requests", "ascending").map((r) => r.hostname)).toEqual([
      "a.xyz.com",
      "b.xyz.com",
      "c.xyz.com",
    ]);
    expect(sortRows(rows, "p95", "descending").map((r) => r.hostname)).toEqual([
      "a.xyz.com",
      "b.xyz.com",
      "c.xyz.com",
    ]);
    expect(sortRows(rows, "route", "descending")[0]?.hostname).toBe("c.xyz.com");
  });
});
