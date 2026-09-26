import { Link } from "@tanstack/react-router";
import { ChevronRight, LayoutGrid, TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";
import { EmptyState } from "@/components/patterns/empty-state";
import { SlowHint } from "@/components/patterns/loading-state";
import { TitlebarToolbar } from "@/components/patterns/titlebar-toolbar";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { type Status, StatusDot } from "@/components/ui/status-dot";
import { ConnectSheet, useAccounts, useActiveAccount } from "@/features/accounts";
import { BinaryNotice, binaryReady, useBinaryStatus } from "@/features/binary";
import { useIssues } from "@/features/doctor/queries";
import { RecentRequests } from "@/features/inspector";
import { useQuickShares } from "@/features/quick-share";
import { useRoutesOverview } from "@/features/routes/queries";
import { routeStatus } from "@/features/routes/status";
import { CliOffer } from "@/features/settings";
import { formatDuration, stripScheme } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { QuickShare } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { useNow } from "@/lib/use-now";
import { Glance } from "./components/glance";

const dots: Record<QuickShare["status"]["status"], Status> = {
  live: "healthy",
  starting: "connecting",
  reconnecting: "warning",
  failed: "error",
};

function Section({
  title,
  action,
  children,
}: {
  title: string;
  action?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="flex flex-col gap-2.5">
      <div className="flex items-baseline justify-between gap-3 px-2.5">
        <h2 className="text-headline">{title}</h2>
        {action}
      </div>
      <ul className="flex flex-col divide-y-(length:--hairline) divide-inset rounded-card bg-surface-inset px-2.5">
        {children}
      </ul>
    </section>
  );
}

/** Requests the Overview shows. */
const RECENT = 6;

/** What's running right now: health, traffic, errors, uptime and the newest requests. */
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
          {/* Local answers are instant; only Cloudflare can take a while. */}
          {!sharesQuery.isPending && !accounts.isPending ? (
            <SlowHint className="px-2.5 pt-1">{t("overview.checkingRoutes")}</SlowHint>
          ) : null}
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
        <Glance
          tunnelId={tunnel && routes.length > 0 ? tunnel.id : null}
          accountId={active?.id ?? null}
        />
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
        <RecentRequests limit={RECENT}>
          {(rows) => (
            <Section
              title={t("overview.recent")}
              action={
                <Link to="/inspector" className="text-callout text-accent">
                  {t("overview.showAll")}
                </Link>
              }
            >
              {rows}
            </Section>
          )}
        </RecentRequests>
      </div>
    </>
  );
}
