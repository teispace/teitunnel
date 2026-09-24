import { ExternalLink } from "lucide-react";
import { m } from "motion/react";
import { toast } from "sonner";
import { CopyField } from "@/components/patterns/copy-field";
import { Button } from "@/components/ui/button";
import { Disclosure } from "@/components/ui/disclosure";
import { IconButton } from "@/components/ui/icon-button";
import { type Status, StatusDot } from "@/components/ui/status-dot";
import { Tooltip } from "@/components/ui/tooltip";
import { formatDuration, stripScheme } from "@/lib/format";
import { t, translate } from "@/lib/i18n";
import type { QuickShare } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { spring } from "@/lib/motion-tokens";
import { openUrl } from "@/lib/open-url";
import { useNow } from "@/lib/use-now";
import { useCheckShare, useSetShareHostHeader, useShareStats, useStopShare } from "../queries";
import { cardClass } from "./card";
import { QrButton } from "./qr-button";
import { HostHeaderNote, ShareCheck } from "./share-check";
import { ShareLog } from "./share-log";

function statusOf(share: QuickShare): { dot: Status; label: string } {
  switch (share.status.status) {
    case "live":
      return { dot: "healthy", label: t("quickShare.status.live") };
    case "starting":
      return { dot: "connecting", label: t("quickShare.status.starting") };
    case "reconnecting":
      return { dot: "warning", label: t("quickShare.status.reconnecting") };
    case "failed":
      return { dot: "error", label: t("quickShare.status.failed") };
  }
}

/** One running Quick Share. */
export function ShareCard({ share }: { share: QuickShare }) {
  const now = useNow();
  const live = share.status.status === "live";
  const { data: stats } = useShareStats(share.id, live);
  const stop = useStopShare();
  const setHostHeader = useSetShareHostHeader();
  const check = useCheckShare();
  const { dot, label } = statusOf(share);

  return (
    <article
      aria-label={t("quickShare.cardLabel", { origin: stripScheme(share.origin) })}
      className={cardClass}
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
        <p className="text-body text-secondary">{translate(share.status.message)}</p>
      ) : (
        <div className="flex items-center gap-2">
          <div className="min-w-0 flex-1">
            {share.url ? (
              <CopyField
                label={t("common.url")}
                value={share.url}
                className="h-7 bg-surface-content"
              />
            ) : (
              <div className="flex h-7 items-center rounded-control bg-surface-content px-2 text-body text-tertiary">
                {t("quickShare.waiting")}
              </div>
            )}
          </div>
          {share.url ? (
            <>
              <Tooltip content={t("quickShare.openInBrowser")}>
                <IconButton
                  icon={ExternalLink}
                  label={t("quickShare.openInBrowser")}
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

      {share.hostHeader ? <HostHeaderNote header={share.hostHeader} /> : null}
      {share.status.status === "failed" ? null : (
        <ShareCheck
          check={share.check}
          via="share"
          onSendHost={(host) => setHostHeader.mutateAsync({ id: share.id, host })}
          sending={setHostHeader.isPending}
          onCheck={() => check.mutateAsync(share.id)}
          checking={check.isPending}
        />
      )}

      <footer className="flex items-center gap-3 text-callout text-secondary">
        {live && stats ? (
          <span className="tabular">
            {t("quickShare.requests", { count: stats.requests })}
            {stats.errors > 0 ? ` · ${t("quickShare.errors", { count: stats.errors })}` : ""}
          </span>
        ) : null}
        {share.stopAt ? (
          <span className="tabular">
            {t("quickShare.stopsIn", { duration: formatDuration(share.stopAt - now) })}
          </span>
        ) : null}
        <Button
          variant="destructive"
          size="sm"
          className="ml-auto"
          pending={stop.isPending}
          onClick={() =>
            stop.mutate(share.id, {
              onError: (error) => toast.error(toIpcError(error).message),
            })
          }
        >
          {t("quickShare.stop")}
        </Button>
      </footer>

      <Disclosure
        title={
          <span className="text-callout font-normal text-secondary">{t("quickShare.log")}</span>
        }
      >
        <ShareLog id={share.id} />
      </Disclosure>
    </article>
  );
}
