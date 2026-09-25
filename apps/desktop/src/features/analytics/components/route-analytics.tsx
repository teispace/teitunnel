import { useMemo, useState } from "react";
import { InspectorSection } from "@/components/patterns/inspector";
import { KeyValueGrid } from "@/components/patterns/key-value-grid";
import { type ChartSeries, TimeSeriesChart } from "@/components/patterns/time-series-chart";
import { Button } from "@/components/ui/button";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { Skeleton } from "@/components/ui/skeleton";
import { PermissionFix } from "@/features/accounts";
import { t } from "@/lib/i18n";
import type { AnalyticsRange, RouteStats, RouteView } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import {
  chartColumns,
  formatBytes,
  formatCount,
  formatMs,
  rangeNoun,
  rangeOptions,
  TICKS,
  unavailableNote,
} from "../model";
import { useRouteAnalytics } from "../queries";
import { RankedList } from "./ranked-list";
import { UptimeSection } from "./uptime-section";

let requestSeries: ChartSeries[] | undefined;
const requestSeriesOf = (): ChartSeries[] => {
  requestSeries ??= [
    {
      label: t("analytics.requests"),
      tone: "accent",
      fill: true,
      format: (v) => (v === null ? "–" : formatCount(v)),
    },
    {
      label: t("analytics.serverErrors"),
      tone: "error",
      sparse: true,
      format: (v) => formatCount(v ?? 0),
    },
  ];
  return requestSeries;
};

const SECONDS: Record<AnalyticsRange, number> = {
  hour: 3_600,
  day: 86_400,
  week: 604_800,
  month: 2_592_000,
};

function EdgeStats({ stats, range }: { stats: RouteStats; range: AnalyticsRange }) {
  const period = rangeNoun(range);
  const data = useMemo(() => chartColumns(stats.series), [stats.series]);
  const end = Date.now() / 1000;
  const c = stats.classes;
  const note = unavailableNote(stats.unavailable);
  const origin = stats.originMs;
  return (
    <>
      {stats.requests === 0 ? (
        <p className="text-callout text-secondary">{t("analytics.noTraffic", { period })}</p>
      ) : (
        <TimeSeriesChart
          label={t("analytics.requestsChart", { period })}
          data={data}
          series={requestSeriesOf()}
          xRange={[stats.availableFrom ? stats.availableFrom / 1000 : end - SECONDS[range], end]}
          formatTick={(s) => TICKS[range].tick.format(s * 1000)}
          formatTime={(s) => TICKS[range].time.format(s * 1000)}
          formatValue={formatCount}
        />
      )}
      <KeyValueGrid
        items={[
          { label: t("analytics.requests"), value: formatCount(stats.requests) },
          { label: t("analytics.sent"), value: formatBytes(stats.bytes) },
          {
            label: t("analytics.responses"),
            value: `2xx ${formatCount(c.ok)} · 3xx ${formatCount(c.redirects)} · 4xx ${formatCount(c.clientErrors)} · 5xx ${formatCount(c.serverErrors)}`,
          },
          ...(origin
            ? [
                {
                  label: t("analytics.originTime"),
                  value: t("analytics.originTimeValue", {
                    p50: formatMs(origin.p50),
                    p95: formatMs(origin.p95),
                    p99: formatMs(origin.p99),
                  }),
                },
              ]
            : []),
        ]}
      />
      <RankedList title={t("analytics.topPaths")} rows={stats.paths} mono />
      <RankedList title={t("analytics.countries")} rows={stats.countries} />
      <RankedList title={t("analytics.browsers")} rows={stats.browsers} />
      <RankedList title={t("analytics.bots")} rows={stats.bots} emptyKey={t("analytics.people")} />
      <RankedList title={t("analytics.cache")} rows={stats.cache} />
      {stats.availableFrom ? (
        <p className="text-footnote text-secondary">
          {t("analytics.shortHistory", {
            date: new Date(stats.availableFrom).toLocaleDateString(),
          })}
        </p>
      ) : null}
      {note ? <p className="text-footnote text-secondary">{note}</p> : null}
      <p className="text-footnote text-secondary">{t(`analytics.source.${stats.source}`)}</p>
    </>
  );
}

/** The route inspector's Analytics: uptime from this Mac, traffic from Cloudflare. */
export function RouteAnalytics({ accountId, route }: { accountId: string; route: RouteView }) {
  const [range, setRange] = useState<AnalyticsRange>("day");
  const stats = useRouteAnalytics(accountId, route.hostname, route.path, range);
  const error = stats.error ? toIpcError(stats.error) : null;
  return (
    <InspectorSection title={t("analytics.section")}>
      <SegmentedControl
        label={t("analytics.rangeLabel")}
        size="sm"
        segments={rangeOptions()}
        value={range}
        onValueChange={setRange}
      />
      {route.local ? (
        <UptimeSection hostname={route.hostname} path={route.path} range={range} />
      ) : null}
      {error?.code === "permissionDenied" ? (
        <PermissionFix
          accountId={accountId}
          needs={[{ kind: "analytics" }]}
          refused
          onReady={() => void stats.refetch()}
        />
      ) : error ? (
        <div role="alert" className="flex items-center gap-2">
          <p className="min-w-0 flex-1 text-callout text-secondary">
            {error.code === "unavailable" ? error.message : t("analytics.loadFailed")}
          </p>
          <Button size="sm" pending={stats.isFetching} onClick={() => void stats.refetch()}>
            {t("common.tryAgain")}
          </Button>
        </div>
      ) : stats.data ? (
        <EdgeStats stats={stats.data} range={range} />
      ) : (
        <div
          role="status"
          aria-busy
          aria-label={t("analytics.section")}
          className="flex flex-col gap-2"
        >
          <Skeleton className="h-28" />
          <Skeleton className="h-4 w-2/3" />
        </div>
      )}
    </InspectorSection>
  );
}
