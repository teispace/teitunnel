import { ExternalLink, SquareTerminal } from "lucide-react";
import { CopyField } from "@/components/patterns/copy-field";
import { Button } from "@/components/ui/button";
import { IconButton } from "@/components/ui/icon-button";
import { Tooltip } from "@/components/ui/tooltip";
import { formatDuration, stripScheme } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { CliShare } from "@/lib/ipc/bindings";
import { openUrl } from "@/lib/open-url";
import { useNow } from "@/lib/use-now";
import { useStopTerminalShare } from "../queries";
import { QrButton } from "./qr-button";

/** A Quick Share running in a terminal (`teitunnel-cli share`). */
export function TerminalShareCard({ share }: { share: CliShare }) {
  const now = useNow();
  const stop = useStopTerminalShare();
  return (
    <article
      aria-label={t("quickShare.terminal.cardLabel", { origin: stripScheme(share.origin) })}
      className="flex flex-col gap-3 rounded-card bg-surface-inset p-4"
    >
      <header className="flex items-center gap-2 text-callout">
        <SquareTerminal aria-hidden className="size-3.5 text-secondary" strokeWidth={2} />
        <span className="font-medium text-primary">{t("quickShare.terminal.title")}</span>
        <span className="text-tertiary">·</span>
        <span className="selectable font-mono text-mono text-secondary">
          {stripScheme(share.origin)}
        </span>
        <span className="ml-auto text-secondary tabular">
          {formatDuration(now - (share.startedAt ?? now))}
        </span>
      </header>
      <div className="flex items-center gap-2">
        <div className="min-w-0 flex-1">
          <CopyField label={t("common.url")} value={share.url} className="h-7 bg-surface-content" />
        </div>
        <Tooltip content={t("quickShare.openInBrowser")}>
          <IconButton
            icon={ExternalLink}
            label={t("quickShare.openInBrowser")}
            variant="secondary"
            size="lg"
            onClick={() => void openUrl(share.url)}
          />
        </Tooltip>
        <QrButton url={share.url} />
      </div>
      <footer className="flex items-center gap-3 text-callout text-secondary">
        {share.stopAt ? (
          <span className="tabular">
            {t("quickShare.stopsIn", { duration: formatDuration(share.stopAt - now) })}
          </span>
        ) : null}
        <Button
          variant="destructive"
          size="sm"
          className="ml-auto"
          disabled={stop.isPending}
          onClick={() => stop.mutate(share.owner)}
        >
          {t("quickShare.stop")}
        </Button>
      </footer>
    </article>
  );
}
