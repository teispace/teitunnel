import { describe, expect, it, vi } from "vitest";
import type { MenuCommand } from "@/lib/ipc/bindings";
import { appCommands, runMenuCommand } from "./commands";

const menuCommands: MenuCommand[] = [
  "newRoute",
  "newQuickShare",
  "toggleSidebar",
  "toggleInspector",
  "refresh",
  "goOverview",
  "goRoutes",
  "goQuickShare",
  "goDomains",
  "goTunnels",
  "goActivity",
  "goDoctor",
];

describe("appCommands", () => {
  it("has unique ids and shortcuts", () => {
    const ids = appCommands.map((c) => c.id);
    const shortcuts = appCommands.flatMap((c) => (c.shortcut ? [c.shortcut] : []));
    expect(new Set(ids).size).toBe(ids.length);
    expect(new Set(shortcuts).size).toBe(shortcuts.length);
  });

  it("handles every menu-bar command except the palette", () => {
    const navigate = vi.fn();
    const queryClient = { invalidateQueries: vi.fn() };
    for (const menu of menuCommands) {
      const handled = runMenuCommand(menu, {
        navigate: navigate as never,
        queryClient: queryClient as never,
      });
      expect(handled, menu).toBe(true);
    }
    expect(navigate).toHaveBeenCalledWith({ to: "/doctor" });
    expect(queryClient.invalidateQueries).toHaveBeenCalled();
  });
});
