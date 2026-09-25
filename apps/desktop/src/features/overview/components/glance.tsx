import { Link } from "@tanstack/react-router";
import type { ReactNode } from "react";
import { Sparkline } from "@/components/patterns/sparkline";
import { useUptimeList } from "@/features/analytics";
import { useLiveTraffic } from "@/features/routes";
import { cn } from "@/lib/cn";
import { t } from "@/lib/i18n";
import { formatRate, formatShare, perSecond, recentRate } from "@/lib/traffic";
import { errorTone, recentErrors, type Tone, uptimeGlance } from "../model";

const toneText: Record<Tone, string> = {
  healthy: "text-primary",
  warning: "text-warning",
  error: "text-error",
  neutral: "text-primary",
};

function Tile({
  label,
  value,
  detail,
  tone = "neutral",
  children,
}: {
  label: string;
  value: ReactNode;
  detail: ReactNode;
  tone?: Tone;
  children?: ReactNode;
}) {
  return (
    <Link
      to="/analytics"
      className="flex min-w-0 flex-col rounded-card bg-surface-inset px-3 py-2.5 outline-offset-0"
    >
      <span className="text-callout text-secondary">{label}</span>
      <span className={cn("truncate tabular text-title3", toneText[tone])}>{value}</span>
      <span className="truncate tabular text-callout text-secondary">{detail}</span>
      {children ? <div className="mt-1.5">{children}</div> : null}
    </Link>
  );
}

interface GlanceProps {
  /** The tunnel this computer runs for the account (traffic and errors come from it). */
  tunnelId: string | null;
  accountId: string | null;
}

/**
 * Traffic, errors and uptime at a glance, each opening Analytics. Traffic and errors are
 * this computer's connector's (every 10 s: enough for a glance, and the connector stays
 * on its idle sampling rate); uptime is the routes' checks through Cloudflare.
 */
export function Glance({ tunnelId, accountId }: GlanceProps) {
  const traffic = useLiveTraffic(tunnelId, 10_000).data;
  const uptimes = useUptimeList().data;
  const series = traffic && traffic.series.at.length >= 2 ? traffic.series : null;
  const uptime = uptimeGlance(uptimes ?? [], accountId);
  if (!series && !uptime) return null;

  const rate = series ? recentRate(series, 60) : null;
  const errors = series ? recentErrors(series) : null;
  const firstDown = uptime?.down[0];

  return (
    <div className="grid auto-cols-fr grid-flow-col gap-2.5">
      {series && traffic ? (
        <Tile
          label={t("overview.traffic")}
          value={
            <>
              {formatRate(rate)}
              <span className="text-callout text-secondary">
                {" "}
                {t("overview.requestsPerSecond")}
              </span>
            </>
          }
          detail={t("overview.sinceStart", { count: traffic.totalRequests })}
        >
          <Sparkline
            values={perSecond(series.requests, series.span).map((v) => v ?? 0)}
            label={t("overview.sparkline")}
            height={28}
          />
        </Tile>
      ) : null}
      {series ? (
        <Tile
          label={t("overview.errors")}
          tone={errorTone(errors)}
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
        />
      ) : null}
      {uptime ? (
        <Tile
          label={t("overview.uptime")}
          tone={uptime.down.length > 0 ? "error" : "neutral"}
          value={
            uptime.down.length > 0
              ? t("overview.uptimeDown", { count: uptime.down.length })
              : t("overview.uptimeAllUp")
          }
          detail={
            firstDown
              ? uptime.down.length > 1
                ? t("overview.uptimeDownMore", {
                    hostname: firstDown.route.hostname,
                    count: uptime.down.length - 1,
                  })
                : firstDown.route.hostname
              : uptime.lowestDay !== null
                ? t("overview.uptimeLowest", { percent: formatShare(uptime.lowestDay) })
                : ""
          }
        />
      ) : null}
    </div>
  );
}
