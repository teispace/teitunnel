import type { TrafficSeries, UptimeSummary } from "@/lib/ipc/bindings";

/** How far back the Overview counts errors. */
export const ERROR_WINDOW_SECONDS = 300;

export interface RecentErrors {
  /** Requests in the window. */
  requests: number;
  /** Of those, 5xx answers and requests that never reached the service. */
  failed: number;
}

/** Failed requests among the samples of the last `seconds` (null without samples). */
export function recentErrors(
  series: TrafficSeries,
  seconds = ERROR_WINDOW_SECONDS,
  nowMs = Date.now(),
): RecentErrors | null {
  let requests = 0;
  let failed = 0;
  let samples = 0;
  for (let i = series.at.length - 1; i >= 0; i--) {
    if ((series.at[i] ?? 0) < nowMs - seconds * 1000) break;
    requests += series.requests[i] ?? 0;
    failed += (series.serverErrors[i] ?? 0) + (series.errors[i] ?? 0);
    samples++;
  }
  if (samples === 0) return null;
  // Unreachable requests can also be counted as 5xx by the connector: never over 100%.
  return { requests, failed: Math.min(failed, requests) };
}

export type Tone = "healthy" | "warning" | "error" | "neutral";

/** How worrying a failure share is: any is a warning, over 5% an error. */
export function errorTone(errors: RecentErrors | null): Tone {
  if (!errors || errors.requests === 0) return "neutral";
  if (errors.failed === 0) return "healthy";
  return errors.failed / errors.requests > 0.05 ? "error" : "warning";
}

export interface UptimeGlance {
  /** Routes with a check so far. */
  checked: number;
  /** Of those, down at their last check. */
  down: UptimeSummary[];
  /** The lowest 24-hour uptime among them (0–1). */
  lowestDay: number | null;
}

/** The account's routes' uptime at a glance (null before any route was checked). */
export function uptimeGlance(
  list: readonly UptimeSummary[],
  accountId: string | null,
): UptimeGlance | null {
  const checked = list.filter((u) => u.accountId === accountId && u.up !== null);
  if (checked.length === 0) return null;
  const days = checked.flatMap((u) => (u.uptimeDay === null ? [] : [u.uptimeDay]));
  return {
    checked: checked.length,
    down: checked.filter((u) => u.up === false),
    lowestDay: days.length > 0 ? Math.min(...days) : null,
  };
}
