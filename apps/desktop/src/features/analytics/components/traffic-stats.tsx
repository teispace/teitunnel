import { useMemo } from "react";
import { KeyValueGrid } from "@/components/patterns/key-value-grid";
import { type ChartSeries, TimeSeriesChart } from "@/components/patterns/time-series-chart";
import { t } from "@/lib/i18n";
import type { AnalyticsRange, RouteStats } from "@/lib/ipc/bindings";
import {
  chartColumns,
  formatBytes,
  formatCount,
  formatMs,
  formatRate,
  rangeNoun,
  TICKS,
  unavailableNote,
} from "../model";
import { RankedList } from "./ranked-list";

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

/**
 * A route's or share's traffic: requests over time, rates, answers, response times and
 * the top paths, countries, browsers and bots, from whichever source answered.
 */
export function TrafficStats({ stats, range }: { stats: RouteStats; range: AnalyticsRange }) {
  const period = rangeNoun(range);
  const data = useMemo(() => chartColumns(stats.series), [stats.series]);
  const end = Date.now() / 1000;
  const c = stats.classes;
  // Only the edge's missing parts are about the plan; the others' are just not measured.
  const note = stats.source === "edge" ? unavailableNote(stats.unavailable) : null;
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
          ...(stats.requests > 0
            ? [
                {
                  label: t("analytics.perSecond"),
                  value: t("analytics.perSecondValue", {
                    average: formatRate(stats.rate.average),
                    peak: formatRate(stats.rate.peak),
                  }),
                },
              ]
            : []),
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
      <RankedList
        title={stats.source === "edge" ? t("analytics.bots") : t("analytics.botsClaimed")}
        rows={stats.bots}
        emptyKey={t("analytics.people")}
      />
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
