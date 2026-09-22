import { createFileRoute } from "@tanstack/react-router";

export const Route = createFileRoute("/settings")({
  component: SettingsPage,
});

function SettingsPage() {
  return (
    <div className="flex h-full flex-col bg-surface-window">
      <div data-tauri-drag-region="deep" className="h-(--toolbar-height) shrink-0" />
      <div className="flex-1 px-5 text-secondary">Settings</div>
    </div>
  );
}
