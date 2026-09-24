import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { ConfirmDialog } from "@/components/patterns/confirm-dialog";
import { GroupedRow, GroupedSection, SkeletonSection } from "@/components/patterns/grouped-list";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { TextArea } from "@/components/ui/text-area";
import { t } from "@/lib/i18n";
import type { InspectorSettingsPatch } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { queryKeys, refresh } from "@/lib/ipc/query-keys";
import { useClearExchanges, useInspectorSettings, useUpdateInspectorSettings } from "../queries";

const RETENTION = ["1", "6", "12", "24", "48", "72", "168"] as const;
const IDLE = ["0", "15", "30", "60", "120", "480"] as const;

const hoursLabel = (hours: number) =>
  hours % 24 === 0 && hours >= 48
    ? t("inspector.settings.days", { count: hours / 24 })
    : t("inspector.settings.hours", { count: hours });

const paths = (text: string) =>
  text
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);

/** Settings ▸ Inspector: Quick Shares by default, history, idle stop, watched paths. */
export function InspectorSettingsPane() {
  const { data } = useInspectorSettings();
  const update = useUpdateInspectorSettings();
  const clear = useClearExchanges();
  const queryClient = useQueryClient();
  const [watched, setWatched] = useState("");
  const saved = (data?.watchedPaths ?? []).join("\n");
  useEffect(() => setWatched(saved), [saved]);

  if (!data) {
    return (
      <>
        <SkeletonSection />
        <SkeletonSection rows={2} />
        <SkeletonSection />
      </>
    );
  }
  // Show a control where it's going while the change is saved.
  const pending = update.isPending ? update.variables : undefined;
  const value = <K extends keyof InspectorSettingsPatch>(key: K) =>
    pending?.[key] ?? data[key as keyof typeof data];
  const retention = String(value("retentionHours") ?? 24);
  const idle = String(value("idleStopMinutes") ?? 0);

  return (
    <>
      <GroupedSection
        title={t("inspector.settings.shares")}
        footer={t("inspector.settings.sharesFooter")}
      >
        <GroupedRow
          label={t("inspector.settings.inspectShares")}
          description={t("inspector.settings.inspectSharesDetail")}
        >
          <Switch
            aria-label={t("inspector.settings.inspectShares")}
            checked={value("inspectQuickShares") !== false}
            disabled={update.isPending}
            onCheckedChange={(inspectQuickShares) => update.mutate({ inspectQuickShares })}
          />
        </GroupedRow>
        <GroupedRow
          label={t("inspector.settings.idle")}
          description={t("inspector.settings.idleDetail")}
        >
          <Select
            label={t("inspector.settings.idle")}
            options={IDLE.map((minutes) => ({
              value: minutes,
              label:
                minutes === "0"
                  ? t("inspector.settings.never")
                  : t("inspector.settings.minutes", { count: Number(minutes) }),
            }))}
            value={IDLE.includes(idle as (typeof IDLE)[number]) ? idle : "0"}
            disabled={update.isPending}
            onValueChange={(minutes) => update.mutate({ idleStopMinutes: Number(minutes) })}
            className="w-32"
          />
        </GroupedRow>
      </GroupedSection>
      <GroupedSection
        title={t("inspector.settings.history")}
        footer={t("inspector.settings.historyFooter")}
      >
        <GroupedRow
          label={t("inspector.settings.keepHistory")}
          description={t("inspector.settings.keepHistoryDetail")}
        >
          <Switch
            aria-label={t("inspector.settings.keepHistory")}
            checked={value("keepHistory") !== false}
            disabled={update.isPending}
            onCheckedChange={(keepHistory) => update.mutate({ keepHistory })}
          />
        </GroupedRow>
        <GroupedRow label={t("inspector.settings.retention")}>
          <Select
            label={t("inspector.settings.retention")}
            options={RETENTION.map((hours) => ({ value: hours, label: hoursLabel(Number(hours)) }))}
            value={RETENTION.includes(retention as (typeof RETENTION)[number]) ? retention : "24"}
            disabled={update.isPending || value("keepHistory") === false}
            onValueChange={(hours) => update.mutate({ retentionHours: Number(hours) })}
            className="w-32"
          />
        </GroupedRow>
        <GroupedRow
          label={t("inspector.settings.clear")}
          description={t("inspector.settings.clearDetail")}
        >
          <ConfirmDialog
            trigger={<Button size="sm">{t("inspector.settings.clearButton")}</Button>}
            title={t("inspector.settings.clearTitle")}
            description={t("inspector.settings.clearDetail")}
            confirmLabel={t("inspector.settings.clearConfirm")}
            variant="destructive"
            onConfirm={async () => {
              await clear.mutateAsync(null);
              await refresh(queryClient, queryKeys.inspector.all());
            }}
          />
        </GroupedRow>
      </GroupedSection>
      <GroupedSection
        title={t("inspector.settings.watched")}
        footer={t("inspector.settings.watchedFooter")}
      >
        <div className="flex flex-col gap-2 py-2.5">
          <TextArea
            aria-label={t("inspector.settings.watched")}
            rows={3}
            placeholder="/webhooks/*"
            className="font-mono text-mono"
            value={watched}
            onChange={(event) => setWatched(event.target.value)}
          />
          <div className="flex justify-end">
            <Button
              size="sm"
              disabled={watched.trim() === saved.trim()}
              pending={update.isPending && update.variables?.watchedPaths !== undefined}
              onClick={() => update.mutate({ watchedPaths: paths(watched) })}
            >
              {t("inspector.settings.save")}
            </Button>
          </div>
        </div>
      </GroupedSection>
      {update.error ? (
        <p role="alert" className="px-2.5 text-callout text-error">
          {toIpcError(update.error).message}
        </p>
      ) : null}
    </>
  );
}
