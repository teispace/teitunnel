import { useState } from "react";
import { CopyField } from "@/components/patterns/copy-field";
import { GroupedRow, GroupedSection, SkeletonSection } from "@/components/patterns/grouped-list";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ProgressBar } from "@/components/ui/progress-bar";
import { Spinner } from "@/components/ui/spinner";
import { type MessageKey, t } from "@/lib/i18n";
import { toIpcError } from "@/lib/ipc/client";
import { useBinaryStatus, useCheckUpdate, useInstallBinary, useRevealBinary } from "../queries";
import { BinaryNotice, describeProgress } from "./binary-notice";

const sources: Record<string, MessageKey> = {
  managed: "binary.source.managed",
  system: "binary.source.system",
  override: "binary.source.override",
};

/** Settings → cloudflared: which binary runs, and keeping it current. */
export function CloudflaredPane() {
  const { data: binary, isSuccess } = useBinaryStatus();
  const [checkRequested, setCheckRequested] = useState(false);
  const update = useCheckUpdate(checkRequested && binary?.source === "managed");
  const install = useInstallBinary();
  const reveal = useRevealBinary();
  const progress = describeProgress(install.progress);

  if (!isSuccess) return <SkeletonSection rows={3} />;
  if (!binary?.supported) return <BinaryNotice binary={binary ?? null} />;

  const managed = binary.source === "managed";
  return (
    <div className="flex flex-col gap-5">
      <GroupedSection title="cloudflared" footer={t("binary.footer")}>
        <GroupedRow label={t("binary.version")}>
          <span className="selectable font-mono text-mono">
            {binary.version ?? t("binary.unknown")}
          </span>
          <Badge tone="healthy">{t("binary.supported")}</Badge>
        </GroupedRow>
        <GroupedRow label={t("binary.sourceLabel")}>
          <span className="text-body text-secondary">
            {binary.source in sources ? t(sources[binary.source] as MessageKey) : binary.source}
          </span>
        </GroupedRow>
        <GroupedRow label={t("binary.location")}>
          <CopyField label={t("binary.path")} value={binary.path} className="w-72" />
          <Button size="sm" pending={reveal.isPending} onClick={() => reveal.mutate()}>
            {t("binary.reveal")}
          </Button>
        </GroupedRow>
      </GroupedSection>

      <GroupedSection title={t("binary.updates")}>
        {managed ? (
          <GroupedRow
            label={
              update.data?.available
                ? t("binary.available", { version: update.data.latest })
                : update.data
                  ? t("binary.upToDate")
                  : t("binary.checkNewer")
            }
            description={
              install.isPending
                ? progress.label
                : update.error
                  ? toIpcError(update.error).message
                  : t("binary.sharesKeepVersion")
            }
          >
            {install.isPending ? (
              <ProgressBar
                label={t("binary.updating")}
                className="w-32"
                {...(progress.value === undefined ? {} : { value: progress.value })}
              />
            ) : update.data?.available ? (
              <Button variant="primary" size="sm" onClick={() => install.mutate()}>
                {t("binary.update")}
              </Button>
            ) : update.isFetching ? (
              <Spinner />
            ) : (
              <Button
                size="sm"
                onClick={() => (checkRequested ? void update.refetch() : setCheckRequested(true))}
              >
                {t("binary.checkNow")}
              </Button>
            )}
          </GroupedRow>
        ) : (
          <GroupedRow label={t("binary.byInstaller")} description={t("binary.byInstallerDetail")}>
            <Button size="sm" onClick={() => install.mutate()} pending={install.isPending}>
              {t("binary.useOurs")}
            </Button>
          </GroupedRow>
        )}
      </GroupedSection>
    </div>
  );
}
