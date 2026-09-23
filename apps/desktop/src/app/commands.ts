import type { QueryClient } from "@tanstack/react-query";
import type { useNavigate } from "@tanstack/react-router";
import {
  FileArchive,
  type LucideIcon,
  PanelLeft,
  PanelRight,
  Plus,
  RefreshCw,
  Settings,
  Share,
} from "lucide-react";
import { commands as ipc, type MenuCommand } from "@/lib/ipc/bindings";
import { navItems } from "./navigation";
import { useUiStore } from "./ui-store";

export interface CommandContext {
  navigate: ReturnType<typeof useNavigate>;
  queryClient: QueryClient;
}

export interface AppCommand {
  id: string;
  title: string;
  group: "Go to" | "Actions" | "View";
  icon: LucideIcon;
  /** Display form of the shortcut, as shown in menus. */
  shortcut?: string;
  /** The menu-bar item that triggers it, if any. */
  menu?: MenuCommand;
  run: (context: CommandContext) => void;
}

const goMenu: readonly MenuCommand[] = [
  "goOverview",
  "goRoutes",
  "goQuickShare",
  "goDomains",
  "goTunnels",
  "goActivity",
  "goDoctor",
];

/**
 * Every app-level command, in one place. The menu bar, the command palette and
 * keyboard shortcuts all dispatch through this list, so they can't drift apart.
 */
export const appCommands: readonly AppCommand[] = [
  ...navItems.map(
    (item, index): AppCommand => ({
      id: `go:${item.to}`,
      title: item.label,
      group: "Go to",
      icon: item.icon,
      shortcut: `⌘${index + 1}`,
      ...(goMenu[index] ? { menu: goMenu[index] } : {}),
      run: ({ navigate }) => void navigate({ to: item.to }),
    }),
  ),
  {
    id: "new-route",
    title: "New Route",
    group: "Actions",
    icon: Plus,
    shortcut: "⌘N",
    menu: "newRoute",
    run: ({ navigate }) => void navigate({ to: "/routes", search: { add: true } }),
  },
  {
    id: "new-quick-share",
    title: "New Quick Share",
    group: "Actions",
    icon: Share,
    shortcut: "⇧⌘N",
    menu: "newQuickShare",
    run: ({ navigate }) => void navigate({ to: "/quick-share", search: { compose: true } }),
  },
  {
    id: "refresh",
    title: "Refresh",
    group: "Actions",
    icon: RefreshCw,
    shortcut: "⌘R",
    menu: "refresh",
    run: ({ queryClient }) => void queryClient.invalidateQueries(),
  },
  {
    id: "settings",
    title: "Settings",
    group: "Actions",
    icon: Settings,
    shortcut: "⌘,",
    run: () => void ipc.appOpenSettings(),
  },
  {
    id: "export-diagnostics",
    title: "Export Diagnostics",
    group: "Actions",
    icon: FileArchive,
    menu: "exportDiagnostics",
    run: ({ navigate }) => {
      void navigate({ to: "/doctor" });
      useUiStore.getState().setDiagnosticsOpen(true);
    },
  },
  {
    id: "toggle-sidebar",
    title: "Toggle Sidebar",
    group: "View",
    icon: PanelLeft,
    shortcut: "⌥⌘S",
    menu: "toggleSidebar",
    run: () => useUiStore.getState().toggleSidebar(),
  },
  {
    id: "toggle-inspector",
    title: "Toggle Inspector",
    group: "View",
    icon: PanelRight,
    shortcut: "⌥⌘I",
    menu: "toggleInspector",
    run: () => useUiStore.getState().toggleInspector(),
  },
];

/** Runs the command bound to a menu-bar item. The palette itself is handled by the caller. */
export function runMenuCommand(menu: MenuCommand, context: CommandContext): boolean {
  const command = appCommands.find((candidate) => candidate.menu === menu);
  command?.run(context);
  return command !== undefined;
}
