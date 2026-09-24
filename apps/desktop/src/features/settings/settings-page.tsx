import { Blocks, Cable, CircleUser, type LucideIcon, Settings2 } from "lucide-react";
import { useState } from "react";
import { CopyField } from "@/components/patterns/copy-field";
import { GroupedRow, GroupedSection, SkeletonSection } from "@/components/patterns/grouped-list";
import { Button } from "@/components/ui/button";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { Switch } from "@/components/ui/switch";
import { AccountsPane } from "@/features/accounts";
import { AlertSettings } from "@/features/analytics";
import { CloudflaredPane } from "@/features/binary";
import { UpdateSection } from "@/features/updates";
import { cn } from "@/lib/cn";
import { type MessageKey, t } from "@/lib/i18n";
import { toIpcError } from "@/lib/ipc/client";
import { AiTools } from "./ai-tools";
import { IntegrationsPane } from "./integrations";
import {
  useCliStatus,
  useOpenAtLogin,
  useSetCliInstalled,
  useSetOpenAtLogin,
  useSettings,
  useUpdateSettings,
} from "./queries";
import { QuietHoursRow } from "./quiet-hours";

const themes = () =>
  (["system", "light", "dark"] as const).map((value) => ({
    value,
    label: t(`settings.theme.${value}`),
  }));

type Tab = "general" | "accounts" | "cloudflared" | "integrations";

const tabs: readonly { id: Tab; label: MessageKey; icon: LucideIcon }[] = [
  { id: "general", label: "settings.tab.general", icon: Settings2 },
  { id: "accounts", label: "settings.tab.accounts", icon: CircleUser },
  { id: "cloudflared", label: "settings.tab.cloudflared", icon: Cable },
  { id: "integrations", label: "settings.tab.integrations", icon: Blocks },
];

function OpenAtLogin() {
  const login = useOpenAtLogin();
  const change = useSetOpenAtLogin();
  return (
    <GroupedSection title={t("settings.startup.title")} footer={t("settings.startup.footer")}>
      <GroupedRow label={t("settings.startup.openAtLogin")}>
        <Switch
          aria-label={t("settings.startup.openAtLogin")}
          checked={change.isPending ? change.variables : login.data === true}
          disabled={!login.isSuccess || change.isPending}
          onCheckedChange={(enabled) => change.mutate(enabled)}
        />
      </GroupedRow>
    </GroupedSection>
  );
}

/** Settings ▸ General ▸ Command line: `teitunnel` on the PATH (D-077). */
function CommandLine() {
  const { data: state } = useCliStatus();
  const change = useSetCliInstalled();
  if (!state || state.state === "unavailable") return null;
  const description = (() => {
    switch (state.state) {
      case "packaged":
        return t("settings.cli.packaged", { path: state.path });
      case "installed":
        return t("settings.cli.installed", { path: state.path });
      case "taken":
        return t("settings.cli.taken", { path: state.path });
      case "notInstalled":
        return state.command ? t("settings.cli.manual") : t("settings.cli.notInstalled");
    }
  })();
  return (
    <GroupedSection title={t("settings.cli.title")} footer={t("settings.cli.footer")}>
      <GroupedRow label="teitunnel" description={description}>
        {state.state === "installed" ? (
          <Button size="sm" pending={change.isPending} onClick={() => change.mutate(false)}>
            {t("settings.cli.uninstall")}
          </Button>
        ) : state.state === "notInstalled" && !state.command ? (
          <Button size="sm" pending={change.isPending} onClick={() => change.mutate(true)}>
            {t("settings.cli.install")}
          </Button>
        ) : null}
      </GroupedRow>
      {state.state === "notInstalled" && state.command ? (
        <div className="pb-2">
          <CopyField label={t("settings.cli.command")} value={state.command} />
        </div>
      ) : null}
      {change.error ? (
        <p role="alert" className="pb-2 text-callout text-error">
          {toIpcError(change.error).message}
        </p>
      ) : null}
    </GroupedSection>
  );
}

/** The Settings window (⌘,): toolbar tabs over System Settings–style forms. */
export function SettingsPage() {
  const [tab, setTab] = useState<Tab>("general");
  const current = tabs.find((entry) => entry.id === tab) ?? tabs[0];
  return (
    <div className="flex h-full flex-col bg-surface-content">
      <header data-tauri-drag-region="deep" className="shrink-0 border-separator border-b-hairline">
        <h1 className="flex h-7 items-center justify-center text-headline [:root[data-window-active=false]_&]:text-secondary">
          {current ? t(current.label) : null}
        </h1>
        <div
          role="tablist"
          aria-label={t("settings.tabs")}
          className="flex justify-center gap-1 pb-1.5"
        >
          {tabs.map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              type="button"
              role="tab"
              aria-selected={tab === id}
              onClick={() => setTab(id)}
              className={cn(
                "flex h-12 min-w-16 flex-col items-center justify-center gap-1 rounded-row px-2 text-footnote outline-offset-0",
                tab === id ? "bg-surface-pressed text-primary" : "text-secondary",
              )}
            >
              <Icon
                aria-hidden
                className={cn("size-5", tab === id ? "text-accent" : "text-secondary")}
                strokeWidth={1.6}
              />
              {t(label)}
            </button>
          ))}
        </div>
      </header>
      <div role="tabpanel" className="flex flex-1 flex-col gap-5 overflow-y-auto px-5 pt-4 pb-6">
        {tab === "general" ? (
          <GeneralPane />
        ) : tab === "accounts" ? (
          <AccountsPane />
        ) : tab === "integrations" ? (
          <IntegrationsPane />
        ) : (
          <CloudflaredPane />
        )}
      </div>
    </div>
  );
}

function GeneralPane() {
  const { data: settings } = useSettings();
  const update = useUpdateSettings();
  if (!settings) {
    return (
      <>
        <SkeletonSection />
        <SkeletonSection rows={2} />
        <SkeletonSection rows={3} />
      </>
    );
  }
  return (
    <>
      <GroupedSection>
        <GroupedRow label={t("settings.appearance")}>
          <SegmentedControl
            label={t("settings.appearance")}
            segments={themes()}
            value={settings.theme}
            onValueChange={(theme) => update.mutate({ theme })}
          />
        </GroupedRow>
      </GroupedSection>
      <UpdateSection />
      <OpenAtLogin />
      <CommandLine />
      <AiTools />
      <GroupedSection
        title={t("settings.notifications.title")}
        footer={t("settings.notifications.footer")}
      >
        <GroupedRow
          label={t("settings.notifications.routes")}
          description={t("settings.notifications.routesDetail")}
        >
          <Switch
            aria-label={t("settings.notifications.routes")}
            checked={settings.notifyConnectors}
            onCheckedChange={(notifyConnectors) => update.mutate({ notifyConnectors })}
          />
        </GroupedRow>
        <GroupedRow
          label={t("settings.notifications.problems")}
          description={t("settings.notifications.problemsDetail")}
        >
          <Switch
            aria-label={t("settings.notifications.doctor")}
            checked={settings.notifyDoctor}
            onCheckedChange={(notifyDoctor) => update.mutate({ notifyDoctor })}
          />
        </GroupedRow>
        <GroupedRow
          label={t("settings.notifications.shares")}
          description={t("settings.notifications.sharesDetail")}
        >
          <Switch
            aria-label={t("settings.notifications.sharesLabel")}
            checked={settings.notifyQuickShares}
            onCheckedChange={(notifyQuickShares) => update.mutate({ notifyQuickShares })}
          />
        </GroupedRow>
        <GroupedRow
          label={t("settings.notifications.alerts")}
          description={t("settings.notifications.alertsDetail")}
        >
          <Switch
            aria-label={t("settings.notifications.alertsLabel")}
            checked={settings.notifyAlerts}
            onCheckedChange={(notifyAlerts) => update.mutate({ notifyAlerts })}
          />
        </GroupedRow>
        <QuietHoursRow
          value={settings.quietHours}
          onChange={(quietHours) => update.mutate({ quietHours })}
        />
      </GroupedSection>
      <AlertSettings />
      <GroupedSection title={t("settings.menuBar.title")} footer={t("settings.menuBar.footer")}>
        <GroupedRow
          label={t("settings.menuBar.show")}
          description={t("settings.menuBar.showDetail")}
        >
          <Switch
            aria-label={t("settings.menuBar.show")}
            checked={settings.showInMenuBar}
            onCheckedChange={(showInMenuBar) => update.mutate({ showInMenuBar })}
          />
        </GroupedRow>
      </GroupedSection>
    </>
  );
}
