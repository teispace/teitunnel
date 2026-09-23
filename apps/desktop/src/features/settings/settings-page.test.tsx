import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import type { Settings, SettingsPatch, UpdateStatus } from "@/lib/ipc/bindings";
import { SettingsPage } from "./settings-page";

let stored: Settings;
let update: UpdateStatus;
const calls: string[] = [];

beforeEach(() => {
  stored = {
    theme: "system",
    showInMenuBar: true,
    notifyConnectors: true,
    notifyQuickShares: true,
    notifyDoctor: true,
    checkForUpdates: true,
    ignoredIssues: [],
  };
  update = {
    currentVersion: "0.1.0",
    unsupported: null,
    automatic: true,
    lastChecked: null,
    state: { state: "idle" },
    installOnQuit: true,
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
        notifyDoctor: patch.notifyDoctor ?? stored.notifyDoctor,
        checkForUpdates: patch.checkForUpdates ?? stored.checkForUpdates,
        ignoredIssues: stored.ignoredIssues,
      };
      return stored;
    }
    if (cmd === "updates_status") return update;
    if (cmd === "updates_check") {
      update = {
        ...update,
        lastChecked: Date.now(),
        state: { state: "ready", version: "0.2.0", notes: null },
      };
      return update;
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

  it("checks for updates on request and restarts into a downloaded one", async () => {
    renderPage();
    expect(await screen.findByText("Teitunnel 0.1.0")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Check Now" }));
    expect(await screen.findByText(/Version 0\.2\.0 is ready to install/)).toBeTruthy();
    expect(screen.getByText(/installs when you quit Teitunnel/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Restart to Update" }));
    await waitFor(() => expect(calls).toContain("updates_restart"));
  });

  it("turns automatic update checks off", async () => {
    renderPage();
    const toggle = await screen.findByRole("switch", { name: "Check for updates automatically" });
    fireEvent.click(toggle);
    await waitFor(() => expect(stored.checkForUpdates).toBe(false));
  });

  it("explains when this copy can't update itself", async () => {
    update = {
      ...update,
      unsupported: { key: "core.updates.development", args: {} },
    };
    renderPage();
    expect(await screen.findByText("Development builds don't update themselves.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Check Now" })).toBeNull();
    const toggle = screen.getByRole("switch", { name: "Check for updates automatically" });
    expect(toggle.hasAttribute("disabled")).toBe(true);
  });
});
