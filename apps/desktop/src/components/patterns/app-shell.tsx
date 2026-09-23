import type { ReactNode } from "react";
import { usePaneSize, useUiStore } from "@/app/ui-store";
import { cn } from "@/lib/cn";
import { t } from "@/lib/i18n";
import { ResizeHandle } from "./resize-handle";

export const SIDEBAR = { default: 220, min: 180, max: 300 } as const;

/**
 * Window layout: the sidebar sits on the native vibrancy material, the content pane is
 * opaque (DESIGN §2). The sidebar is resizable, and collapsible with ⌘⌥S.
 */
export function AppShell({ sidebar, children }: { sidebar: ReactNode; children: ReactNode }) {
  const collapsed = useUiStore((state) => state.sidebarCollapsed);
  const [width, setWidth] = usePaneSize("sidebar", SIDEBAR.default);
  return (
    <div
      data-sidebar-collapsed={collapsed}
      className="flex h-full"
      style={{ "--sidebar-width": `${width}px` } as React.CSSProperties}
    >
      <div
        inert={collapsed}
        className={cn(
          "relative shrink-0 transition-[margin-left] transition-smooth",
          collapsed && "-ml-(--sidebar-width)",
        )}
      >
        {sidebar}
        <ResizeHandle
          edge="right"
          label={t("shell.resizeSidebar")}
          size={width}
          min={SIDEBAR.min}
          max={SIDEBAR.max}
          onResize={setWidth}
        />
      </div>
      <main className="flex min-w-0 flex-1 flex-col bg-surface-content">{children}</main>
    </div>
  );
}
