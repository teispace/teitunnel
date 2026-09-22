import { Cable, type LucideIcon, Settings2 } from "lucide-react";
import { useState } from "react";
import { GroupedRow, GroupedSection } from "@/components/patterns/grouped-list";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { Switch } from "@/components/ui/switch";
import { CloudflaredPane } from "@/features/binary";
import { cn } from "@/lib/cn";
import { useSettings, useUpdateSettings } from "./queries";

const themes = [
  { value: "system", label: "Automatic" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
] as const;

type Tab = "general" | "cloudflared";

const tabs: readonly { id: Tab; label: string; icon: LucideIcon }[] = [
  { id: "general", label: "General", icon: Settings2 },
  { id: "cloudflared", label: "cloudflared", icon: Cable },
];

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
        {tab === "general" ? <GeneralPane /> : <CloudflaredPane />}
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
      <GroupedSection
        title="Menu bar"
        footer="Closing the window keeps Quick Shares running. Quit Teitunnel with ⌘Q to stop them."
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
