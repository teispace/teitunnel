import { GroupedRow, GroupedSection } from "@/components/patterns/grouped-list";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { Switch } from "@/components/ui/switch";
import { useSettings, useUpdateSettings } from "./queries";

const themes = [
  { value: "system", label: "Automatic" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
] as const;

/** The Settings window (⌘,): a compact, System Settings–style form. */
export function SettingsPage() {
  const { data: settings } = useSettings();
  const update = useUpdateSettings();
  return (
    <div className="flex h-full flex-col bg-surface-content">
      <header
        data-tauri-drag-region="deep"
        className="flex h-(--toolbar-height) shrink-0 items-center justify-center"
      >
        <h1 className="text-headline [:root[data-window-active=false]_&]:text-secondary">
          General
        </h1>
      </header>
      <div className="flex flex-1 flex-col gap-5 overflow-y-auto px-5 pt-1 pb-6">
        {settings ? (
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
              footer="Closing the window keeps tunnels running. Quit Teitunnel with ⌘Q."
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
        ) : null}
      </div>
    </div>
  );
}
