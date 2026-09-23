import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import type { UpdateStatus } from "@/lib/ipc/bindings";
import { UpdateNotice } from "./update-notice";

let status: UpdateStatus;
let calls: string[];

beforeEach(() => {
  calls = [];
  status = {
    currentVersion: "0.1.0",
    unsupported: null,
    automatic: true,
    lastChecked: null,
    state: { state: "upToDate" },
    installOnQuit: true,
  };
  mockIPC((cmd) => {
    calls.push(cmd);
    return cmd === "updates_status" ? status : null;
  });
});

function renderNotice() {
  return render(
    <QueryClientProvider client={createQueryClient()}>
      <UpdateNotice />
    </QueryClientProvider>,
  );
}

describe("UpdateNotice", () => {
  it("stays hidden until an update is ready", async () => {
    const { container } = renderNotice();
    await waitFor(() => expect(calls).toContain("updates_status"));
    expect(container.textContent).toBe("");
  });

  it("offers to restart into a downloaded update", async () => {
    status = { ...status, state: { state: "ready", version: "0.2.0", notes: null } };
    renderNotice();
    expect(await screen.findByText("Version 0.2.0 is ready to install.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Restart to Update" }));
    await waitFor(() => expect(calls).toContain("updates_restart"));
  });
});
