import { Link } from "@tanstack/react-router";
import { ChevronRight, LayoutGrid, TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";
import { EmptyState } from "@/components/patterns/empty-state";
import { Sparkline } from "@/components/patterns/sparkline";
import { TitlebarToolbar } from "@/components/patterns/titlebar-toolbar";
import { Button } from "@/components/ui/button";
import { type Status, StatusDot } from "@/components/ui/status-dot";
import { ConnectSheet, useAccounts, useActiveAccount } from "@/features/accounts";
import { BinaryNotice, binaryReady, useBinaryStatus } from "@/features/binary";
import { useIssues } from "@/features/doctor/queries";
import { useQuickShares } from "@/features/quick-share";
import { useLiveTraffic } from "@/features/routes";
import { useRoutesOverview } from "@/features/routes/queries";
import { routeStatus } from "@/features/routes/status";
import { formatDuration, stripScheme } from "@/lib/format";
import type { QuickShare } from "@/lib/ipc/bindings";
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
  const { data: shares = [] } = useQuickShares();
  const binary = useBinaryStatus();
  const accounts = useAccounts();
  const now = useNow();
  const active = useActiveAccount();
  const overview = useRoutesOverview(active?.id ?? null);
  const routes = overview.data?.routes ?? [];
  const tunnel = overview.data?.tunnel ?? null;
  const { issues } = useIssues();
  const problems = issues.filter((i) => i.severity === "error").length;

  if (shares.length === 0 && binary.isSuccess && !binaryReady(binary.data)) {
    return (
      <>
        <TitlebarToolbar title="Overview" />
        <div className="mx-auto flex w-full max-w-[560px] flex-1 flex-col justify-center gap-5 px-5 pb-(--toolbar-height)">
          <div>
            <h2 className="text-large-title">Welcome to Teitunnel</h2>
            <p className="mt-2 text-body text-secondary">
              Share anything running on this Mac at a public URL, and connect your own domains
              through Cloudflare. First, Teitunnel needs Cloudflare's connector.
            </p>
          </div>
          <BinaryNotice binary={binary.data ?? null} />
        </div>
      </>
    );
  }

  if (shares.length === 0 && routes.length === 0) {
    return (
      <>
        <TitlebarToolbar title="Overview" />
        <EmptyState
          icon={LayoutGrid}
          title="Nothing running yet"
          description="Share a local service to get a public URL. It shows up here with its live status."
          action={
            <div className="flex flex-col items-center gap-2">
              <Button variant="primary" asChild>
                <Link to="/quick-share" search={{ compose: true }}>
                  Share a Local Service
                </Link>
              </Button>
              {accounts.isSuccess && accounts.data.length === 0 ? (
                <ConnectSheet trigger={<Button variant="plain">Use My Own Domain…</Button>} />
              ) : null}
            </div>
          }
        />
      </>
    );
  }

  return (
    <>
      <TitlebarToolbar title="Overview" />
      <div className="mx-auto flex w-full max-w-[680px] flex-col gap-6 overflow-y-auto px-5 pt-2 pb-8">
        {problems > 0 ? (
          <Link
            to="/doctor"
            className="flex min-h-11 items-center gap-3 rounded-card bg-error/10 px-3 py-2 outline-offset-0"
          >
            <TriangleAlert aria-hidden className="size-4 text-error" strokeWidth={1.75} />
            <span className="min-w-0 flex-1 text-body">
              {problems === 1 ? "1 problem needs" : `${problems} problems need`} your attention
            </span>
            <ChevronRight aria-hidden className="size-3.5 text-tertiary" strokeWidth={2} />
          </Link>
        ) : null}
        {tunnel && routes.length > 0 ? <TrafficCard tunnelId={tunnel.id} /> : null}
        {routes.length > 0 ? (
          <Section title="Routes">
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
          <Section title="Quick Shares">
            {shares.map((share) => (
              <li key={share.id}>
                <Link
                  to="/quick-share"
                  className="flex min-h-11 items-center gap-3 rounded-row py-2 outline-offset-0"
                >
                  <StatusDot status={dots[share.status.status]} />
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-body">
                      {share.url ? stripScheme(share.url) : "Getting a URL…"}
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
        <span className="text-callout text-secondary">Traffic on this Mac</span>
        <span className="tabular text-title3">
          {now === null ? "–" : formatRate(now)}
          <span className="text-callout text-secondary"> requests/s</span>
        </span>
        <span className="tabular text-callout text-secondary">
          {traffic.totalRequests.toLocaleString()} since start
        </span>
      </div>
      <div className="min-w-0 flex-1">
        <Sparkline values={rates} label="Requests per second over the last hour" height={40} />
      </div>
      <ChevronRight aria-hidden className="size-3.5 shrink-0 text-tertiary" strokeWidth={2} />
    </Link>
  );
}
