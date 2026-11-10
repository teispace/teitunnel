import { Link } from "@tanstack/react-router";
import { LayoutGrid, TriangleAlert } from "lucide-react";
import { Card, CardLink, DashboardPage, DashboardSkeleton } from "@/components/patterns/dashboard";
import { EmptyState } from "@/components/patterns/empty-state";
import { SlowHint } from "@/components/patterns/loading-state";
import { TitlebarToolbar } from "@/components/patterns/titlebar-toolbar";
import { Button } from "@/components/ui/button";
import { ConnectSheet, ErrorAction, useAccounts, useActiveAccount } from "@/features/accounts";
import { useUptimeList } from "@/features/analytics";
import { BinaryNotice, binaryReady, useBinaryStatus } from "@/features/binary";
import { useIssues } from "@/features/doctor/queries";
import { RecentRequests } from "@/features/inspector";
import { useQuickShares } from "@/features/quick-share";
import { useActivity, useLiveTraffic, useRoutesOverview } from "@/features/routes/queries";
import { CliOffer } from "@/features/settings";
import { t } from "@/lib/i18n";
import { toIpcError } from "@/lib/ipc/client";
import { useNow } from "@/lib/use-now";
import {
  ActivityCard,
  OverviewStats,
  ResponsesCard,
  RoutesCard,
  SharesCard,
  StatusBanner,
  TrafficCard,
} from "./components/sections";

/** Requests the Overview shows. */
const RECENT = 8;

/**
 * The dashboard: health at the top, the numbers that matter, live traffic, and what's
 * running (routes, shares, requests, changes), filling the window at any size.
 */
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
  const warnings = issues.filter((i) => i.severity === "warning").length;
  const tunnelId = tunnel && routes.length > 0 ? tunnel.id : null;
  const traffic = useLiveTraffic(tunnelId, 5_000).data ?? undefined;
  const uptimes = useUptimeList().data ?? [];
  const activity = useActivity(active?.id ?? null);

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
        <DashboardSkeleton label={t("overview.loading")} />
        {/* Local answers are instant; only Cloudflare can take a while. */}
        {!sharesQuery.isPending && !accounts.isPending ? (
          <SlowHint className="px-8 pb-4">{t("overview.checkingRoutes")}</SlowHint>
        ) : null}
      </>
    );
  }

  const failed = overview.error ? (
    <div role="alert" className="flex items-center gap-3 rounded-card bg-surface-inset px-4 py-3">
      <TriangleAlert aria-hidden className="size-4 shrink-0 text-warning" strokeWidth={1.75} />
      <span className="min-w-0 flex-1 text-body">
        {t("overview.routesFailed")}{" "}
        <span className="text-secondary">{toIpcError(overview.error).message}</span>
      </span>
      <ErrorAction error={overview.error} onRetry={() => void overview.refetch()} />
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
      <DashboardPage>
        <CliOffer />
        {failed}
        <StatusBanner
          problems={problems}
          warnings={warnings}
          connections={traffic ? traffic.connections : null}
          locations={traffic?.locations ?? []}
          account={accounts.data && accounts.data.length > 1 ? (active?.name ?? null) : null}
        />
        <OverviewStats
          traffic={traffic}
          uptimes={uptimes}
          accountId={active?.id ?? null}
          routes={routes}
          tunnel={tunnel}
          issues={issues}
          shares={shares}
        />
        {tunnelId ? (
          <div className="grid gap-3 @5xl:grid-cols-3">
            <TrafficCard traffic={traffic} />
            <ResponsesCard traffic={traffic} />
          </div>
        ) : null}
        <div className="grid gap-3 @5xl:grid-cols-2">
          {active ? <RoutesCard routes={routes} tunnel={tunnel} issues={issues} /> : null}
          <SharesCard shares={shares} now={now} />
          <RecentRequests limit={RECENT}>
            {(rows) => (
              <Card
                title={t("overview.recent")}
                action={<CardLink to="/inspector">{t("overview.showAll")}</CardLink>}
              >
                <ul className="flex flex-col divide-y-(length:--hairline) divide-inset">{rows}</ul>
              </Card>
            )}
          </RecentRequests>
          {active ? <ActivityCard entries={activity.data ?? []} /> : null}
        </div>
      </DashboardPage>
    </>
  );
}
