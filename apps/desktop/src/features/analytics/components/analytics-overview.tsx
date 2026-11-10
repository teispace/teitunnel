import {
  Activity,
  CircleCheck,
  Gauge,
  HardDriveDownload,
  TriangleAlert,
  Trophy,
} from "lucide-react";
import { useMemo } from "react";
import { Card, StatCard, StatGrid } from "@/components/patterns/dashboard";
import { type ChartSeries, TimeSeriesChart } from "@/components/patterns/time-series-chart";
import { t } from "@/lib/i18n";
import type { AnalyticsRange, AnalyticsSummary } from "@/lib/ipc/bindings";
import type { Columns } from "@/lib/traffic";
import { formatBytes, formatCount, formatMs, formatPercent, type Row, TICKS } from "../model";

export interface Totals {
  requests: number;
  bytes: number;
  /** Share of 5xx answers over every route with a known rate. */
  errorRate: number | null;
  slowestP95: { hostname: string; ms: number } | null;
  lowestUptime: { key: string; share: number } | null;
  busiest: { key: string; requests: number } | null;
}

/** The account's numbers over the range, from the per-route rows and the summary. */
export function totals(rows: readonly Row[], summary: AnalyticsSummary | undefined): Totals {
  const hosts = summary?.hosts ?? [];
  const requests = hosts.reduce((sum, h) => sum + h.requests, 0);
  const bytes = hosts.reduce((sum, h) => sum + h.bytes, 0);
  let failed = 0;
  let counted = 0;
  for (const host of hosts) {
    if (host.errorRate === null) continue;
    failed += host.errorRate * host.requests;
    counted += host.requests;
  }
  let slowestP95: Totals["slowestP95"] = null;
  for (const host of hosts) {
    if (host.p95Ms !== null && (slowestP95 === null || host.p95Ms > slowestP95.ms)) {
      slowestP95 = { hostname: host.hostname, ms: host.p95Ms };
    }
  }
  let lowestUptime: Totals["lowestUptime"] = null;
  let busiest: Totals["busiest"] = null;
  for (const row of rows) {
    if (row.uptime !== null && (lowestUptime === null || row.uptime < lowestUptime.share)) {
      lowestUptime = { key: row.key, share: row.uptime };
    }
    if (
      row.requests !== null &&
      row.requests > 0 &&
      (busiest === null || row.requests > busiest.requests)
    ) {
      busiest = { key: row.key, requests: row.requests };
    }
  }
  return {
    requests,
    bytes,
    errorRate: counted > 0 ? failed / counted : null,
    slowestP95,
    lowestUptime,
    busiest,
  };
}

/** Requests and 5xx answers of every route, bucket by bucket (seconds on x). */
export function combinedSeries(summary: AnalyticsSummary): Columns | null {
  const buckets = Math.max(0, ...summary.hosts.map((h) => h.spark.length));
  if (buckets < 2) return null;
  const xs: number[] = [];
  const requests: (number | null)[] = [];
  const errors: (number | null)[] = [];
  const end = summary.endsAt / 1000;
  for (let i = 0; i < buckets; i++) {
    xs.push(end - (buckets - 1 - i) * summary.bucketSeconds);
    let r = 0;
    let e = 0;
    for (const host of summary.hosts) {
      // Sparklines are aligned at their end.
      const offset = buckets - host.spark.length;
      r += host.spark[i - offset] ?? 0;
      e += host.sparkErrors[i - offset] ?? 0;
    }
    requests.push(r);
    errors.push(e === 0 ? null : e);
  }
  return [xs, requests, errors];
}

let seriesCache: ChartSeries[] | undefined;
const chartSeries = (): ChartSeries[] => {
  seriesCache ??= [
    {
      label: t("analytics.chart.requests"),
      tone: "accent",
      fill: true,
      format: (v) => (v === null ? "–" : formatCount(v)),
    },
    {
      label: t("analytics.chart.errors"),
      tone: "error",
      sparse: true,
      format: (v) => formatCount(v ?? 0),
    },
  ];
  return seriesCache;
};

/** The account at a glance over the range: totals, the traffic chart and the top routes. */
export function AnalyticsOverview({
  rows,
  summary,
  range,
  loading,
}: {
  rows: readonly Row[];
  summary: AnalyticsSummary | undefined;
  range: AnalyticsRange;
  loading: boolean;
}) {
  const sum = useMemo(() => totals(rows, summary), [rows, summary]);
  const data = useMemo(() => (summary ? combinedSeries(summary) : null), [summary]);
  const ranked = useMemo(
    () =>
      rows
        .filter((r) => r.requests !== null && r.requests > 0)
        .sort((a, b) => (b.requests ?? 0) - (a.requests ?? 0))
        .slice(0, 6),
    [rows],
  );
  const known = !loading && summary !== undefined;
  const spec = TICKS[range];

  return (
    <>
      <StatGrid>
        <StatCard
          icon={Activity}
          label={t("analytics.kpi.requests")}
          value={known ? formatCount(sum.requests) : "–"}
          detail={t("analytics.kpi.requestsDetail", { count: rows.length })}
          tone="accent"
        />
        <StatCard
          icon={HardDriveDownload}
          label={t("analytics.kpi.bandwidth")}
          value={known ? formatBytes(sum.bytes) : "–"}
          detail={t("analytics.kpi.bandwidthDetail")}
        />
        <StatCard
          icon={TriangleAlert}
          label={t("analytics.kpi.errorRate")}
          value={formatPercent(sum.errorRate)}
          detail={t("analytics.kpi.errorRateDetail")}
          tone={
            sum.errorRate === null
              ? "neutral"
              : sum.errorRate > 0.05
                ? "error"
                : sum.errorRate > 0
                  ? "warning"
                  : "healthy"
          }
        />
        <StatCard
          icon={Gauge}
          label={t("analytics.kpi.p95")}
          value={sum.slowestP95 ? formatMs(sum.slowestP95.ms) : "–"}
          detail={sum.slowestP95?.hostname ?? t("analytics.kpi.p95Detail")}
        />
        <StatCard
          icon={CircleCheck}
          label={t("analytics.kpi.uptime")}
          value={sum.lowestUptime ? formatPercent(sum.lowestUptime.share) : "–"}
          detail={sum.lowestUptime?.key ?? t("analytics.kpi.uptimeDetail")}
          tone={
            sum.lowestUptime === null
              ? "neutral"
              : sum.lowestUptime.share < 0.99
                ? "warning"
                : "healthy"
          }
        />
        <StatCard
          icon={Trophy}
          label={t("analytics.kpi.busiest")}
          value={sum.busiest ? formatCount(sum.busiest.requests) : "–"}
          detail={sum.busiest?.key ?? t("analytics.kpi.busiestNone")}
        />
      </StatGrid>
      <div className="grid gap-3 @5xl:grid-cols-3">
        <Card
          title={t("analytics.chart.title")}
          detail={t("analytics.chart.detail")}
          className="@5xl:col-span-2"
        >
          {data ? (
            <TimeSeriesChart
              label={t("analytics.chart.label")}
              data={data}
              series={chartSeries()}
              formatTick={(s) => spec.tick.format(s * 1000)}
              formatTime={(s) => spec.time.format(s * 1000)}
              formatValue={formatCount}
              height={220}
            />
          ) : (
            <div className="skeleton h-[220px] rounded-control" aria-hidden />
          )}
        </Card>
        <Card title={t("analytics.top.title")} detail={t("analytics.top.detail")}>
          {ranked.length === 0 ? (
            <p className="py-8 text-center text-callout text-secondary">
              {known ? t("analytics.kpi.busiestNone") : ""}
            </p>
          ) : (
            <ul className="flex flex-col gap-2.5 pt-1">
              {ranked.map((row) => {
                const share = sum.requests > 0 ? (row.requests ?? 0) / sum.requests : 0;
                return (
                  <li key={row.key} className="flex flex-col gap-1">
                    <div className="flex items-baseline justify-between gap-3 text-callout">
                      <span className="min-w-0 truncate">{row.key}</span>
                      <span className="shrink-0 text-secondary tabular">
                        {formatCount(row.requests ?? 0)} · {formatPercent(share)}
                      </span>
                    </div>
                    <div className="h-1.5 overflow-hidden rounded-full bg-surface-control">
                      <div
                        className="h-full rounded-full bg-accent"
                        style={{ width: `${share * 100}%` }}
                      />
                    </div>
                  </li>
                );
              })}
            </ul>
          )}
        </Card>
      </div>
    </>
  );
}
