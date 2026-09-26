import { type MessageKey, t } from "@/lib/i18n";
import type {
  AnalyticsRange,
  AnalyticsSummary,
  RouteView,
  StatsPart,
  StatsSeries,
  UptimeSummary,
} from "@/lib/ipc/bindings";
import type { Columns } from "@/lib/traffic";

export const RANGES: readonly AnalyticsRange[] = ["hour", "day", "week", "month"];

export const rangeOptions = () =>
  RANGES.map((value) => ({ value, label: t(`analytics.range.${value}`) }));

export const rangeNoun = (range: AnalyticsRange): string => t(`analytics.noun.${range}`);

const clock = new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" });
const weekday = new Intl.DateTimeFormat(undefined, { weekday: "short" });
const date = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" });
const dayTime = new Intl.DateTimeFormat(undefined, {
  weekday: "short",
  hour: "numeric",
  minute: "2-digit",
});

/** Axis ticks and the hovered time, per range (seconds since the epoch). */
export const TICKS: Record<
  AnalyticsRange,
  { tick: Intl.DateTimeFormat; time: Intl.DateTimeFormat }
> = {
  hour: { tick: clock, time: clock },
  day: { tick: clock, time: dayTime },
  week: { tick: weekday, time: dayTime },
  month: { tick: date, time: date },
};

/**
 * The literal path a path rule's regex starts with, like the core (`^/api/.*` → `/api/`),
 * or null. `/` counts as no path.
 */
export function pathPrefix(rule: string | null): string | null {
  if (!rule?.startsWith("^")) return null;
  const match = /^[^.*+?()[\]{}|$\\^]*/.exec(rule.slice(1));
  const prefix = match?.[0] ?? "";
  return prefix.startsWith("/") && prefix !== "/" ? prefix : null;
}

/** The key uptime uses for a route: `hostname` or `hostname/path`. */
export function routeKey(hostname: string, pathRule: string | null): string {
  return `${hostname}${pathPrefix(pathRule) ?? ""}`;
}

const compact = new Intl.NumberFormat(undefined, { notation: "compact", maximumFractionDigits: 1 });

/** 1234 → "1.2K". */
export const formatCount = (n: number): string => compact.format(n);

/** A share (0–1) as a percentage: "100%", "99.9%", "0.4%", "<0.1%", or "–". */
export function formatPercent(share: number | null | undefined): string {
  if (share === null || share === undefined || !Number.isFinite(share)) return "–";
  const percent = share * 100;
  if (percent === 0) return "0%";
  if (percent < 0.1) return "<0.1%";
  // 99.96% is not 100%: never round a partial outage away.
  if (percent < 100 && percent >= 99.95) return "99.9%";
  return `${Number(percent.toFixed(percent >= 10 ? 1 : 2)).toString()}%`;
}

const rateFormats = [0, 1, 2].map(
  (digits) => new Intl.NumberFormat(undefined, { maximumFractionDigits: digits }),
);

/** Requests per second: "12", "3.4", "0.05", "<0.01" (two significant digits below 10). */
export function formatRate(perSecond: number | null): string {
  if (perSecond === null || !Number.isFinite(perSecond) || perSecond <= 0) return "0";
  if (perSecond < 0.01) return `<${rateFormats[2]?.format(0.01)}`;
  const digits = perSecond >= 10 ? 0 : perSecond >= 1 ? 1 : 2;
  return rateFormats[digits]?.format(perSecond) ?? String(perSecond);
}

export function formatMs(ms: number | null | undefined): string {
  if (ms === null || ms === undefined) return "–";
  return ms >= 10_000 ? `${(ms / 1000).toFixed(1)} s` : `${Math.round(ms)} ms`;
}

export function formatBytes(n: number): string {
  const units = ["B", "kB", "MB", "GB", "TB"];
  let value = n;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit++;
  }
  return `${unit === 0 ? value : value.toFixed(1)} ${units[unit]}`;
}

export function uptimeFor(summary: UptimeSummary | undefined, range: AnalyticsRange) {
  if (!summary) return null;
  if (range === "week") return summary.uptimeWeek;
  if (range === "month") return summary.uptimeMonth;
  return summary.uptimeDay;
}

/** "Not on this domain's plan: status codes, response times." or null. */
export function unavailableNote(parts: readonly StatsPart[]): string | null {
  if (parts.length === 0) return null;
  const names = parts.map((p) => t(`analytics.part.${p}` as MessageKey));
  return t("analytics.notOnPlan", { parts: names.join(", ") });
}

/** A series as chart columns: seconds, requests, 5xx (5xx only where there were some). */
export function chartColumns(series: StatsSeries): Columns {
  return [
    series.at.map((at) => at / 1000),
    series.requests.map((n) => n),
    series.serverErrors.map((n) => (n === 0 ? null : n)),
  ];
}

export interface Row {
  key: string;
  hostname: string;
  path: string | null;
  local: boolean;
  requests: number | null;
  errorRate: number | null;
  p95: number | null;
  uptime: number | null;
  spark: number[];
  up: boolean | null;
}

export type SortColumn = "route" | "requests" | "errors" | "p95" | "uptime";
export type SortDirection = "ascending" | "descending";

/** One row per route: edge numbers by hostname, uptime by route. */
export function buildRows(
  routes: readonly RouteView[],
  summary: AnalyticsSummary | undefined,
  uptimes: readonly UptimeSummary[],
  range: AnalyticsRange,
): Row[] {
  return routes
    .filter((route) => route.client === null)
    .map((route) => {
      const key = routeKey(route.hostname, route.path);
      const host = summary?.hosts.find((h) => h.hostname === route.hostname);
      const uptime = uptimes.find(
        (u) => `${u.route.hostname}${u.route.path ?? ""}` === key.toLowerCase(),
      );
      return {
        key,
        hostname: route.hostname,
        path: route.path,
        local: route.local,
        requests: host ? host.requests : null,
        errorRate: host?.errorRate ?? null,
        p95: host?.p95Ms ?? null,
        uptime: uptimeFor(uptime, range),
        spark: host?.spark ?? [],
        up: uptime?.up ?? null,
      };
    });
}

const value = (row: Row, column: SortColumn): number | string | null => {
  switch (column) {
    case "route":
      return row.key;
    case "requests":
      return row.requests;
    case "errors":
      return row.errorRate;
    case "p95":
      return row.p95;
    case "uptime":
      return row.uptime;
  }
};

/** Sorted copy; rows without a value go last whatever the direction. */
export function sortRows(rows: readonly Row[], column: SortColumn, direction: SortDirection) {
  const sign = direction === "ascending" ? 1 : -1;
  return [...rows].sort((a, b) => {
    const [x, y] = [value(a, column), value(b, column)];
    if (x === null && y === null) return a.key.localeCompare(b.key);
    if (x === null) return 1;
    if (y === null) return -1;
    const order = typeof x === "string" ? x.localeCompare(String(y)) : x - Number(y);
    return order === 0 ? a.key.localeCompare(b.key) : order * sign;
  });
}
