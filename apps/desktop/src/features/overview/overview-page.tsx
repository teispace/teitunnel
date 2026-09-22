import { Link } from "@tanstack/react-router";
import { ChevronRight, LayoutGrid } from "lucide-react";
import { EmptyState } from "@/components/patterns/empty-state";
import { TitlebarToolbar } from "@/components/patterns/titlebar-toolbar";
import { Button } from "@/components/ui/button";
import { type Status, StatusDot } from "@/components/ui/status-dot";
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
  const now = useNow();

  if (shares.length === 0) {
    return (
      <>
        <TitlebarToolbar title="Overview" />
        <EmptyState
          icon={LayoutGrid}
          title="Nothing running yet"
          description="Share a local service to get a public URL. It shows up here with its live status."
          action={
            <Button variant="primary" asChild>
              <Link to="/quick-share" search={{ compose: true }}>
                Share a local service
              </Link>
            </Button>
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
