import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import type { BinaryInfo } from "@/lib/ipc/bindings";
import { CloudflaredPane } from "./cloudflared-pane";

let binary: BinaryInfo;
const calls: string[] = [];

beforeEach(() => {
  calls.length = 0;
  binary = {
    path: "/data/bin/cloudflared",
    source: "managed",
    version: "2026.8.0",
    supported: true,
  };
  mockWindows("main");
  mockIPC((cmd) => {
    calls.push(cmd);
    switch (cmd) {
      case "binary_status":
        return binary;
      case "binary_check_update":
        return { latest: "2026.9.1", available: binary.version !== "2026.9.1" };
      case "binary_install":
        binary = { ...binary, version: "2026.9.1" };
        return binary;
      default:
        return null;
    }
  });
});

function renderPane() {
  render(
    <QueryClientProvider client={createQueryClient()}>
      <CloudflaredPane />
    </QueryClientProvider>,
  );
}

describe("CloudflaredPane", () => {
  it("checks for and installs an update of the managed copy", async () => {
    renderPane();
    expect(await screen.findByText("2026.8.0")).toBeTruthy();
    expect(calls).not.toContain("binary_check_update");

    fireEvent.click(screen.getByRole("button", { name: "Check now" }));
    expect(await screen.findByText("Version 2026.9.1 is available")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Update" }));
    expect(await screen.findByText("2026.9.1")).toBeTruthy();
    expect(await screen.findByText("cloudflared is up to date")).toBeTruthy();
  });

  it("never offers to update someone else's installation", async () => {
    binary = { ...binary, source: "system", path: "/opt/homebrew/bin/cloudflared" };
    renderPane();
    expect(await screen.findByText("Updated by its installer")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Check now" })).toBeNull();
  });
});
