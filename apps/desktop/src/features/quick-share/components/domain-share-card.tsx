import { ExternalLink, Globe } from "lucide-react";
import { toast } from "sonner";
import { CopyField } from "@/components/patterns/copy-field";
import { Button } from "@/components/ui/button";
import { IconButton } from "@/components/ui/icon-button";
import { Tooltip } from "@/components/ui/tooltip";
import { formatDuration, stripScheme } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { DomainShare } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { openUrl } from "@/lib/open-url";
import { useNow } from "@/lib/use-now";
import { useStopDomainShare } from "../queries";
import { QrButton } from "./qr-button";

/** One share on your own domain: a temporary route, removed when it stops. */
export function DomainShareCard({ share }: { share: DomainShare }) {
  const now = useNow();
  const stop = useStopDomainShare();
  const url = `https://${share.hostname}`;
  const fromCli = share.owner !== "app";
  return (
    <article
      aria-label={t("quickShare.domain.cardLabel", { hostname: share.hostname })}
      className="flex flex-col gap-3 rounded-card bg-surface-inset p-4"
    >
      <header className="flex items-center gap-2 text-callout">
        <Globe aria-hidden className="size-3.5 text-accent" strokeWidth={2} />
        <span className="font-medium text-primary">{t("quickShare.domain.onYourDomain")}</span>
        <span className="text-tertiary">·</span>
        <span className="selectable font-mono text-mono text-secondary">
          {stripScheme(share.origin)}
        </span>
        <span className="ml-auto text-secondary tabular">
          {formatDuration(now - (share.createdAt ?? now))}
        </span>
      </header>
      <div className="flex items-center gap-2">
        <div className="min-w-0 flex-1">
          <CopyField label={t("common.url")} value={url} className="h-7 bg-surface-content" />
        </div>
        <Tooltip content={t("quickShare.openInBrowser")}>
          <IconButton
            icon={ExternalLink}
            label={t("quickShare.openInBrowser")}
            variant="secondary"
            size="lg"
            onClick={() => void openUrl(url)}
          />
        </Tooltip>
        <QrButton url={url} />
      </div>
      <footer className="flex items-center gap-3 text-callout text-secondary">
        <span>{fromCli ? t("quickShare.domain.fromCli") : t("quickShare.domain.endsWithApp")}</span>
        {share.expiresAt ? (
          <span className="tabular">
            {t("quickShare.stopsIn", { duration: formatDuration(share.expiresAt - now) })}
          </span>
        ) : null}
        <Button
          variant="destructive"
          size="sm"
          className="ml-auto"
          disabled={stop.isPending}
          onClick={() =>
            stop.mutate(
              { accountId: share.accountId, hostname: share.hostname },
              { onError: (error) => toast.error(toIpcError(error).message) },
            )
          }
        >
          {t("quickShare.stop")}
        </Button>
      </footer>
    </article>
  );
}
