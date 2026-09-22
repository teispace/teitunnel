import { Link } from "@tanstack/react-router";
import { ChevronRight, LayoutGrid } from "lucide-react";
import { EmptyState } from "@/components/patterns/empty-state";
import { TitlebarToolbar } from "@/components/patterns/titlebar-toolbar";
import { Button } from "@/components/ui/button";
import { type Status, StatusDot } from "@/components/ui/status-dot";
import { ConnectSheet, useAccounts } from "@/features/accounts";
import { BinaryNotice, binaryReady, useBinaryStatus } from "@/features/binary";
import { useQuickShares } from "@/features/quick-share";
import { formatDuration, stripScheme } from "@/lib/format";
import type { QuickShare } from "@/lib/ipc/bindings";
import { useNow } from "@/lib/use-now";

const dots: Record<QuickShare["status"]["status"], Status> = {
  live: "healthy",
  starting: "connecting",
  reconnecting: "warning",
  failed: "error",
};

/** What's running right now, at a glance. */
export function OverviewPage() {
  const { data: shares = [] } = useQuickShares();
  const binary = useBinaryStatus();
  const accounts = useAccounts();
  const now = useNow();

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

  if (shares.length === 0) {
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
                  Share a local service
                </Link>
              </Button>
              {accounts.isSuccess && accounts.data.length === 0 ? (
                <ConnectSheet trigger={<Button variant="plain">Use my own domain…</Button>} />
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
      <div className="mx-auto flex w-full max-w-[680px] flex-col gap-2.5 overflow-y-auto px-5 pt-2 pb-8">
        <h2 className="px-2.5 text-headline">Quick Shares</h2>
        <ul className="flex flex-col divide-y-(length:--hairline) divide-inset rounded-card bg-surface-inset px-2.5">
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
        </ul>
      </div>
    </>
  );
}
