import { GroupedRow, GroupedSection } from "@/components/patterns/grouped-list";
import { Button } from "@/components/ui/button";
import { ProgressBar } from "@/components/ui/progress-bar";
import { Spinner } from "@/components/ui/spinner";
import { Switch } from "@/components/ui/switch";
import { useSettings, useUpdateSettings } from "@/features/settings/queries";
import { relativeTime } from "@/lib/format";
import { t, translate } from "@/lib/i18n";
import { commands, type UpdateStatus } from "@/lib/ipc/bindings";
import { useCheckForUpdates, useRestartToUpdate, useUpdateStatus } from "./queries";

function describe(status: UpdateStatus): string {
  const { state } = status;
  switch (state.state) {
    case "checking":
      return t("updates.checking");
    case "downloading":
      return state.progress === null
        ? t("updates.downloading", { version: state.version })
        : t("updates.downloadingPercent", {
            version: state.version,
            percent: Math.round(state.progress * 100),
          });
    case "ready":
      return t("updates.ready", { version: state.version });
    case "failed":
      return translate(state.message);
    case "upToDate":
      return t("updates.upToDate");
    case "idle":
      return t("updates.notChecked");
  }
}

/** Settings ▸ General ▸ Updates: the version, automatic checks, and the update itself. */
export function UpdateSection() {
  const { data: status } = useUpdateStatus();
  const { data: settings } = useSettings();
  const setSettings = useUpdateSettings();
  const check = useCheckForUpdates();
  const restart = useRestartToUpdate();
  if (!status || !settings) return null;

  const { state } = status;
  const busy = state.state === "checking" || state.state === "downloading" || check.isPending;
  const lastChecked =
    status.lastChecked === null
      ? null
      : t("updates.lastChecked", { when: relativeTime(status.lastChecked) });
  const footer = status.unsupported
    ? translate(status.unsupported)
    : state.state === "ready"
      ? status.installOnQuit
        ? t("updates.installsOnQuit")
        : t("updates.installsOnRestart")
      : t("updates.footer");

  return (
    <GroupedSection title={t("updates.title")} footer={footer}>
      <GroupedRow
        label={t("updates.version", { version: status.currentVersion })}
        description={
          status.unsupported ? null : [describe(status), lastChecked].filter(Boolean).join(" · ")
        }
      >
        {status.unsupported ? null : state.state === "ready" ? (
          <>
            <Button size="sm" onClick={() => void commands.appOpenHelp("releaseNotes")}>
              {t("updates.releaseNotes")}
            </Button>
            <Button
              size="sm"
              variant="primary"
              disabled={restart.isPending}
              onClick={() => restart.mutate()}
            >
              {t("updates.restart")}
            </Button>
          </>
        ) : busy ? (
          <Spinner label={t("updates.checking")} />
        ) : (
          <Button size="sm" onClick={() => check.mutate()}>
            {t("updates.checkNow")}
          </Button>
        )}
      </GroupedRow>
      {state.state === "downloading" ? (
        <ProgressBar
          {...(state.progress === null ? {} : { value: state.progress })}
          label={t("updates.downloading", { version: state.version })}
          className="mb-2"
        />
      ) : null}
      <GroupedRow label={t("updates.automatic")}>
        <Switch
          aria-label={t("updates.automatic")}
          checked={settings.checkForUpdates && !status.unsupported}
          disabled={status.unsupported !== null}
          onCheckedChange={(checkForUpdates) => setSettings.mutate({ checkForUpdates })}
        />
      </GroupedRow>
    </GroupedSection>
  );
}
