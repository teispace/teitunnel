import { useMemo, useState } from "react";
import { InspectorSection } from "@/components/patterns/inspector";
import { KeyValueGrid } from "@/components/patterns/key-value-grid";
import { type ChartSeries, TimeSeriesChart } from "@/components/patterns/time-series-chart";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { type MessageKey, t } from "@/lib/i18n";
import type { HistoryRange, TrafficSeries } from "@/lib/ipc/bindings";
import {
  classShares,
  formatRate,
  formatShare,
  perSecond,
  recentRate,
  withGaps,
} from "@/lib/traffic";
import { useLiveTraffic, useTrafficHistory } from "./queries";

type Range = "hour" | HistoryRange;

const rangeOptions = () =>
  (["hour", "day", "week"] as const).map((value) => ({
    value,
    label: t(`traffic.range.${value}`),
  }));

interface RangeSpec {
  seconds: number;
  /** Samples further apart than this are drawn as a gap. */
  maxGap: number;
  tick: Intl.DateTimeFormat;
  time: Intl.DateTimeFormat;
  noun: MessageKey;
}

const clock = new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" });
const SPECS: Record<Range, RangeSpec> = {
  hour: {
    seconds: 3_600,
    maxGap: 25,
    tick: clock,
    time: new Intl.DateTimeFormat(undefined, { timeStyle: "medium" }),
    noun: "traffic.noun.hour",
  },
  day: {
    seconds: 86_400,
    maxGap: 450,
    tick: clock,
    time: clock,
    noun: "traffic.noun.day",
  },
  week: {
    seconds: 604_800,
    maxGap: 2_700,
    tick: new Intl.DateTimeFormat(undefined, { weekday: "short" }),
    time: new Intl.DateTimeFormat(undefined, {
      weekday: "short",
      hour: "numeric",
      minute: "2-digit",
    }),
    noun: "traffic.noun.week",
  },
};

const perSecondLabel = (v: number | null) => (v === null ? "–" : `${formatRate(v)}/s`);
// Built once, after the language is set (charts compare series by identity).
let rateSeries: ChartSeries[] | undefined;
let rttSeries: ChartSeries[] | undefined;
const rateSeriesOf = (): ChartSeries[] => {
  rateSeries ??= [
    { label: t("traffic.requests"), tone: "accent", fill: true, format: perSecondLabel },
    // Only drawn where requests failed: a red line along zero would read as trouble.
    {
      label: t("traffic.failed"),
      tone: "error",
      sparse: true,
      format: (v) => perSecondLabel(v ?? 0),
    },
  ];
  return rateSeries;
};
const rttSeriesOf = (): ChartSeries[] => {
  rttSeries ??= [
    {
      label: t("traffic.roundTrip"),
      tone: "secondary",
      format: (v) => (v === null ? "–" : `${Math.round(v)} ms`),
    },
  ];
  return rttSeries;
};

function useCharts(series: TrafficSeries | undefined, spec: RangeSpec) {
  return useMemo(() => {
    if (!series) return null;
    const rates = withGaps(
      series.at,
      [
        perSecond(series.requests, series.span),
        perSecond(series.errors, series.span).map((v) => (v === 0 ? null : v)),
      ],
      spec.maxGap,
    );
    const rtt = series.rttMs.some((v) => v !== null)
      ? withGaps(series.at, [series.rttMs], spec.maxGap)
      : null;
    return { rates, rtt };
  }, [series, spec]);
}

/** Live and historical traffic of this Mac's connector for a tunnel. */
export function TunnelTraffic({ tunnelId }: { tunnelId: string }) {
  const [range, setRange] = useState<Range>("hour");
  const live = useLiveTraffic(tunnelId);
  const history = useTrafficHistory(tunnelId, range === "hour" ? null : range);
  const spec = SPECS[range];
  const traffic = live.data;
  const series = range === "hour" ? traffic?.series : history.data;
  const charts = useCharts(series, spec);
  if (!traffic) return null;

  const end = Date.now() / 1000;
  const xRange = [end - spec.seconds, end] as const;
  const now = recentRate(traffic.series, 10);
  const shares = classShares(series ?? traffic.series);
  const formatTick = (s: number) => spec.tick.format(s * 1000);
  const formatTime = (s: number) => spec.time.format(s * 1000);

  return (
    <InspectorSection title={t("traffic.title")}>
      <SegmentedControl
        label={t("traffic.timeRange")}
        size="sm"
        segments={rangeOptions()}
        value={range}
        onValueChange={setRange}
      />
      {charts && series && series.at.length > 0 ? (
        <>
          <TimeSeriesChart
            label={t("traffic.rateChart", { period: t(spec.noun) })}
            data={charts.rates}
            series={rateSeriesOf()}
            xRange={xRange}
            formatTick={formatTick}
            formatTime={formatTime}
            formatValue={formatRate}
          />
          {charts.rtt ? (
            <TimeSeriesChart
              label={t("traffic.rttChart", { period: t(spec.noun) })}
              data={charts.rtt}
              series={rttSeriesOf()}
              xRange={xRange}
              formatTick={formatTick}
              formatTime={formatTime}
              formatValue={(v) => `${Math.round(v)}`}
              minMax={10}
              height={72}
            />
          ) : null}
        </>
      ) : (
        <p className="text-callout text-secondary">
          {range === "hour" ? t("traffic.emptyHour") : t("traffic.emptyHistory")}
        </p>
      )}
      <KeyValueGrid
        items={[
          {
            label: t("traffic.now"),
            value: now === null ? "–" : t("traffic.nowValue", { rate: formatRate(now) }),
          },
          ...(shares.length > 0
            ? [
                {
                  label: t("traffic.responses"),
                  value: shares.map((c) => `${c.label} ${formatShare(c.share)}`).join(" · "),
                },
              ]
            : []),
          {
            label: t("traffic.sinceStart"),
            value: t("traffic.sinceStartValue", {
              requests: traffic.totalRequests,
              failed: traffic.totalErrors,
            }),
          },
          { label: t("traffic.connections"), value: String(traffic.connections) },
          ...(traffic.locations.length > 0
            ? [{ label: t("traffic.edge"), value: traffic.locations.join(", ").toUpperCase() }]
            : []),
        ]}
      />
    </InspectorSection>
  );
}
