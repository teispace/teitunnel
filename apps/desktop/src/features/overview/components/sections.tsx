import { Link } from "@tanstack/react-router";
import {
  Activity,
  CircleCheck,
  Gauge,
  Globe,
  Radio,
  Share2,
  ShieldCheck,
  TriangleAlert,
  Waypoints,
} from "lucide-react";
import { useMemo } from "react";
import {
  Card,
  CardLink,
  DistributionBar,
  MetricList,
  type Segment,
  StatCard,
  StatGrid,
  type Tone,
} from "@/components/patterns/dashboard";
import { Sparkline } from "@/components/patterns/sparkline";
import { type ChartSeries, TimeSeriesChart } from "@/components/patterns/time-series-chart";
import { type Status, StatusDot } from "@/components/ui/status-dot";
import { routeStatus } from "@/features/routes/status";
import { cn } from "@/lib/cn";
import { formatDuration, relativeTime, stripScheme } from "@/lib/format";
import { t } from "@/lib/i18n";
import type {
  ActivityEntry,
  Issue,
  QuickShare,
  RoutesOverview,
  Traffic,
  UptimeSummary,
} from "@/lib/ipc/bindings";
import { formatRate, formatShare, perSecond, recentRate, withGaps } from "@/lib/traffic";
import { errorTone, lastRoundTrip, recentErrors, responseCounts, uptimeGlance } from "../model";

const HOUR = 3_600;

const shareDots: Record<QuickShare["status"]["status"], Status> = {
  live: "healthy",
  starting: "connecting",
  reconnecting: "warning",
  failed: "error",
};

type Routes = RoutesOverview["routes"];
type Tunnel = RoutesOverview["tunnel"];

/** Routes serving, and those that aren't (by their status). */
function routeCounts(routes: Routes, tunnel: Tunnel, issues: readonly Issue[]) {
  let live = 0;
  for (const route of routes) {
    if (routeStatus(route, tunnel, issues).dot === "healthy") live++;
  }
  return { live, total: routes.length };
}

/** The line at the top: everything's fine, or what isn't. */
export function StatusBanner({
  problems,
  warnings,
  connections,
  locations,
  account,
}: {
  problems: number;
  warnings: number;
  connections: number | null;
  locations: readonly string[];
  account: string | null;
}) {
  const tone: Tone = problems > 0 ? "error" : warnings > 0 ? "warning" : "healthy";
  const Icon = tone === "healthy" ? CircleCheck : TriangleAlert;
  const title =
    problems > 0
      ? t("overview.attention", { count: problems })
      : warnings > 0
        ? t("overview.warnings", { count: warnings })
        : t("overview.healthy");
  return (
    <Link
      to="/doctor"
      className={cn(
        "flex min-h-16 items-center gap-3.5 rounded-card border-hairline px-4 py-3 outline-offset-0 transition-colors",
        tone === "error" && "border-error/25 bg-error/8 hover:bg-error/12",
        tone === "warning" && "border-warning/25 bg-warning/8 hover:bg-warning/12",
        tone === "healthy" && "border-inset bg-surface-inset hover:bg-surface-hover",
      )}
    >
      <span
        className={cn(
          "flex size-9 shrink-0 items-center justify-center rounded-full",
          tone === "error" && "bg-error/15 text-error",
          tone === "warning" && "bg-warning/15 text-warning",
          tone === "healthy" && "bg-healthy/15 text-healthy",
        )}
      >
        <Icon aria-hidden className="size-4.5" strokeWidth={2} />
      </span>
      <div className="min-w-0 flex-1">
        <div className="truncate text-title3">{title}</div>
        <div className="truncate text-callout text-secondary">
          {tone === "healthy" ? t("overview.healthyDetail") : t("overview.openDoctor")}
        </div>
      </div>
      <div className="hidden shrink-0 items-center gap-2 @3xl:flex">
        {account ? <Chip icon={ShieldCheck}>{account}</Chip> : null}
        {connections !== null ? (
          <Chip icon={Radio} tone={connections > 0 ? "healthy" : "error"}>
            {connections > 0
              ? t("overview.connections", { count: connections })
              : t("overview.notConnected")}
          </Chip>
        ) : null}
        {locations.length > 0 ? (
          <Chip icon={Globe}>{locations.join(" · ").toUpperCase()}</Chip>
        ) : null}
      </div>
    </Link>
  );
}

function Chip({
  icon: Icon,
  tone = "neutral",
  children,
}: {
  icon: typeof Globe;
  tone?: Tone;
  children: string;
}) {
  return (
    <span className="inline-flex h-6 max-w-56 items-center gap-1.5 rounded-full bg-surface-control px-2.5 text-callout">
      <Icon
        aria-hidden
        className={cn(
          "size-3 shrink-0",
          tone === "healthy" ? "text-healthy" : tone === "error" ? "text-error" : "text-secondary",
        )}
        strokeWidth={2}
      />
      <span className="truncate">{children}</span>
    </span>
  );
}

/** The numbers that matter, one card each. */
export function OverviewStats({
  traffic,
  uptimes,
  accountId,
  routes,
  tunnel,
  issues,
  shares,
}: {
  traffic: Traffic | undefined;
  uptimes: readonly UptimeSummary[];
  accountId: string | null;
  routes: Routes;
  tunnel: Tunnel;
  issues: readonly Issue[];
  shares: readonly QuickShare[];
}) {
  const series = traffic && traffic.series.at.length >= 2 ? traffic.series : null;
  const rate = series ? recentRate(series, 60) : null;
  const errors = series ? recentErrors(series) : null;
  const rtt = traffic?.rttMs ?? (series ? lastRoundTrip(series) : null);
  const uptime = uptimeGlance(uptimes, accountId);
  const counts = routeCounts(routes, tunnel, issues);
  const liveShares = shares.filter((s) => s.status.status === "live").length;
  const rttValues = series ? series.rttMs.map((v) => v ?? 0) : [];

  return (
    <StatGrid>
      <StatCard
        icon={Activity}
        label={t("overview.traffic")}
        value={formatRate(rate)}
        unit={t("overview.requestsPerSecond")}
        detail={traffic ? t("overview.sinceStart", { count: traffic.totalRequests }) : "–"}
        tone="accent"
        to="/analytics"
      >
        {series ? (
          <Sparkline
            values={perSecond(series.requests, series.span).map((v) => v ?? 0)}
            label={t("overview.sparkline")}
            height={28}
          />
        ) : null}
      </StatCard>
      <StatCard
        icon={TriangleAlert}
        label={t("overview.errors")}
        value={
          !errors || errors.failed === 0
            ? t("overview.errorsNone")
            : formatShare(errors.failed / errors.requests)
        }
        detail={
          !errors || errors.requests === 0
            ? t("overview.errorsQuiet")
            : t("overview.errorsDetail", { count: errors.requests, failed: errors.failed })
        }
        tone={errorTone(errors)}
        to="/analytics"
      />
      <StatCard
        icon={Gauge}
        label={t("overview.roundTrip")}
        value={rtt === null ? "–" : Math.round(rtt)}
        unit={rtt === null ? undefined : t("overview.ms")}
        detail={t("overview.roundTripDetail")}
        to="/tunnels"
      >
        {rttValues.some((v) => v > 0) ? (
          <Sparkline values={rttValues} label={t("overview.roundTripChart")} height={28} />
        ) : null}
      </StatCard>
      <StatCard
        icon={CircleCheck}
        label={t("overview.uptime")}
        value={
          !uptime
            ? "–"
            : uptime.down.length > 0
              ? t("overview.uptimeDown", { count: uptime.down.length })
              : t("overview.uptimeAllUp")
        }
        detail={
          !uptime
            ? t("overview.uptimeNone")
            : uptime.down[0]
              ? uptime.down[0].route.hostname
              : uptime.lowestDay !== null
                ? t("overview.uptimeLowest", { percent: formatShare(uptime.lowestDay) })
                : ""
        }
        tone={uptime ? (uptime.down.length > 0 ? "error" : "healthy") : "neutral"}
        to="/analytics"
      />
      <StatCard
        icon={Waypoints}
        label={t("overview.routes")}
        value={counts.total === 0 ? "0" : t("overview.ofTotal", counts)}
        detail={
          counts.total === 0
            ? t("overview.routesNone")
            : counts.live === counts.total
              ? t("overview.routesAllLive")
              : t("overview.routesNeedAttention", { count: counts.total - counts.live })
        }
        tone={counts.total === 0 ? "neutral" : counts.live === counts.total ? "healthy" : "warning"}
        to="/routes"
      >
        {counts.total > 0 ? <Meter value={counts.live / counts.total} /> : null}
      </StatCard>
      <StatCard
        icon={Share2}
        label={t("overview.shares")}
        value={String(liveShares)}
        detail={
          shares.length > liveShares
            ? t("overview.sharesStarting", { count: shares.length - liveShares })
            : t("overview.sharesDetail")
        }
        to="/quick-share"
      />
    </StatGrid>
  );
}

function Meter({ value }: { value: number }) {
  return (
    <div className="h-1.5 w-full overflow-hidden rounded-full bg-surface-control">
      <div
        className={cn("h-full rounded-full", value >= 1 ? "bg-healthy" : "bg-warning")}
        style={{ width: `${Math.max(0, Math.min(1, value)) * 100}%` }}
      />
    </div>
  );
}

const perSecondLabel = (v: number | null) => (v === null ? "–" : `${formatRate(v)}/s`);
let chartSeries: ChartSeries[] | undefined;
const chartSeriesOf = (): ChartSeries[] => {
  chartSeries ??= [
    { label: t("traffic.requests"), tone: "accent", fill: true, format: perSecondLabel },
    {
      label: t("traffic.failed"),
      tone: "error",
      sparse: true,
      format: (v) => perSecondLabel(v ?? 0),
    },
  ];
  return chartSeries;
};
const clock = new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" });
const clockSeconds = new Intl.DateTimeFormat(undefined, { timeStyle: "medium" });

/** Requests over the last hour through this computer's connector. */
export function TrafficCard({ traffic }: { traffic: Traffic | undefined }) {
  const series = traffic?.series;
  const data = useMemo(
    () =>
      series && series.at.length > 1
        ? withGaps(
            series.at,
            [
              perSecond(series.requests, series.span),
              perSecond(series.errors, series.span).map((v) => (v === 0 ? null : v)),
            ],
            25,
          )
        : null,
    [series],
  );
  const end = Date.now() / 1000;
  return (
    <Card
      title={t("overview.trafficChart")}
      detail={t("overview.trafficChartDetail")}
      action={<CardLink to="/analytics">{t("overview.openAnalytics")}</CardLink>}
      className="@5xl:col-span-2"
    >
      {data ? (
        <TimeSeriesChart
          label={t("overview.trafficChartLabel")}
          data={data}
          series={chartSeriesOf()}
          xRange={[end - HOUR, end]}
          formatTick={(s) => clock.format(s * 1000)}
          formatTime={(s) => clockSeconds.format(s * 1000)}
          formatValue={formatRate}
          height={210}
        />
      ) : (
        <div className="flex h-[210px] items-center justify-center text-callout text-secondary">
          {t("traffic.emptyHour")}
        </div>
      )}
    </Card>
  );
}

/** How requests were answered over the last hour, and the connector's state. */
export function ResponsesCard({ traffic }: { traffic: Traffic | undefined }) {
  const counts = traffic ? responseCounts(traffic.series) : null;
  const segments: Segment[] = [
    { label: t("overview.class.ok"), value: counts?.ok ?? 0, tone: "healthy" },
    { label: t("overview.class.redirects"), value: counts?.redirects ?? 0, tone: "accent" },
    { label: t("overview.class.clientErrors"), value: counts?.clientErrors ?? 0, tone: "warning" },
    { label: t("overview.class.serverErrors"), value: counts?.serverErrors ?? 0, tone: "error" },
  ];
  const peak = traffic ? Math.max(0, ...traffic.series.concurrent) : null;
  return (
    <Card title={t("overview.responses")} detail={t("overview.responsesDetail")}>
      <div className="flex h-full flex-col gap-4">
        <DistributionBar label={t("overview.responses")} segments={segments} format={formatShare} />
        <MetricList
          items={[
            {
              label: t("overview.requestsTotal"),
              value: traffic ? traffic.totalRequests.toLocaleString() : "–",
            },
            {
              label: t("overview.failedTotal"),
              value: traffic ? traffic.totalErrors.toLocaleString() : "–",
            },
            { label: t("overview.peakInFlight"), value: peak === null ? "–" : String(peak) },
            {
              label: t("overview.edgeConnections"),
              value: traffic ? String(traffic.connections) : "–",
            },
          ]}
        />
      </div>
    </Card>
  );
}

/** The account's routes and how each is doing. */
export function RoutesCard({
  routes,
  tunnel,
  issues,
  limit = 8,
}: {
  routes: Routes;
  tunnel: Tunnel;
  issues: readonly Issue[];
  limit?: number;
}) {
  return (
    <Card
      title={t("overview.routes")}
      detail={t("overview.routesCount", { count: routes.length })}
      action={<CardLink to="/routes">{t("overview.showAll")}</CardLink>}
      bodyClassName="px-2"
    >
      {routes.length === 0 ? (
        <p className="px-2 py-6 text-center text-callout text-secondary">
          {t("overview.routesNone")}
        </p>
      ) : (
        <ul className="flex flex-col">
          {routes.slice(0, limit).map((route) => {
            const status = routeStatus(route, tunnel, issues);
            return (
              <li key={`${route.hostname}${route.path ?? ""}`}>
                <Link
                  to="/routes"
                  className="flex min-h-11 items-center gap-3 rounded-row px-2 py-1.5 outline-offset-0 hover:bg-surface-hover"
                >
                  <StatusDot status={status.dot} label={status.label} />
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-body">
                      {route.hostname}
                      {route.path ? <span className="text-secondary"> {route.path}</span> : null}
                    </div>
                    <div className="truncate font-mono text-[11px] leading-4 text-secondary">
                      {stripScheme(route.origin)}
                    </div>
                  </div>
                  <span className="max-w-[45%] truncate text-callout text-secondary">
                    {status.label}
                  </span>
                </Link>
              </li>
            );
          })}
        </ul>
      )}
    </Card>
  );
}

/** Quick Shares running now. */
export function SharesCard({ shares, now }: { shares: readonly QuickShare[]; now: number }) {
  return (
    <Card
      title={t("overview.shares")}
      detail={t("overview.sharesCount", { count: shares.length })}
      action={<CardLink to="/quick-share">{t("overview.showAll")}</CardLink>}
      bodyClassName="px-2"
    >
      {shares.length === 0 ? (
        <div className="flex flex-col items-center gap-2 px-2 py-6 text-center">
          <p className="text-callout text-secondary">{t("overview.sharesNone")}</p>
          <Link to="/quick-share" search={{ compose: true }} className="text-callout text-accent">
            {t("overview.empty.share")}
          </Link>
        </div>
      ) : (
        <ul className="flex flex-col">
          {shares.map((share) => (
            <li key={share.id}>
              <Link
                to="/quick-share"
                className="flex min-h-11 items-center gap-3 rounded-row px-2 py-1.5 outline-offset-0 hover:bg-surface-hover"
              >
                <StatusDot status={shareDots[share.status.status]} />
                <div className="min-w-0 flex-1">
                  <div className="truncate text-body">
                    {share.url ? stripScheme(share.url) : t("overview.gettingUrl")}
                  </div>
                  <div className="truncate font-mono text-[11px] leading-4 text-secondary">
                    {stripScheme(share.origin)}
                  </div>
                </div>
                <span className="text-callout text-secondary tabular">
                  {formatDuration(now - share.startedAt)}
                </span>
              </Link>
            </li>
          ))}
        </ul>
      )}
    </Card>
  );
}

const outcomeDot: Record<string, Status> = {
  applied: "healthy",
  rolledBack: "warning",
  partiallyApplied: "error",
  alert: "error",
};

/** The latest changes to the account (Activity). */
export function ActivityCard({
  entries,
  limit = 6,
}: {
  entries: readonly ActivityEntry[];
  limit?: number;
}) {
  return (
    <Card
      title={t("overview.activity")}
      action={<CardLink to="/activity">{t("overview.showAll")}</CardLink>}
      bodyClassName="px-2"
    >
      {entries.length === 0 ? (
        <p className="px-2 py-6 text-center text-callout text-secondary">
          {t("overview.activityNone")}
        </p>
      ) : (
        <ul className="flex flex-col">
          {entries.slice(0, limit).map((entry) => (
            <li key={entry.id}>
              <Link
                to="/activity"
                className="flex min-h-10 items-center gap-3 rounded-row px-2 py-1.5 outline-offset-0 hover:bg-surface-hover"
              >
                <StatusDot status={outcomeDot[entry.outcome] ?? "idle"} />
                <span className="min-w-0 flex-1 truncate text-body">{entry.summary}</span>
                <span className="shrink-0 text-callout text-secondary">
                  {relativeTime(entry.at)}
                </span>
              </Link>
            </li>
          ))}
        </ul>
      )}
    </Card>
  );
}
