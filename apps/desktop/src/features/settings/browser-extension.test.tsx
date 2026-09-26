import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import type { BrowserHostView } from "@/lib/ipc/bindings";
import { BrowserExtensionSection } from "./browser-extension";

let view: BrowserHostView;
let calls: string[];

const browsers = (installed: boolean): BrowserHostView => ({
  available: true,
  browsers: [
    { browser: "chrome", name: "Google Chrome", detected: true, installed },
    { browser: "edge", name: "Microsoft Edge", detected: false, installed: false },
    { browser: "firefox", name: "Firefox", detected: true, installed },
  ],
});

beforeEach(() => {
  calls = [];
  view = browsers(false);
  mockIPC((cmd) => {
    calls.push(cmd);
    if (cmd === "browser_host_status") return view;
    if (cmd === "browser_host_install") return browsers(true);
    if (cmd === "browser_host_uninstall") return browsers(false);
    return null;
  });
});

const renderIt = () =>
  render(
    <QueryClientProvider client={createQueryClient()}>
      <BrowserExtensionSection />
    </QueryClientProvider>,
  );

describe("the browser extension", () => {
  it("sets it up in the installed browsers, and removes it", async () => {
    renderIt();
    expect(await screen.findByText("Google Chrome")).toBeTruthy();
    expect(screen.queryByText("Microsoft Edge")).toBeNull();
    expect(screen.getAllByText("Not set up")).toHaveLength(2);
    fireEvent.click(screen.getByRole("button", { name: "Set Up" }));
    await waitFor(() => expect(screen.getAllByText("Ready")).toHaveLength(2));
    expect(calls).toContain("browser_host_install");
    fireEvent.click(screen.getByRole("button", { name: "Remove" }));
    await waitFor(() => expect(calls).toContain("browser_host_uninstall"));
  });

  it("says when it isn't available here", async () => {
    view = { available: false, browsers: [] };
    renderIt();
    expect(await screen.findByText("Available in the installed app.")).toBeTruthy();
  });
});
