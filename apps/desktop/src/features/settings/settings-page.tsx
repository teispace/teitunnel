import { Cable, CircleUser, type LucideIcon, Settings2 } from "lucide-react";
import { useState } from "react";
import { GroupedRow, GroupedSection } from "@/components/patterns/grouped-list";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { Switch } from "@/components/ui/switch";
import { AccountsPane } from "@/features/accounts";
import { CloudflaredPane } from "@/features/binary";
import { cn } from "@/lib/cn";
import { useOpenAtLogin, useSetOpenAtLogin, useSettings, useUpdateSettings } from "./queries";

const themes = [
  { value: "system", label: "Automatic" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
] as const;

type Tab = "general" | "accounts" | "cloudflared";

const tabs: readonly { id: Tab; label: string; icon: LucideIcon }[] = [
  { id: "general", label: "General", icon: Settings2 },
  { id: "accounts", label: "Accounts", icon: CircleUser },
  { id: "cloudflared", label: "cloudflared", icon: Cable },
];

function OpenAtLogin() {
  const login = useOpenAtLogin();
  const change = useSetOpenAtLogin();
  return (
    <GroupedSection
      title="Startup"
      footer="Teitunnel opens in the menu bar, without a window. Routes set to keep running don't need this."
    >
      <GroupedRow label="Open at login">
        <Switch
          aria-label="Open at login"
          checked={change.isPending ? change.variables : login.data === true}
          disabled={!login.isSuccess || change.isPending}
          onCheckedChange={(enabled) => change.mutate(enabled)}
        />
      </GroupedRow>
    </GroupedSection>
  );
}

/** The Settings window (⌘,): toolbar tabs over System Settings–style forms. */
export function SettingsPage() {
  const [tab, setTab] = useState<Tab>("general");
  const current = tabs.find((t) => t.id === tab) ?? tabs[0];
  return (
    <div className="flex h-full flex-col bg-surface-content">
      <header data-tauri-drag-region="deep" className="shrink-0 border-separator border-b-hairline">
        <h1 className="flex h-7 items-center justify-center text-headline [:root[data-window-active=false]_&]:text-secondary">
          {current?.label}
        </h1>
        <div role="tablist" aria-label="Settings" className="flex justify-center gap-1 pb-1.5">
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
              {label}
            </button>
          ))}
        </div>
      </header>
      <div role="tabpanel" className="flex flex-1 flex-col gap-5 overflow-y-auto px-5 pt-4 pb-6">
        {tab === "general" ? (
          <GeneralPane />
        ) : tab === "accounts" ? (
          <AccountsPane />
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
  if (!settings) return null;
  return (
    <>
      <GroupedSection title="Appearance">
        <GroupedRow label="Appearance">
          <SegmentedControl
            label="Appearance"
            segments={themes}
            value={settings.theme}
            onValueChange={(theme) => update.mutate({ theme })}
          />
        </GroupedRow>
      </GroupedSection>
      <OpenAtLogin />
      <GroupedSection
        title="Notifications"
        footer="Only when no Teitunnel window is in front. macOS decides how they look in System Settings ▸ Notifications."
      >
        <GroupedRow
          label="Routes down or back"
          description="When this Mac's connector loses its connection for more than 20 seconds."
        >
          <Switch
            aria-label="Routes down or back"
            checked={settings.notifyConnectors}
            onCheckedChange={(notifyConnectors) => update.mutate({ notifyConnectors })}
          />
        </GroupedRow>
        <GroupedRow label="Quick Shares" description="When a share goes live or stops working.">
          <Switch
            aria-label="Quick Share notifications"
            checked={settings.notifyQuickShares}
            onCheckedChange={(notifyQuickShares) => update.mutate({ notifyQuickShares })}
          />
        </GroupedRow>
      </GroupedSection>
      <GroupedSection
        title="Menu bar"
        footer="Closing the window keeps routes and Quick Shares running. Quit Teitunnel with ⌘Q to stop them."
      >
        <GroupedRow
          label="Show in menu bar"
          description="Status and quick actions without opening the window."
        >
          <Switch
            aria-label="Show in menu bar"
            checked={settings.showInMenuBar}
            onCheckedChange={(showInMenuBar) => update.mutate({ showInMenuBar })}
          />
        </GroupedRow>
      </GroupedSection>
    </>
  );
}
