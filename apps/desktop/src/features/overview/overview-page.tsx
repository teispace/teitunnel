import { Link } from "@tanstack/react-router";
import { ChevronRight, LayoutGrid, TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";
import { EmptyState } from "@/components/patterns/empty-state";
import { Sparkline } from "@/components/patterns/sparkline";
import { TitlebarToolbar } from "@/components/patterns/titlebar-toolbar";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { type Status, StatusDot } from "@/components/ui/status-dot";
import { ConnectSheet, useAccounts, useActiveAccount } from "@/features/accounts";
import { BinaryNotice, binaryReady, useBinaryStatus } from "@/features/binary";
import { useIssues } from "@/features/doctor/queries";
import { useQuickShares } from "@/features/quick-share";
import { useLiveTraffic } from "@/features/routes";
import { useRoutesOverview } from "@/features/routes/queries";
import { routeStatus } from "@/features/routes/status";
import { CliOffer } from "@/features/settings";
import { formatDuration, stripScheme } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { QuickShare } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { formatRate, perSecond, recentRate } from "@/lib/traffic";
import { useNow } from "@/lib/use-now";

const dots: Record<QuickShare["status"]["status"], Status> = {
  live: "healthy",
  starting: "connecting",
  reconnecting: "warning",
  failed: "error",
};

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-2.5">
      <h2 className="px-2.5 text-headline">{title}</h2>
      <ul className="flex flex-col divide-y-(length:--hairline) divide-inset rounded-card bg-surface-inset px-2.5">
        {children}
      </ul>
    </section>
  );
}

/** What's running right now, at a glance. */
export function OverviewPage() {
  const sharesQuery = useQuickShares();
  const shares = sharesQuery.data ?? [];
  const binary = useBinaryStatus();
  const accounts = useAccounts();
  const now = useNow();
  const active = useActiveAccount();
  const overview = useRoutesOverview(active?.id ?? null);
  // Until the first answers are in, "nothing running" would be a guess.
  const loading =
    sharesQuery.isPending || accounts.isPending || (active !== null && overview.isPending);
  const routes = overview.data?.routes ?? [];
  const tunnel = overview.data?.tunnel ?? null;
  const { issues } = useIssues();
  const problems = issues.filter((i) => i.severity === "error").length;

  if (shares.length === 0 && binary.isSuccess && !binaryReady(binary.data)) {
    return (
      <>
        <TitlebarToolbar title={t("overview.title")} />
        <div className="mx-auto flex w-full max-w-[560px] flex-1 flex-col justify-center gap-5 px-5 pb-(--toolbar-height)">
          <div>
            <h2 className="text-large-title">{t("overview.welcome")}</h2>
            <p className="mt-2 text-body text-secondary">{t("overview.welcomeDetail")}</p>
          </div>
          <BinaryNotice binary={binary.data ?? null} />
        </div>
      </>
    );
  }

  if (loading) {
    return (
      <>
        <TitlebarToolbar title={t("overview.title")} />
        <div
          role="status"
          aria-busy
          aria-label={t("overview.loading")}
          className="mx-auto flex w-full max-w-[680px] flex-col gap-2.5 px-5 pt-2"
        >
          <Skeleton className="mx-2.5 h-4 w-24" />
          <Skeleton className="h-11" />
          <Skeleton className="h-11" />
        </div>
      </>
    );
  }

  const failed = overview.error ? (
    <div role="alert" className="flex items-center gap-3 rounded-card bg-surface-inset px-3 py-2.5">
      <TriangleAlert aria-hidden className="size-4 shrink-0 text-warning" strokeWidth={1.75} />
      <span className="min-w-0 flex-1 text-body">
        {t("overview.routesFailed")}{" "}
        <span className="text-secondary">{toIpcError(overview.error).message}</span>
      </span>
      <Button size="sm" pending={overview.isFetching} onClick={() => void overview.refetch()}>
        {t("common.tryAgain")}
      </Button>
    </div>
  ) : null;

  if (shares.length === 0 && routes.length === 0 && !failed) {
    return (
      <>
        <TitlebarToolbar title={t("overview.title")} />
        <div className="mx-auto w-full max-w-[680px] px-5 pt-2 empty:hidden">
          <CliOffer />
        </div>
        <EmptyState
          icon={LayoutGrid}
          title={t("overview.empty.title")}
          description={t("overview.empty.description")}
          action={
            <div className="flex flex-col items-center gap-2">
              <Button variant="primary" asChild>
                <Link to="/quick-share" search={{ compose: true }}>
                  {t("overview.empty.share")}
                </Link>
              </Button>
              {accounts.isSuccess && accounts.data.length === 0 ? (
                <ConnectSheet
                  trigger={<Button variant="plain">{t("overview.empty.ownDomain")}</Button>}
                />
              ) : null}
            </div>
          }
        />
      </>
    );
  }

  return (
    <>
      <TitlebarToolbar title={t("overview.title")} />
      <div className="mx-auto flex w-full max-w-[680px] flex-col gap-6 overflow-y-auto px-5 pt-2 pb-8">
        <CliOffer />
        {failed}
        {problems > 0 ? (
          <Link
            to="/doctor"
            className="flex min-h-11 items-center gap-3 rounded-card bg-error/10 px-3 py-2 outline-offset-0"
          >
            <TriangleAlert aria-hidden className="size-4 text-error" strokeWidth={1.75} />
            <span className="min-w-0 flex-1 text-body">
              {t("overview.attention", { count: problems })}
            </span>
            <ChevronRight aria-hidden className="size-3.5 text-tertiary" strokeWidth={2} />
          </Link>
        ) : null}
        {tunnel && routes.length > 0 ? <TrafficCard tunnelId={tunnel.id} /> : null}
        {routes.length > 0 ? (
          <Section title={t("overview.routes")}>
            {routes.map((route) => {
              const status = routeStatus(route, tunnel, issues);
              return (
                <li key={`${route.hostname}${route.path ?? ""}`}>
                  <Link
                    to="/routes"
                    className="flex min-h-11 items-center gap-3 rounded-row py-2 outline-offset-0"
                  >
                    <StatusDot status={status.dot} label={status.label} />
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-body">
                        {route.hostname}
                        {route.path ? ` ${route.path}` : ""}
                      </div>
                      <div className="truncate font-mono text-[11px] leading-4 text-secondary">
                        {stripScheme(route.origin)}
                      </div>
                    </div>
                    <span className="text-callout text-secondary">{status.label}</span>
                    <ChevronRight aria-hidden className="size-3.5 text-tertiary" strokeWidth={2} />
                  </Link>
                </li>
              );
            })}
          </Section>
        ) : null}
        {shares.length > 0 ? (
          <Section title={t("overview.shares")}>
            {shares.map((share) => (
              <li key={share.id}>
                <Link
                  to="/quick-share"
                  className="flex min-h-11 items-center gap-3 rounded-row py-2 outline-offset-0"
                >
                  <StatusDot status={dots[share.status.status]} />
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
                  <ChevronRight aria-hidden className="size-3.5 text-tertiary" strokeWidth={2} />
                </Link>
              </li>
            ))}
          </Section>
        ) : null}
      </div>
    </>
  );
}

/** This Mac's traffic in the last hour, at a glance; opens the Tunnels view. */
function TrafficCard({ tunnelId }: { tunnelId: string }) {
  // Every 10 s is enough for a glance, and keeps the connector on its idle sampling rate.
  const traffic = useLiveTraffic(tunnelId, 10_000).data;
  if (!traffic || traffic.series.at.length < 2) return null;
  const now = recentRate(traffic.series, 60);
  const rates = perSecond(traffic.series.requests, traffic.series.span).map((v) => v ?? 0);
  return (
    <Link
      to="/tunnels"
      className="flex items-center gap-4 rounded-card bg-surface-inset px-3 py-2.5 outline-offset-0"
    >
      <div className="flex w-36 shrink-0 flex-col">
        <span className="text-callout text-secondary">{t("overview.traffic")}</span>
        <span className="tabular text-title3">
          {now === null ? "–" : formatRate(now)}
          <span className="text-callout text-secondary"> {t("overview.requestsPerSecond")}</span>
        </span>
        <span className="tabular text-callout text-secondary">
          {t("overview.sinceStart", { count: traffic.totalRequests })}
        </span>
      </div>
      <div className="min-w-0 flex-1">
        <Sparkline values={rates} label={t("overview.sparkline")} height={40} />
      </div>
      <ChevronRight aria-hidden className="size-3.5 shrink-0 text-tertiary" strokeWidth={2} />
    </Link>
  );
}
