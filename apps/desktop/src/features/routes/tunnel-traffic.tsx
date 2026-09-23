import { useMemo, useState } from "react";
import { InspectorSection } from "@/components/patterns/inspector";
import { KeyValueGrid } from "@/components/patterns/key-value-grid";
import { type ChartSeries, TimeSeriesChart } from "@/components/patterns/time-series-chart";
import { SegmentedControl } from "@/components/ui/segmented-control";
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

const RANGES = [
  { value: "hour", label: "Hour" },
  { value: "day", label: "Day" },
  { value: "week", label: "Week" },
] as const;

interface RangeSpec {
  seconds: number;
  /** Samples further apart than this are drawn as a gap. */
  maxGap: number;
  tick: Intl.DateTimeFormat;
  time: Intl.DateTimeFormat;
  noun: string;
}

const clock = new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" });
const SPECS: Record<Range, RangeSpec> = {
  hour: {
    seconds: 3_600,
    maxGap: 25,
    tick: clock,
    time: new Intl.DateTimeFormat(undefined, { timeStyle: "medium" }),
    noun: "the last hour",
  },
  day: {
    seconds: 86_400,
    maxGap: 450,
    tick: clock,
    time: clock,
    noun: "the last 24 hours",
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
    noun: "the last 7 days",
  },
};

const perSecondLabel = (v: number | null) => (v === null ? "–" : `${formatRate(v)}/s`);
const RATE_SERIES: ChartSeries[] = [
  { label: "Requests", tone: "accent", fill: true, format: perSecondLabel },
  // Only drawn where requests failed: a red line along zero would read as trouble.
  { label: "Failed", tone: "error", sparse: true, format: (v) => perSecondLabel(v ?? 0) },
];
const RTT_SERIES: ChartSeries[] = [
  {
    label: "Round trip",
    tone: "secondary",
    format: (v) => (v === null ? "–" : `${Math.round(v)} ms`),
  },
];

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
    <InspectorSection title="Traffic">
      <SegmentedControl
        label="Time range"
        size="sm"
        segments={RANGES}
        value={range}
        onValueChange={setRange}
      />
      {charts && series && series.at.length > 0 ? (
        <>
          <TimeSeriesChart
            label={`Requests per second over ${spec.noun}`}
            data={charts.rates}
            series={RATE_SERIES}
            xRange={xRange}
            formatTick={formatTick}
            formatTime={formatTime}
            formatValue={formatRate}
          />
          {charts.rtt ? (
            <TimeSeriesChart
              label={`Round trip to Cloudflare over ${spec.noun}`}
              data={charts.rtt}
              series={RTT_SERIES}
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
          {range === "hour"
            ? "Traffic appears here a few seconds after the connector starts."
            : "History appears here after the connector has run for a minute."}
        </p>
      )}
      <KeyValueGrid
        items={[
          {
            label: "Now",
            value: now === null ? "–" : `${formatRate(now)} requests/s`,
          },
          ...(shares.length > 0
            ? [
                {
                  label: "Responses",
                  value: shares.map((c) => `${c.label} ${formatShare(c.share)}`).join(" · "),
                },
              ]
            : []),
          {
            label: "Since start",
            value: `${traffic.totalRequests.toLocaleString()} requests, ${traffic.totalErrors.toLocaleString()} failed`,
          },
          { label: "Connections", value: String(traffic.connections) },
          ...(traffic.locations.length > 0
            ? [{ label: "Edge", value: traffic.locations.join(", ").toUpperCase() }]
            : []),
        ]}
      />
    </InspectorSection>
  );
}
