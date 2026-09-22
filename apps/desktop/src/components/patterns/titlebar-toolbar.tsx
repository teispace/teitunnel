import { PanelLeft } from "lucide-react";
import type { ReactNode } from "react";
import { useUiStore } from "@/app/ui-store";
import { IconButton } from "@/components/ui/icon-button";
import { cn } from "@/lib/cn";

interface TitlebarToolbarProps {
  title: ReactNode;
  children?: ReactNode;
  /** Show the hairline under the band, e.g. once content scrolls beneath it. */
  separator?: boolean;
}

/**
 * The unified 52 px title bar + toolbar band above the content pane. The whole band is a
 * window drag region; buttons and inputs inside it stay interactive.
 */
export function TitlebarToolbar({ title, children, separator = false }: TitlebarToolbarProps) {
  const collapsed = useUiStore((state) => state.sidebarCollapsed);
  const toggleSidebar = useUiStore((state) => state.toggleSidebar);
  return (
    <header
      data-tauri-drag-region="deep"
      className={cn(
        "flex h-(--toolbar-height) shrink-0 items-center gap-3 border-b-hairline pr-3 pl-5",
        collapsed && "pl-(--traffic-light-inset)",
        "transition-colors transition-smooth",
        separator ? "border-separator" : "border-transparent",
      )}
    >
      {collapsed ? (
        <IconButton
          icon={PanelLeft}
          label="Show sidebar"
          onClick={toggleSidebar}
          className="-ml-2"
        />
      ) : null}
      <h1 className="min-w-0 truncate text-title3 [:root[data-window-active=false]_&]:text-secondary">
        {title}
      </h1>
      {children ? <div className="ml-auto flex items-center gap-2">{children}</div> : null}
    </header>
  );
}
