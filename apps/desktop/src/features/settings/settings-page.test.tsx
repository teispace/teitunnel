import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import type { Settings, SettingsPatch } from "@/lib/ipc/bindings";
import { SettingsPage } from "./settings-page";

let stored: Settings;
const calls: string[] = [];

beforeEach(() => {
  stored = {
    theme: "system",
    showInMenuBar: true,
    notifyConnectors: true,
    notifyQuickShares: true,
  };
  calls.length = 0;
  mockIPC((cmd, args) => {
    calls.push(cmd);
    if (cmd === "settings_get") return stored;
    if (cmd === "settings_set") {
      const { patch } = args as { patch: SettingsPatch };
      stored = {
        theme: patch.theme ?? stored.theme,
        showInMenuBar: patch.showInMenuBar ?? stored.showInMenuBar,
        notifyConnectors: patch.notifyConnectors ?? stored.notifyConnectors,
        notifyQuickShares: patch.notifyQuickShares ?? stored.notifyQuickShares,
      };
      return stored;
    }
    return null;
  });
});

function renderPage() {
  return render(
    <QueryClientProvider client={createQueryClient()}>
      <SettingsPage />
    </QueryClientProvider>,
  );
}

describe("SettingsPage", () => {
  it("shows the stored settings and saves changes", async () => {
    renderPage();
    const toggle = await screen.findByRole("switch", { name: "Show in menu bar" });
    expect(toggle.getAttribute("aria-checked")).toBe("true");

    fireEvent.click(toggle);
    await waitFor(() => expect(stored.showInMenuBar).toBe(false));
    await waitFor(() => expect(toggle.getAttribute("aria-checked")).toBe("false"));

    fireEvent.click(screen.getByRole("radio", { name: "Dark" }));
    await waitFor(() => expect(stored.theme).toBe("dark"));
    expect(calls.filter((c) => c === "settings_set")).toHaveLength(2);
  });
});
