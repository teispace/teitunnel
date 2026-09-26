import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import type { Browser, BrowserHostStatus, BrowserHostView } from "@/lib/ipc/bindings";
import { BrowserTab } from "./browser-tab";

let view: BrowserHostView;
let calls: { cmd: string; browser: unknown }[];

function browser(
  id: Browser,
  name: string,
  extra: Partial<BrowserHostStatus> = {},
): BrowserHostStatus {
  return {
    browser: id,
    name,
    detected: true,
    app: `/Applications/${name}.app`,
    installed: false,
    manifest: `/Users/me/Library/Application Support/${name}/NativeMessagingHosts/com.teispace.teitunnel.json`,
    extensionSeenAt: null,
    ...extra,
  };
}

beforeEach(() => {
  calls = [];
  view = {
    available: true,
    browsers: [
      browser("chrome", "Google Chrome", {
        installed: true,
        extensionSeenAt: Date.now() - 5 * 60_000,
      }),
      browser("edge", "Microsoft Edge", { detected: false, app: null }),
      browser("firefox", "Firefox"),
    ],
  };
  mockIPC((cmd, args) => {
    const only = (args as { browser?: Browser | null } | undefined)?.browser ?? null;
    calls.push({ cmd, browser: only });
    if (cmd === "browser_host_install" || cmd === "browser_host_uninstall") {
      view = {
        ...view,
        browsers: view.browsers.map((b) =>
          b.detected && (only === null || only === b.browser)
            ? { ...b, installed: cmd === "browser_host_install" }
            : b,
        ),
      };
    }
    return view;
  });
});

const renderIt = () =>
  render(
    <QueryClientProvider client={createQueryClient()}>
      <BrowserTab />
    </QueryClientProvider>,
  );

describe("BrowserTab", () => {
  it("lists only browsers found, with whether the extension has connected", async () => {
    renderIt();
    expect(await screen.findByText("Google Chrome")).toBeTruthy();
    expect(screen.getByText("Set up · extension connected 5 min ago")).toBeTruthy();
    expect(screen.getByText("Not set up")).toBeTruthy();
    // Edge isn't installed: only named under "Not Found".
    expect(screen.getByText("Not Found (1)")).toBeTruthy();
  });

  it("sets up one browser, or all, and removes one", async () => {
    renderIt();
    fireEvent.click(await screen.findByRole("button", { name: "Set Up" }));
    await waitFor(() =>
      expect(calls).toContainEqual({ cmd: "browser_host_install", browser: "firefox" }),
    );
    expect(await screen.findByText("Set up · the extension hasn't connected yet")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Set Up All" })).toBeNull();
    fireEvent.click(screen.getAllByRole("button", { name: "Remove" })[0] as HTMLElement);
    await waitFor(() =>
      expect(calls).toContainEqual({ cmd: "browser_host_uninstall", browser: "chrome" }),
    );
    fireEvent.click(await screen.findByRole("button", { name: "Set Up All" }));
    await waitFor(() =>
      expect(calls).toContainEqual({ cmd: "browser_host_install", browser: null }),
    );
  });

  it("says when it needs the installed app", async () => {
    view = { available: false, browsers: [] };
    renderIt();
    expect(
      await screen.findByText(
        "The browser extension works with the installed app, not this development build.",
      ),
    ).toBeTruthy();
  });
});
