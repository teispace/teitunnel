import type {
  AlertRules,
  AnalyticsRange,
  AnalyticsSummary,
  RouteStats,
  StatsSeries,
  UptimeBar,
  UptimeSummary,
} from "@/lib/ipc/bindings";

/**
 * Dev-only analytics, uptime and alert fixtures for the browser preview (see
 * `mock-ipc.ts`). Numbers are shaped like real traffic: a daily rhythm, a few 5xx.
 */
const SECONDS: Record<AnalyticsRange, [range: number, bucket: number]> = {
  hour: [3_600, 60],
  day: [86_400, 900],
  week: [604_800, 3_600],
  month: [2_592_000, 86_400],
};

function wave(i: number, n: number, scale: number) {
  const t = i / n;
  return Math.max(0, Math.round(scale * (1 + 0.6 * Math.sin(t * 9) + 0.25 * Math.sin(t * 37))));
}

export function mockStatsSeries(
  range: AnalyticsRange,
  scale: number,
  now = Date.now(),
): StatsSeries {
  const [total, bucket] = SECONDS[range];
  const n = Math.round(total / bucket);
  const at: number[] = [];
  const requests: number[] = [];
  const serverErrors: number[] = [];
  for (let i = 0; i < n; i++) {
    at.push(now - (n - 1 - i) * bucket * 1000);
    const count = wave(i, n, (scale * bucket) / 60);
    requests.push(count);
    serverErrors.push(i % 23 === 7 ? Math.ceil(count * 0.08) : 0);
  }
  return {
    at,
    span: at.map(() => bucket),
    requests,
    clientErrors: requests.map((c) => Math.round(c * 0.02)),
    serverErrors,
    bytes: requests.map((c) => c * 14_000),
  };
}

const sum = (values: readonly number[]) => values.reduce((a, b) => a + b, 0);

export function mockSummary(hostnames: readonly string[], range: AnalyticsRange): AnalyticsSummary {
  const now = Date.now();
  return {
    range,
    hosts: hostnames.map((hostname, index) => {
      const series = mockStatsSeries(range, 12 / (index + 1), now);
      const requests = sum(series.requests);
      return {
        hostname,
        requests,
        bytes: sum(series.bytes),
        errorRate: requests > 0 ? sum(series.serverErrors) / requests : null,
        p95Ms: index === 0 ? 184 : 92 + index * 11,
        spark: series.requests,
        sparkErrors: series.serverErrors,
      };
    }),
    availableFrom: null,
    unavailable: [],
    endsAt: now,
    bucketSeconds: SECONDS[range][1],
    fetchedAt: now,
  };
}

export function mockRouteStats(
  hostname: string,
  path: string | null,
  range: AnalyticsRange,
): RouteStats {
  const series = mockStatsSeries(range, 12);
  const requests = sum(series.requests);
  const errors = sum(series.serverErrors);
  const clientErrors = sum(series.clientErrors);
  return {
    source: "edge",
    route: { hostname, path },
    range,
    series,
    requests,
    rate: {
      average: requests / sum(series.span),
      peak: Math.max(0, ...series.requests.map((n, i) => n / (series.span[i] ?? 1))),
    },
    bytes: sum(series.bytes),
    classes: {
      ok: requests - errors - clientErrors - Math.round(requests * 0.03),
      redirects: Math.round(requests * 0.03),
      clientErrors,
      serverErrors: errors,
    },
    statuses: [
      { key: "200", requests: Math.round(requests * 0.9) },
      { key: "304", requests: Math.round(requests * 0.03) },
      { key: "404", requests: clientErrors },
      { key: "502", requests: errors },
    ],
    paths: [
      { key: "/", requests: Math.round(requests * 0.41) },
      { key: "/api/session", requests: Math.round(requests * 0.22) },
      { key: "/assets/index-4f1c.js", requests: Math.round(requests * 0.12) },
      { key: "/api/events", requests: Math.round(requests * 0.08) },
      { key: "/favicon.ico", requests: Math.round(requests * 0.05) },
    ],
    countries: [
      { key: "DE", requests: Math.round(requests * 0.38) },
      { key: "US", requests: Math.round(requests * 0.27) },
      { key: "NP", requests: Math.round(requests * 0.12) },
      { key: "GB", requests: Math.round(requests * 0.08) },
    ],
    browsers: [
      { key: "Chrome", requests: Math.round(requests * 0.55) },
      { key: "Safari", requests: Math.round(requests * 0.3) },
      { key: "Firefox", requests: Math.round(requests * 0.09) },
    ],
    bots: [
      { key: "", requests: Math.round(requests * 0.96) },
      { key: "Search Engine Crawler", requests: Math.round(requests * 0.04) },
    ],
    cache: [
      { key: "dynamic", requests: Math.round(requests * 0.8) },
      { key: "hit", requests: Math.round(requests * 0.2) },
    ],
    originMs: { p50: 42, p95: 184, p99: 402 },
    ttfbMs: { p50: 61, p95: 230, p99: 480 },
    availableFrom: null,
    unavailable: [],
    fetchedAt: Date.now(),
  };
}

export function mockUptimeSummary(hostname: string, path: string | null): UptimeSummary {
  const now = Date.now();
  return {
    accountId: "acc-personal",
    route: { hostname, path },
    up: true,
    lastChecked: now - 20_000,
    lastLatencyMs: 88,
    lastCause: null,
    uptimeDay: 0.9965,
    uptimeWeek: 0.9991,
    uptimeMonth: 0.9996,
    p95Ms: 142,
    openIncident: null,
  };
}

export function mockUptimeDetail(hostname: string, path: string | null, range: AnalyticsRange) {
  const now = Date.now();
  const [total] = SECONDS[range];
  const width = (total * 1000) / 90;
  const bars: UptimeBar[] = Array.from({ length: 90 }, (_, i) => {
    const start = now - total * 1000 + i * width;
    // The app wasn't running for a while at the start of the range.
    const checks = i < 8 ? 0 : Math.max(1, Math.round(width / 60_000));
    const up = i === 61 ? Math.max(0, checks - 4) : checks;
    return { start, end: start + width, checks, up };
  });
  const at: number[] = [];
  const ms: (number | null)[] = [];
  for (let i = 0; i < 120; i++) {
    at.push(now - (120 - i) * 60_000);
    ms.push(i === 70 ? null : 80 + Math.round(30 * Math.abs(Math.sin(i / 7))));
  }
  const startedAt = now - total * 1000 * 0.32;
  return {
    summary: mockUptimeSummary(hostname, path),
    bars,
    latency: { at, ms },
    incidents: [
      {
        id: 1,
        accountId: "acc-personal",
        route: { hostname, path },
        startedAt,
        endedAt: startedAt + 4 * 60_000,
        cause: "originUnreachable" as const,
      },
    ],
  };
}

export let mockAlertRules: AlertRules = {
  routeDown: true,
  downAfter: 3,
  recovered: true,
  errorRate: true,
  errorRatePercent: 5,
  errorRateMinutes: 10,
  minRequests: 20,
  latency: false,
  latencyMs: 3000,
  latencyMinutes: 10,
  connectorDown: true,
  muted: [],
};

/** Answers the analytics commands, or `undefined` for any other command. */
export function analyticsMock(cmd: string, payload: Record<string, unknown>): unknown {
  const range = (payload["range"] as AnalyticsRange | undefined) ?? "day";
  switch (cmd) {
    case "analytics_summary":
      return mockSummary(payload["hostnames"] as string[], range);
    case "analytics_route":
      return mockRouteStats(
        String(payload["hostname"]),
        (payload["path"] as string) ?? null,
        range,
      );
    case "uptime_list":
      return [
        mockUptimeSummary("app.teispace.com", null),
        mockUptimeSummary("teispace.dev", null),
        mockUptimeSummary("api.teispace.dev", "/v1/"),
      ];
    case "uptime_route":
      return mockUptimeDetail(String(payload["hostname"]), null, range);
    case "alerts_get":
      return mockAlertRules;
    case "alerts_set":
      mockAlertRules = payload["rules"] as AlertRules;
      return mockAlertRules;
    default:
      return undefined;
  }
}
