import { useEffect, useState } from "react";
import { detectPlatform } from "@/app/platform";
import { GroupedRow, GroupedSection, SkeletonSection } from "@/components/patterns/grouped-list";
import { Button } from "@/components/ui/button";
import { Kbd } from "@/components/ui/kbd";
import { Select } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { t } from "@/lib/i18n";
import type {
  GlobalShortcut,
  Integrations,
  IntegrationsPatch,
  ShortcutAction,
} from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { BrowserExtensionSection } from "./browser-extension";
import { acceleratorFromEvent, formatAccelerator } from "./global-shortcut";
import { useIntegrations, useRevokeClient, useUpdateIntegrations } from "./queries";

const day = new Intl.DateTimeFormat(undefined, { dateStyle: "medium" });

/**
 * Settings ▸ Integrations ▸ Global shortcut: off by default; the keys are recorded by
 * typing them, and the shell registers them (a shortcut another app has is refused).
 */
function ShortcutSection({
  shortcut,
  disabled,
  onChange,
}: {
  shortcut: GlobalShortcut;
  disabled: boolean;
  onChange: (shortcut: GlobalShortcut) => void;
}) {
  const platform = detectPlatform();
  const [recording, setRecording] = useState(false);
  useEffect(() => {
    if (!recording) return;
    const onKey = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
      if (event.key === "Escape") {
        setRecording(false);
        return;
      }
      const keys = acceleratorFromEvent(event, platform);
      if (!keys) return;
      setRecording(false);
      onChange({ ...shortcut, keys });
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [recording, platform, shortcut, onChange]);
  const actions: { value: ShortcutAction; label: string }[] = [
    { value: "shareDevServer", label: t("integrations.shortcut.shareDevServer") },
    { value: "openQuickShare", label: t("integrations.shortcut.openQuickShare") },
  ];
  return (
    <GroupedSection
      title={t("integrations.shortcut.title")}
      footer={t("integrations.shortcut.footer")}
    >
      <GroupedRow
        label={t("integrations.shortcut.enabled")}
        description={t("integrations.shortcut.enabledDetail")}
      >
        <Switch
          aria-label={t("integrations.shortcut.enabled")}
          checked={shortcut.enabled}
          disabled={disabled}
          onCheckedChange={(enabled) => onChange({ ...shortcut, enabled })}
        />
      </GroupedRow>
      <GroupedRow label={t("integrations.shortcut.keys")}>
        {recording ? (
          <span role="status" className="text-callout text-secondary">
            {t("integrations.shortcut.recording")}
          </span>
        ) : (
          <Kbd keys={formatAccelerator(shortcut.keys, platform)} />
        )}
        <Button
          size="sm"
          disabled={disabled}
          aria-label={
            recording
              ? t("integrations.shortcut.cancelRecord")
              : t("integrations.shortcut.recordLabel")
          }
          onClick={() => setRecording((now) => !now)}
        >
          {recording ? t("integrations.shortcut.cancelRecord") : t("integrations.shortcut.record")}
        </Button>
      </GroupedRow>
      <GroupedRow
        label={t("integrations.shortcut.action")}
        description={
          shortcut.action === "shareDevServer"
            ? t("integrations.shortcut.shareDevServerDetail")
            : t("integrations.shortcut.openQuickShareDetail")
        }
      >
        <Select
          label={t("integrations.shortcut.action")}
          options={actions}
          value={shortcut.action}
          disabled={disabled}
          onValueChange={(action) => onChange({ ...shortcut, action })}
        />
      </GroupedRow>
    </GroupedSection>
  );
}

/** Settings ▸ Integrations: the control connection, links and always-allowed programs. */
export function IntegrationsPane() {
  const { data } = useIntegrations();
  const update = useUpdateIntegrations();
  const revoke = useRevokeClient();
  if (!data) {
    return (
      <>
        <SkeletonSection />
        <SkeletonSection rows={2} />
        <SkeletonSection />
      </>
    );
  }
  // Show the switch where it's going while the change is saved.
  const pending = update.isPending ? update.variables : undefined;
  const value = <K extends keyof IntegrationsPatch & keyof Integrations>(key: K) =>
    pending?.[key] ?? data[key];
  const error = update.error ?? revoke.error;
  return (
    <>
      <GroupedSection
        title={t("integrations.control.title")}
        footer={t("integrations.control.footer")}
      >
        <GroupedRow
          label={t("integrations.control.enabled")}
          description={t("integrations.control.enabledDetail")}
        >
          <Switch
            aria-label={t("integrations.control.enabled")}
            checked={value("controlEnabled")}
            disabled={update.isPending}
            onCheckedChange={(controlEnabled) => update.mutate({ controlEnabled })}
          />
        </GroupedRow>
      </GroupedSection>
      <GroupedSection
        title={t("integrations.clients.title")}
        footer={t("integrations.clients.footer")}
      >
        {data.clients.length === 0 ? (
          <p className="py-2 text-callout text-secondary">{t("integrations.clients.empty")}</p>
        ) : (
          [...data.clients].reverse().map((client) => {
            const busy = revoke.isPending && revoke.variables === client.name;
            return (
              <GroupedRow
                key={client.name}
                label={client.name}
                description={t("integrations.clients.approved", {
                  date: day.format(new Date(client.approvedAt)),
                })}
              >
                <Button
                  size="sm"
                  pending={busy}
                  disabled={revoke.isPending && !busy}
                  aria-label={t("integrations.clients.revokeLabel", { name: client.name })}
                  onClick={() => revoke.mutate(client.name)}
                >
                  {t("integrations.clients.revoke")}
                </Button>
              </GroupedRow>
            );
          })
        )}
      </GroupedSection>
      <GroupedSection title={t("integrations.links.title")} footer={t("integrations.links.footer")}>
        <GroupedRow
          label={t("integrations.links.enabled")}
          description={t("integrations.links.enabledDetail")}
        >
          <Switch
            aria-label={t("integrations.links.enabled")}
            checked={value("deepLinksEnabled")}
            disabled={update.isPending}
            onCheckedChange={(deepLinksEnabled) => update.mutate({ deepLinksEnabled })}
          />
        </GroupedRow>
      </GroupedSection>
      <BrowserExtensionSection />
      <ShortcutSection
        shortcut={value("shortcut")}
        disabled={update.isPending}
        onChange={(shortcut) => update.mutate({ shortcut })}
      />
      {error ? (
        <p role="alert" className="text-callout text-error">
          {t("integrations.error", { message: toIpcError(error).message })}
        </p>
      ) : null}
    </>
  );
}
