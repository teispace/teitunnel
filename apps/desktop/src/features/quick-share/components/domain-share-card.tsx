import { useNavigate } from "@tanstack/react-router";
import { Camera, ExternalLink, Folder, Globe, Pause, Play, Shield } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { CopyField } from "@/components/patterns/copy-field";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { IconButton } from "@/components/ui/icon-button";
import { Tooltip } from "@/components/ui/tooltip";
import { CommentsToggle } from "@/features/comments";
import { useRoute, useSendHostOnRoute } from "@/features/dev-server";
import { InspectDomainShareButton } from "@/features/inspector";
import { ProtectionSheet, useProtection } from "@/features/protection";
import { siteUrl } from "@/features/snapshots";
import { formatDuration, stripScheme } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { DomainShare } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { openUrl } from "@/lib/open-url";
import { useNow } from "@/lib/use-now";
import {
  useDomainShareCheck,
  useSchedules,
  useSetSharePaused,
  useStopDomainShare,
} from "../queries";
import { nextChangeLabel, scheduleLabel } from "../schedule";
import { cardClass } from "./card";
import { QrButton } from "./qr-button";
import { ScheduleButton } from "./schedule-editor";
import { HostHeaderNote, ShareCheck } from "./share-check";

/** One share on your own domain: a temporary route, removed when it stops. */
export function DomainShareCard({ share }: { share: DomainShare }) {
  const now = useNow();
  const stop = useStopDomainShare();
  const pause = useSetSharePaused();
  const check = useDomainShareCheck(share.accountId, share.hostname, share.createdAt);
  const route = useRoute(share.accountId, share.hostname).data ?? null;
  const sendHost = useSendHostOnRoute(share.accountId);
  const schedule =
    useSchedules().data?.find(
      (s) => s.accountId === share.accountId && s.hostname === share.hostname,
    ) ?? null;
  const hostHeader = route?.options.httpHostHeader ?? null;
  const navigate = useNavigate();
  const url = `https://${share.hostname}`;
  const fromCli = share.owner !== "app";
  const [protecting, setProtecting] = useState(false);
  const protection = useProtection(share.accountId, share.hostname, protecting);
  const shared = share.source ?? share.origin;
  const pauseLabel = share.paused ? t("quickShare.pause.resume") : t("quickShare.pause.pause");
  const next = schedule ? nextChangeLabel(schedule) : null;
  return (
    <article
      aria-label={t("quickShare.domain.cardLabel", { hostname: share.hostname })}
      aria-busy={stop.isPending || undefined}
      className={cardClass}
    >
      <header className="flex items-center gap-2 text-callout">
        {share.folder ? (
          <Folder aria-hidden className="size-3.5 text-accent" strokeWidth={2} />
        ) : (
          <Globe aria-hidden className="size-3.5 text-accent" strokeWidth={2} />
        )}
        <span className="font-medium text-primary">{t("quickShare.domain.onYourDomain")}</span>
        <span className="text-tertiary">·</span>
        <span className="selectable min-w-0 truncate font-mono text-mono text-secondary">
          {share.folder ? shared : stripScheme(shared)}
        </span>
        {share.paused ? <Badge tone="warning">{t("quickShare.pause.badge")}</Badge> : null}
        <span className="ml-auto shrink-0 text-secondary tabular">
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
        <InspectDomainShareButton accountId={share.accountId} hostname={share.hostname} />
        <Tooltip content={pauseLabel}>
          <IconButton
            icon={share.paused ? Play : Pause}
            label={pauseLabel}
            variant="secondary"
            size="lg"
            pending={pause.isPending}
            onClick={() =>
              pause.mutate(
                { accountId: share.accountId, hostname: share.hostname, paused: !share.paused },
                { onError: (error) => toast.error(toIpcError(error).message) },
              )
            }
          />
        </Tooltip>
        <ScheduleButton
          accountId={share.accountId}
          hostname={share.hostname}
          current={schedule?.schedule ?? share.schedule}
        />
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
        {share.folder ? null : (
          <Tooltip content={t("quickShare.snapshot")}>
            <IconButton
              icon={Camera}
              label={t("quickShare.snapshot")}
              variant="secondary"
              size="lg"
              onClick={() =>
                void navigate({ to: "/snapshots", search: { capture: siteUrl(shared) } })
              }
            />
          </Tooltip>
        )}
      </div>
      {share.paused ? (
        <p className="text-callout text-secondary">{t("quickShare.pause.detail")}</p>
      ) : null}
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
        settling={check.settling}
      />
      {schedule ? (
        <p className="text-callout text-secondary">
          {t("quickShare.schedule.line", { schedule: scheduleLabel(schedule.schedule) })}
          {next ? ` · ${next}` : ""}
        </p>
      ) : null}
      <CommentsToggle
        target={{ kind: "route", accountId: share.accountId, hostname: share.hostname }}
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
