import { useMemo } from "react";
import { type ChartSeries, TimeSeriesChart } from "@/components/patterns/time-series-chart";
import { Skeleton } from "@/components/ui/skeleton";
import { StatusDot } from "@/components/ui/status-dot";
import { formatDuration, relativeTime } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { AnalyticsRange, Incident } from "@/lib/ipc/bindings";
import { formatMs, formatPercent, rangeNoun, TICKS, uptimeFor } from "../model";
import { useUptimeRoute } from "../queries";
import { UptimeStrip } from "./uptime-strip";

let latencySeries: ChartSeries[] | undefined;
const latencySeriesOf = (): ChartSeries[] => {
  latencySeries ??= [
    { label: t("uptime.responseTime"), tone: "secondary", format: (v) => formatMs(v) },
  ];
  return latencySeries;
};

function IncidentRow({ incident }: { incident: Incident }) {
  const cause = t(`uptime.cause.${incident.cause}`);
  return (
    <li className="flex items-start gap-2 text-callout">
      <StatusDot
        status={incident.endedAt === null ? "error" : "idle"}
        className="mt-[3px]"
        label={incident.endedAt === null ? t("uptime.down") : t("uptime.up")}
      />
      <span className="min-w-0 flex-1">
        <span className="block">{cause}</span>
        <span className="block text-secondary tabular">
          {incident.endedAt === null
            ? t("uptime.ongoing", { time: relativeTime(incident.startedAt) })
            : t("uptime.lasted", {
                time: relativeTime(incident.startedAt),
                duration: formatDuration(incident.endedAt - incident.startedAt),
              })}
        </span>
      </span>
    </li>
  );
}

/** A route's uptime from this machine's checks: strip, response time and incidents. */
export function UptimeSection({
  hostname,
  path,
  range,
}: {
  hostname: string;
  path: string | null;
  range: AnalyticsRange;
}) {
  const detail = useUptimeRoute(hostname, path, range);
  const period = rangeNoun(range);
  const chart = useMemo(() => {
    const latency = detail.data?.latency;
    if (!latency || latency.at.length < 2) return null;
    return [latency.at.map((at) => at / 1000), latency.ms] as [number[], (number | null)[]];
  }, [detail.data]);

  if (detail.isPending) return <Skeleton className="h-12" />;
  const data = detail.data;
  if (!data) return null;
  const share = uptimeFor(data.summary, range);
  if (data.summary.lastChecked === null) {
    return <p className="text-callout text-secondary">{t("uptime.notChecked")}</p>;
  }
  const now = Date.now() / 1000;
  const seconds = { hour: 3_600, day: 86_400, week: 604_800, month: 2_592_000 }[range];
  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-baseline justify-between gap-2">
        <span className="text-headline tabular">
          {t("uptime.value", { percent: formatPercent(share), period })}
        </span>
        {data.summary.p95Ms !== null ? (
          <span className="text-callout text-secondary tabular">
            P95 {formatMs(data.summary.p95Ms)}
          </span>
        ) : null}
      </div>
      <UptimeStrip
        bars={data.bars}
        label={t("uptime.strip", { period, count: data.bars.length })}
      />
      {chart ? (
        <TimeSeriesChart
          label={t("uptime.responseChart", { period })}
          data={chart}
          series={latencySeriesOf()}
          xRange={[now - seconds, now]}
          formatTick={(s) => TICKS[range].tick.format(s * 1000)}
          formatTime={(s) => TICKS[range].time.format(s * 1000)}
          formatValue={(v) => `${Math.round(v)}`}
          minMax={100}
          height={72}
        />
      ) : null}
      <div className="flex flex-col gap-1.5">
        <h4 className="text-callout text-secondary">{t("uptime.incidents")}</h4>
        {data.incidents.length === 0 ? (
          <p className="text-callout text-secondary">{t("uptime.noIncidents", { period })}</p>
        ) : (
          <ul className="flex flex-col gap-1.5">
            {data.incidents.slice(0, 5).map((incident) => (
              <IncidentRow key={incident.id} incident={incident} />
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
