import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import type { Integrations, IntegrationsPatch } from "@/lib/ipc/bindings";
import { IntegrationsPane } from "./integrations";

let stored: Integrations;
const calls: string[] = [];
let release: (() => void) | null = null;

beforeEach(() => {
  stored = {
    controlEnabled: true,
    deepLinksEnabled: true,
    clients: [
      { name: "teitunnel-cli", version: "0.2.0", approvedAt: Date.UTC(2026, 8, 20) },
      { name: "vscode", version: "1.0.0", approvedAt: Date.UTC(2026, 8, 22) },
    ],
  };
  calls.length = 0;
  release = null;
  mockIPC(async (cmd, args) => {
    calls.push(cmd);
    if (cmd === "integrations_get") return stored;
    if (cmd === "integrations_set") {
      const { patch } = args as { patch: IntegrationsPatch };
      stored = {
        ...stored,
        controlEnabled: patch.controlEnabled ?? stored.controlEnabled,
        deepLinksEnabled: patch.deepLinksEnabled ?? stored.deepLinksEnabled,
      };
      return stored;
    }
    if (cmd === "integrations_revoke") {
      const { name } = args as { name: string };
      await new Promise<void>((resolve) => {
        release = resolve;
      });
      stored = { ...stored, clients: stored.clients.filter((c) => c.name !== name) };
      return stored;
    }
    return null;
  });
});

function renderPane() {
  return render(
    <QueryClientProvider client={createQueryClient()}>
      <IntegrationsPane />
    </QueryClientProvider>,
  );
}

describe("IntegrationsPane", () => {
  it("turns the control connection and links off", async () => {
    renderPane();
    const control = await screen.findByRole("switch", { name: "Allow connections" });
    expect(control.getAttribute("aria-checked")).toBe("true");
    fireEvent.click(control);
    await waitFor(() => expect(stored.controlEnabled).toBe(false));
    await waitFor(() => expect(control.getAttribute("aria-checked")).toBe("false"));
    fireEvent.click(screen.getByRole("switch", { name: "Open teitunnel:// links" }));
    await waitFor(() => expect(stored.deepLinksEnabled).toBe(false));
  });

  it("lists always-allowed programs, newest first, and removes one", async () => {
    renderPane();
    const rows = await screen.findAllByRole("button", { name: /Stop always allowing/ });
    expect(rows.map((row) => row.getAttribute("aria-label"))).toEqual([
      "Stop always allowing vscode",
      "Stop always allowing teitunnel-cli",
    ]);
    const remove = screen.getByRole("button", { name: "Stop always allowing vscode" });
    fireEvent.click(remove);
    // Busy until the list no longer has it.
    await waitFor(() => expect(remove.getAttribute("aria-busy")).toBe("true"));
    release?.();
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "Stop always allowing vscode" })).toBeNull(),
    );
    expect(calls).toContain("integrations_revoke");
  });

  it("says when no program is always allowed", async () => {
    stored = { ...stored, clients: [] };
    renderPane();
    expect(await screen.findByText(/No program is always allowed/)).toBeTruthy();
  });
});
