import { ExternalLink } from "lucide-react";
import { m } from "motion/react";
import { CopyField } from "@/components/patterns/copy-field";
import { Button } from "@/components/ui/button";
import { Disclosure } from "@/components/ui/disclosure";
import { IconButton } from "@/components/ui/icon-button";
import { type Status, StatusDot } from "@/components/ui/status-dot";
import { Tooltip } from "@/components/ui/tooltip";
import { formatCount, formatDuration, stripScheme } from "@/lib/format";
import type { QuickShare } from "@/lib/ipc/bindings";
import { spring } from "@/lib/motion-tokens";
import { openUrl } from "@/lib/open-url";
import { useNow } from "@/lib/use-now";
import { useShareStats, useStopShare } from "../queries";
import { QrButton } from "./qr-button";
import { ShareLog } from "./share-log";

function statusOf(share: QuickShare): { dot: Status; label: string } {
  switch (share.status.status) {
    case "live":
      return { dot: "healthy", label: "Live" };
    case "starting":
      return { dot: "connecting", label: "Getting a URL…" };
    case "reconnecting":
      return { dot: "warning", label: "Reconnecting…" };
    case "failed":
      return { dot: "error", label: "Failed" };
  }
}

/** One running Quick Share. */
export function ShareCard({ share }: { share: QuickShare }) {
  const now = useNow();
  const live = share.status.status === "live";
  const { data: stats } = useShareStats(share.id, live);
  const stop = useStopShare();
  const { dot, label } = statusOf(share);

  return (
    <article
      aria-label={`Quick Share of ${stripScheme(share.origin)}`}
      className="flex flex-col gap-3 rounded-card bg-surface-inset p-4"
    >
      <header className="flex items-center gap-2 text-callout">
        {/* The dot springs in when the share goes live: the one "success" moment. */}
        <m.span
          key={share.status.status}
          initial={live ? { scale: 0.3 } : false}
          animate={{ scale: 1 }}
          transition={spring("bouncy")}
          className="flex"
        >
          <StatusDot status={dot} label={label} />
        </m.span>
        <span className="font-medium text-primary">{label}</span>
        <span className="text-tertiary">·</span>
        <span className="selectable font-mono text-mono text-secondary">
          {stripScheme(share.origin)}
        </span>
        <span className="ml-auto text-secondary tabular">
          {formatDuration(now - share.startedAt)}
        </span>
      </header>

      {share.status.status === "failed" ? (
        <p className="text-body text-secondary">{share.status.message}</p>
      ) : (
        <div className="flex items-center gap-2">
          <div className="min-w-0 flex-1">
            {share.url ? (
              <CopyField label="URL" value={share.url} className="h-7 bg-surface-content" />
            ) : (
              <div className="flex h-7 items-center rounded-control bg-surface-content px-2 text-body text-tertiary">
                Waiting for Cloudflare…
              </div>
            )}
          </div>
          {share.url ? (
            <>
              <Tooltip content="Open in browser">
                <IconButton
                  icon={ExternalLink}
                  label="Open in browser"
                  variant="secondary"
                  size="lg"
                  disabled={!live}
                  onClick={() => share.url && void openUrl(share.url)}
                />
              </Tooltip>
              <QrButton url={share.url} />
            </>
          ) : null}
        </div>
      )}

      <footer className="flex items-center gap-3 text-callout text-secondary">
        {live && stats ? (
          <span className="tabular">
            {formatCount(stats.requests, "request")}
            {stats.errors > 0 ? ` · ${formatCount(stats.errors, "error")}` : ""}
          </span>
        ) : null}
        {share.stopAt ? (
          <span className="tabular">Stops in {formatDuration(share.stopAt - now)}</span>
        ) : null}
        <Button
          variant="destructive"
          size="sm"
          className="ml-auto"
          disabled={stop.isPending}
          onClick={() => stop.mutate(share.id)}
        >
          Stop sharing
        </Button>
      </footer>

      <Disclosure title={<span className="text-callout font-normal text-secondary">Log</span>}>
        <ShareLog id={share.id} />
      </Disclosure>
    </article>
  );
}
