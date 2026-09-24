import { useNavigate } from "@tanstack/react-router";
import { Camera, ExternalLink, Globe, Shield } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { CopyField } from "@/components/patterns/copy-field";
import { Button } from "@/components/ui/button";
import { IconButton } from "@/components/ui/icon-button";
import { Tooltip } from "@/components/ui/tooltip";
import { useRoute, useSendHostOnRoute } from "@/features/dev-server";
import { ProtectionSheet, useProtection } from "@/features/protection";
import { siteUrl } from "@/features/snapshots";
import { formatDuration, stripScheme } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { DomainShare } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { openUrl } from "@/lib/open-url";
import { useNow } from "@/lib/use-now";
import { useDomainShareCheck, useStopDomainShare } from "../queries";
import { cardClass } from "./card";
import { QrButton } from "./qr-button";
import { HostHeaderNote, ShareCheck } from "./share-check";

/** One share on your own domain: a temporary route, removed when it stops. */
export function DomainShareCard({ share }: { share: DomainShare }) {
  const now = useNow();
  const stop = useStopDomainShare();
  const check = useDomainShareCheck(share.accountId, share.hostname);
  const route = useRoute(share.accountId, share.hostname).data ?? null;
  const sendHost = useSendHostOnRoute(share.accountId);
  const hostHeader = route?.options.httpHostHeader ?? null;
  const navigate = useNavigate();
  const url = `https://${share.hostname}`;
  const fromCli = share.owner !== "app";
  const [protecting, setProtecting] = useState(false);
  const protection = useProtection(share.accountId, share.hostname, protecting);
  return (
    <article
      aria-label={t("quickShare.domain.cardLabel", { hostname: share.hostname })}
      aria-busy={stop.isPending || undefined}
      className={cardClass}
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
        <Tooltip content={t("protection.protectShare")}>
          <IconButton
            icon={Shield}
            label={t("protection.protectShare")}
            variant="secondary"
            size="lg"
            aria-busy={protection.isFetching || undefined}
            onClick={() => setProtecting(true)}
          />
        </Tooltip>
        <Tooltip content={t("quickShare.snapshot")}>
          <IconButton
            icon={Camera}
            label={t("quickShare.snapshot")}
            variant="secondary"
            size="lg"
            onClick={() =>
              void navigate({ to: "/snapshots", search: { capture: siteUrl(share.origin) } })
            }
          />
        </Tooltip>
      </div>
      {protecting && protection.error ? (
        <p role="alert" className="text-callout text-error">
          {toIpcError(protection.error).message}
        </p>
      ) : null}
      <ProtectionSheet
        accountId={share.accountId}
        hostname={share.hostname}
        current={protection.data}
        open={protecting && protection.isSuccess}
        onClose={() => setProtecting(false)}
      />
      {hostHeader ? <HostHeaderNote header={{ value: hostHeader, autoFor: null }} /> : null}
      <ShareCheck
        check={check.data ?? null}
        via="route"
        onSendHost={
          route
            ? (host) => sendHost.mutateAsync({ route, host }).then(() => check.refetch())
            : undefined
        }
        sending={sendHost.isPending}
        onCheck={() => check.refetch()}
        checking={check.isFetching}
      />
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
          pending={stop.isPending}
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
