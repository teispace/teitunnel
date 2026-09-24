import type { QueryClient } from "@tanstack/react-query";
import type { useNavigate } from "@tanstack/react-router";
import {
  BookOpen,
  FileArchive,
  type LucideIcon,
  MessageSquareWarning,
  PanelLeft,
  PanelRight,
  Plus,
  Power,
  RefreshCw,
  ScrollText,
  Search,
  Settings,
  Share,
} from "lucide-react";
import type { MessageKey } from "@/lib/i18n";
import { type HelpLink, commands as ipc, type MenuCommand } from "@/lib/ipc/bindings";
import { navItems } from "./navigation";
import type { Platform } from "./platform";
import type { Shortcut } from "./shortcuts";
import { useUiStore } from "./ui-store";

export interface CommandContext {
  navigate: ReturnType<typeof useNavigate>;
  queryClient: QueryClient;
}

export interface AppCommand {
  id: string;
  title: MessageKey;
  group: "go" | "actions" | "view" | "help";
  icon: LucideIcon;
  /** Its keyboard shortcut (⌘ on macOS, Ctrl elsewhere). */
  shortcut?: Shortcut;
  /** Only on these platforms (default: all). */
  platforms?: readonly Platform[];
  /** The menu-bar item that triggers it, if any. */
  menu?: MenuCommand;
  run: (context: CommandContext) => void;
}

const helpLinks: readonly [HelpLink, MessageKey, LucideIcon][] = [
  ["docs", "commands.docs", BookOpen],
  ["cloudflareDocs", "commands.cloudflareDocs", BookOpen],
  ["releaseNotes", "commands.releaseNotes", ScrollText],
  ["reportIssue", "commands.reportIssue", MessageSquareWarning],
];

const goMenu: readonly MenuCommand[] = [
  "goOverview",
  "goRoutes",
  "goQuickShare",
  "goSnapshots",
  "goDomains",
  "goTunnels",
  "goActivity",
  "goDoctor",
  "goAnalytics",
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
      group: "go",
      icon: item.icon,
      shortcut: { key: String(index + 1) },
      ...(goMenu[index] ? { menu: goMenu[index] } : {}),
      run: ({ navigate }) => void navigate({ to: item.to }),
    }),
  ),
  {
    id: "new-route",
    title: "commands.newRoute",
    group: "actions",
    icon: Plus,
    shortcut: { key: "N" },
    menu: "newRoute",
    run: ({ navigate }) => void navigate({ to: "/routes", search: { add: true } }),
  },
  {
    id: "new-quick-share",
    title: "commands.newQuickShare",
    group: "actions",
    icon: Share,
    shortcut: { key: "N", shift: true },
    menu: "newQuickShare",
    run: ({ navigate }) => void navigate({ to: "/quick-share", search: { compose: true } }),
  },
  {
    id: "refresh",
    title: "commands.refresh",
    group: "actions",
    icon: RefreshCw,
    shortcut: { key: "R" },
    menu: "refresh",
    run: ({ queryClient }) => void queryClient.invalidateQueries(),
  },
  {
    id: "settings",
    title: "commands.settings",
    group: "actions",
    icon: Settings,
    shortcut: { key: "," },
    run: () => void ipc.appOpenSettings(),
  },
  {
    id: "export-diagnostics",
    title: "commands.exportDiagnostics",
    group: "actions",
    icon: FileArchive,
    menu: "exportDiagnostics",
    run: ({ navigate }) => {
      void navigate({ to: "/doctor" });
      useUiStore.getState().setDiagnosticsOpen(true);
    },
  },
  {
    id: "toggle-sidebar",
    title: "commands.toggleSidebar",
    group: "view",
    icon: PanelLeft,
    shortcut: { key: "S", alt: true },
    menu: "toggleSidebar",
    run: () => useUiStore.getState().toggleSidebar(),
  },
  {
    id: "toggle-inspector",
    title: "commands.toggleInspector",
    group: "view",
    icon: PanelRight,
    shortcut: { key: "I", alt: true },
    menu: "toggleInspector",
    run: () => useUiStore.getState().toggleInspector(),
  },
  {
    id: "command-palette",
    title: "commands.palette",
    group: "view",
    icon: Search,
    shortcut: { key: "K" },
    run: () => useUiStore.getState().setPaletteOpen(true),
  },
  ...helpLinks.map(
    ([link, title, icon]): AppCommand => ({
      id: `help:${link}`,
      title,
      group: "help",
      icon,
      run: () => void ipc.appOpenHelp(link),
    }),
  ),
  {
    id: "quit",
    title: "commands.quit",
    group: "actions",
    icon: Power,
    shortcut: { key: "Q" },
    // macOS quits from the app menu (⌘Q), which also runs this confirmation.
    platforms: ["windows", "linux"],
    run: () => void ipc.appRequestQuit(),
  },
];

/** The commands available on `platform`. */
export function commandsFor(platform: Platform): AppCommand[] {
  return appCommands.filter((c) => !c.platforms || c.platforms.includes(platform));
}

/** Runs the command bound to a menu-bar item. The palette itself is handled by the caller. */
export function runMenuCommand(menu: MenuCommand, context: CommandContext): boolean {
  const command = appCommands.find((candidate) => candidate.menu === menu);
  command?.run(context);
  return command !== undefined;
}
