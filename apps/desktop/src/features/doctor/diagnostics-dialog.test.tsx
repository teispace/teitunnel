import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC } from "@tauri-apps/api/mocks";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import { useUiStore } from "@/app/ui-store";
import { DiagnosticsDialog } from "./diagnostics-dialog";

let calls: { cmd: string; args: Record<string, unknown> }[];

beforeEach(() => {
  calls = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    if (cmd === "diagnostics_preview")
      return [{ name: "summary.txt", size: 120, excerpt: "Teitunnel 0.1.0" }];
    if (cmd === "diagnostics_export") return "/Users/me/Downloads/teitunnel-diagnostics.tar.gz";
    return null;
  });
});

function renderDialog() {
  render(
    <QueryClientProvider client={createQueryClient()}>
      <DiagnosticsDialog />
    </QueryClientProvider>,
  );
  act(() => useUiStore.getState().setDiagnosticsOpen(true));
}

describe("DiagnosticsDialog", () => {
  it("shows the files, and adds cloudflared's report only when asked", async () => {
    renderDialog();
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText("summary.txt")).toBeTruthy();
    fireEvent.click(
      within(dialog).getByRole("checkbox", { name: "Include cloudflared's own report" }),
    );
    fireEvent.click(within(dialog).getByRole("button", { name: "Save to Downloads" }));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "diagnostics_export")?.args).toEqual({
        includeCloudflared: true,
      }),
    );
  });

  it("leaves it out by default", async () => {
    renderDialog();
    const dialog = await screen.findByRole("dialog");
    await within(dialog).findByText("summary.txt");
    fireEvent.click(within(dialog).getByRole("button", { name: "Save to Downloads" }));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "diagnostics_export")?.args).toEqual({
        includeCloudflared: false,
      }),
    );
  });
});
