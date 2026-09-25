import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import type { CliState, Settings, SettingsPatch } from "@/lib/ipc/bindings";
import { CliOffer } from "./cli-offer";

let stored: Settings;
let cli: CliState;
let calls: string[];

beforeEach(() => {
  calls = [];
  stored = {
    theme: "system",
    showInMenuBar: true,
    notifyConnectors: true,
    notifyQuickShares: true,
    notifyDoctor: true,
    notifyAlerts: true,
    quietHours: { enabled: false, from: 1320, to: 420 },
    checkForUpdates: true,
    cliOfferDismissed: false,
    exposureCheck: true,
    ignoredIssues: [],
  };
  cli = { state: "notInstalled", path: "/opt/homebrew/bin/teitunnel", command: null };
  mockIPC((cmd, args) => {
    calls.push(cmd);
    if (cmd === "settings_get") return stored;
    if (cmd === "settings_set") {
      const { patch } = args as { patch: SettingsPatch };
      stored = {
        ...stored,
        cliOfferDismissed: patch.cliOfferDismissed ?? stored.cliOfferDismissed,
      };
      return stored;
    }
    if (cmd === "cli_status") return cli;
    if (cmd === "cli_install") {
      cli = { state: "installed", path: "/opt/homebrew/bin/teitunnel" };
      return cli;
    }
    return null;
  });
});

function renderOffer() {
  return render(
    <QueryClientProvider client={createQueryClient()}>
      <CliOffer />
    </QueryClientProvider>,
  );
}

describe("CliOffer", () => {
  it("installs the command and doesn't ask again", async () => {
    const { container } = renderOffer();
    fireEvent.click(await screen.findByRole("button", { name: "Install" }));
    await waitFor(() => expect(stored.cliOfferDismissed).toBe(true));
    expect(calls).toContain("cli_install");
    await waitFor(() => expect(container.textContent).toBe(""));
  });

  it("remembers Not Now without installing anything", async () => {
    const { container } = renderOffer();
    fireEvent.click(await screen.findByRole("button", { name: "Not Now" }));
    await waitFor(() => expect(stored.cliOfferDismissed).toBe(true));
    expect(calls).not.toContain("cli_install");
    await waitFor(() => expect(container.textContent).toBe(""));
  });

  it("shows the command to run when Teitunnel can't write the folder", async () => {
    cli = {
      state: "notInstalled",
      path: "/usr/local/bin/teitunnel",
      command:
        "sudo ln -s '/Applications/Teitunnel.app/Contents/MacOS/teitunnel-cli' '/usr/local/bin/teitunnel'",
    };
    renderOffer();
    expect(await screen.findByText(/sudo ln -s/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Install" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Done" }));
    await waitFor(() => expect(stored.cliOfferDismissed).toBe(true));
  });

  it.each<[string, CliState]>([
    ["the installer or a package put it there", { state: "installed", path: "/x/teitunnel" }],
    ["a Linux package installed it", { state: "packaged", path: "/usr/bin/teitunnel" }],
    ["another program has the name", { state: "taken", path: "/usr/local/bin/teitunnel" }],
    ["the build has no CLI", { state: "unavailable" }],
  ])("stays hidden when %s", async (_, state) => {
    cli = state;
    const { container } = renderOffer();
    await waitFor(() => expect(calls).toContain("cli_status"));
    expect(container.textContent).toBe("");
  });

  it("stays hidden once answered", async () => {
    stored = { ...stored, cliOfferDismissed: true };
    const { container } = renderOffer();
    await waitFor(() => expect(calls).toContain("settings_get"));
    expect(container.textContent).toBe("");
  });
});
