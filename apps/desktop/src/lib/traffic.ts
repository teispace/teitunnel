import type { TrafficSeries } from "@/lib/ipc/bindings";

/** Samples kept client-side, matching the backend's ring buffer (an hour at 1 s). */
export const LIVE_CAPACITY = 3_600;

const COLUMNS = [
  "at",
  "span",
  "requests",
  "errors",
  "ok",
  "redirects",
  "clientErrors",
  "serverErrors",
  "concurrent",
  "connections",
  "rttMs",
] as const satisfies readonly (keyof TrafficSeries)[];

export const emptySeries = (): TrafficSeries => ({
  at: [],
  span: [],
  requests: [],
  errors: [],
  ok: [],
  redirects: [],
  clientErrors: [],
  serverErrors: [],
  concurrent: [],
  connections: [],
  rttMs: [],
});

/** `older` followed by the samples of `newer` after its last one, capped to `capacity`. */
export function appendSeries(
  older: TrafficSeries,
  newer: TrafficSeries,
  capacity = LIVE_CAPACITY,
): TrafficSeries {
  const last = older.at.at(-1) ?? Number.NEGATIVE_INFINITY;
  const from = newer.at.findIndex((at) => at > last);
  if (from === -1) return older;
  const total = older.at.length + newer.at.length - from;
  const drop = Math.max(0, total - capacity);
  const merged = {} as Record<(typeof COLUMNS)[number], unknown[]>;
  for (const key of COLUMNS) {
    merged[key] = [...older[key].slice(drop), ...newer[key].slice(from)];
  }
  return merged as unknown as TrafficSeries;
}

/** Per-second rate of `counts`, `null` where an interval has no length. */
export function perSecond(counts: readonly number[], span: readonly number[]): (number | null)[] {
  return counts.map((n, i) => {
    const seconds = span[i] ?? 0;
    return seconds > 0 ? n / seconds : null;
  });
}

/** A chart-ready column set: x in seconds, then one array per y series. */
export type Columns = [xs: number[], ...ys: (number | null)[][]];

/**
 * Converts ms timestamps to seconds and breaks the line wherever samples are further
 * apart than `maxGapSeconds` (the connector wasn't running, or the Mac slept), so a
 * chart never draws a straight line across missing time.
 */
export function withGaps(
  at: readonly number[],
  ys: readonly (readonly (number | null)[])[],
  maxGapSeconds: number,
): Columns {
  const xs: number[] = [];
  const out: (number | null)[][] = ys.map(() => []);
  for (let i = 0; i < at.length; i++) {
    const x = (at[i] ?? 0) / 1000;
    const previous = xs.at(-1);
    if (previous !== undefined && x - previous > maxGapSeconds) {
      xs.push(previous + maxGapSeconds / 2);
      for (const column of out) column.push(null);
    }
    xs.push(x);
    ys.forEach((y, series) => {
      out[series]?.push(y[i] ?? null);
    });
  }
  return [xs, ...out];
}

/** Requests per second over the samples of the last `seconds` (null without samples). */
export function recentRate(series: TrafficSeries, seconds: number, nowMs = Date.now()) {
  let requests = 0;
  let span = 0;
  for (let i = series.at.length - 1; i >= 0; i--) {
    if ((series.at[i] ?? 0) < nowMs - seconds * 1000) break;
    requests += series.requests[i] ?? 0;
    span += series.span[i] ?? 0;
  }
  return span > 0 ? requests / span : null;
}

export interface ClassShare {
  label: string;
  share: number;
}

/** Response classes with their share of all responses, largest first; empty without any. */
export function classShares(series: TrafficSeries): ClassShare[] {
  const sum = (column: readonly number[]) => column.reduce((a, b) => a + b, 0);
  const classes = [
    { label: "2xx", count: sum(series.ok) },
    { label: "3xx", count: sum(series.redirects) },
    { label: "4xx", count: sum(series.clientErrors) },
    { label: "5xx", count: sum(series.serverErrors) },
  ];
  const total = classes.reduce((a, c) => a + c.count, 0);
  if (total === 0) return [];
  return classes
    .filter((c) => c.count > 0)
    .map((c) => ({ label: c.label, share: c.count / total }))
    .sort((a, b) => b.share - a.share);
}

const compact = new Intl.NumberFormat(undefined, {
  notation: "compact",
  maximumFractionDigits: 1,
});

/** A rate for axes and readouts: "0", "0.25", "12", "1.2K". */
export function formatRate(value: number | null | undefined): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return "–";
  if (value === 0) return "0";
  if (value < 1) return value.toFixed(2).replace(/0$/, "");
  if (value < 100) return (Math.round(value * 10) / 10).toString();
  return compact.format(value);
}

/** A share as a percentage: "98%", "0.4%", "<0.1%". */
export function formatShare(share: number): string {
  const percent = share * 100;
  if (percent > 0 && percent < 0.1) return "<0.1%";
  // Never round a partial share up to 100% while other classes show.
  if (percent < 100 && percent >= 99.5) return `${Math.min(99.9, Math.floor(percent * 10) / 10)}%`;
  return `${percent < 10 ? Number(percent.toFixed(1)) : Math.round(percent)}%`;
}
