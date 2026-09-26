import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createQueryClient } from "@/app/query-client";
import { TooltipProvider } from "@/components/ui/tooltip";
import { inspectorMock, mockTaps } from "@/dev/mock-inspector";
import type { QuickShare } from "@/lib/ipc/bindings";
import { InspectRouteSection } from "./components/inspect-route-section";
import { InspectorSettingsPane } from "./components/inspector-settings";
import { ShareInspectSwitch } from "./components/share-inspect";
import { ProtectShareButton, ShareProtectionNote } from "./components/share-protect";
import { TapSettingsSheet } from "./components/tap-settings-sheet";
import { rangesKept, TapStatsSheet } from "./components/tap-stats-sheet";

const navigate = vi.fn();
vi.mock("@tanstack/react-router", async (original) => ({
  ...(await original<typeof import("@tanstack/react-router")>()),
  useNavigate: () => navigate,
}));

let calls: { cmd: string; args: Record<string, unknown> }[];

beforeEach(() => {
  calls = [];
  mockWindows("main");
  mockIPC((cmd, args) => {
    const payload = (args ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args: payload });
    if (cmd === "doctor_run") return [];
    if (cmd === "settings_get") return { ignoredIssues: [] };
    if (cmd === "quick_share_set_inspected") return { ...share, inspected: payload["inspect"] };
    if (cmd === "inspect_routes") return [];
    return inspectorMock(cmd, payload) ?? null;
  });
});

const wrap = (children: ReactNode) =>
  render(
    <QueryClientProvider client={createQueryClient()}>
      <TooltipProvider>{children}</TooltipProvider>
    </QueryClientProvider>,
  );
const called = (cmd: string) => calls.filter((c) => c.cmd === cmd);

const share: QuickShare = {
  id: "qs-1",
  origin: "http://localhost:5173",
  url: "https://a.trycloudflare.com",
  status: { status: "live" },
  startedAt: 0,
  stopAt: null,
  hostHeader: null,
  check: null,
  inspected: true,
  paused: false,
  folder: null,
};

describe("Settings ▸ Inspector", () => {
  it("changes defaults, history and watched paths", async () => {
    wrap(<InspectorSettingsPane />);
    const shares = await screen.findByRole("switch", { name: "Inspect Quick Shares" });
    expect(shares.getAttribute("aria-checked")).toBe("true");
    fireEvent.click(shares);
    await waitFor(() =>
      expect(called("inspect_settings_set")[0]?.args["patch"]).toEqual({
        inspectQuickShares: false,
      }),
    );
    fireEvent.click(screen.getByRole("switch", { name: "Keep recent requests" }));
    await waitFor(() =>
      expect(called("inspect_settings_set")[1]?.args["patch"]).toEqual({ keepHistory: false }),
    );
    const watched = screen.getByRole("textbox", { name: "Watched paths" });
    await waitFor(() => expect((watched as HTMLTextAreaElement).value).toBe("/webhooks/*"));
    fireEvent.change(watched, { target: { value: "/webhooks/*\n/api/callback" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(called("inspect_settings_set").at(-1)?.args["patch"]).toEqual({
        watchedPaths: ["/webhooks/*", "/api/callback"],
      }),
    );
  });

  it("clears the history after confirmation", async () => {
    wrap(<InspectorSettingsPane />);
    fireEvent.click(await screen.findByRole("button", { name: "Clear…" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Clear History" }));
    await waitFor(() => expect(called("inspect_clear")[0]?.args["tap"]).toBeNull());
  });
});

describe("Quick Share inspection switch", () => {
  it("warns about the new address before restarting the share", async () => {
    wrap(<ShareInspectSwitch share={share} />);
    fireEvent.click(screen.getByRole("switch", { name: "Inspect requests" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText(/gets a new address/)).toBeTruthy();
    expect(called("quick_share_set_inspected")).toHaveLength(0);
    fireEvent.click(within(dialog).getByRole("button", { name: "Restart Without Inspector" }));
    await waitFor(() =>
      expect(called("quick_share_set_inspected")[0]?.args).toEqual({ id: "qs-1", inspect: false }),
    );
  });
});

describe("Inspect this route", () => {
  it("reviews the plan before pointing the route at the inspector", async () => {
    wrap(<InspectRouteSection accountId="acc" hostname="app.example.com" path={null} local />);
    const inspect = await screen.findByRole("button", { name: "Inspect this route" });
    await waitFor(() => expect(inspect.hasAttribute("disabled")).toBe(false));
    fireEvent.click(inspect);
    const sheet = await screen.findByRole("dialog", { name: "Inspect app.example.com" });
    expect(await within(sheet).findByText(/Point docs.teispace.com at the inspector/)).toBeTruthy();
    expect(called("inspect_route_preview")[0]?.args).toMatchObject({
      accountId: "acc",
      hostname: "app.example.com",
      on: true,
    });
    fireEvent.click(within(sheet).getByRole("button", { name: "Start Inspecting" }));
    await waitFor(() =>
      expect(called("inspect_route_apply")[0]?.args).toMatchObject({
        on: true,
        fingerprint: "fp-inspect",
        confirmed: false,
      }),
    );
    await waitFor(() =>
      expect(navigate).toHaveBeenCalledWith(expect.objectContaining({ to: "/inspector" })),
    );
  });

  it("isn't offered for routes another machine serves", async () => {
    wrap(
      <InspectRouteSection accountId="acc" hostname="x.example.com" path={null} local={false} />,
    );
    const button = await screen.findByRole("button", { name: "Inspect this route" });
    expect(button.hasAttribute("disabled")).toBe(true);
  });
});

describe("Inspection settings sheet", () => {
  it("saves only what changed", async () => {
    const tap = mockTaps()[0];
    if (!tap) throw new Error("fixture");
    wrap(<TapSettingsSheet tap={tap} open onClose={() => {}} />);
    const sheet = await screen.findByRole("dialog");
    const save = within(sheet).getByRole("button", { name: "Save" });
    expect(save.hasAttribute("disabled")).toBe(true);
    fireEvent.click(within(sheet).getByRole("radio", { name: "Network" }));
    fireEvent.click(within(sheet).getByRole("radio", { name: "3G" }));
    fireEvent.click(within(sheet).getByRole("button", { name: /Add Fault/ }));
    fireEvent.click(save);
    await waitFor(() => expect(called("inspect_configure")).toHaveLength(1));
    expect(called("inspect_configure")[0]?.args).toEqual({
      tap: "qs-1",
      patch: {
        networkPreset: "threeG",
        faults: [
          {
            method: null,
            path: "/*",
            percent: 10,
            action: { type: "status", status: 503, retry_after_secs: null },
          },
        ],
      },
    });
  });

  it("adds a breakpoint", async () => {
    const tap = mockTaps()[0];
    if (!tap) throw new Error("fixture");
    wrap(<TapSettingsSheet tap={tap} open onClose={() => {}} />);
    const sheet = await screen.findByRole("dialog");
    fireEvent.click(within(sheet).getByRole("radio", { name: "Breakpoints" }));
    expect(within(sheet).getByText("No breakpoints.")).toBeTruthy();
    fireEvent.click(within(sheet).getByRole("button", { name: /Add Breakpoint/ }));
    fireEvent.change(within(sheet).getByRole("textbox", { name: "Path" }), {
      target: { value: "/webhooks/*" },
    });
    fireEvent.click(within(sheet).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(called("inspect_configure")).toHaveLength(1));
    expect(called("inspect_configure")[0]?.args["patch"]).toEqual({
      breakpoints: [{ method: null, path: "/webhooks/*", request: true, response: false }],
    });
  });

  it("protects on this computer and shows a new token once", async () => {
    const tap = mockTaps()[1];
    if (!tap) throw new Error("fixture");
    mockIPC((cmd, args) => {
      calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
      if (cmd === "inspect_protect") {
        return { protection: tap.protection, secretLinkKey: null, bearerToken: "tt_abc123" };
      }
      return null;
    });
    wrap(<TapSettingsSheet tap={tap} open onClose={() => {}} />);
    const sheet = await screen.findByRole("dialog");
    fireEvent.click(within(sheet).getByRole("radio", { name: "Protection" }));
    expect(within(sheet).getByText(/Enforced by the inspector on this computer/)).toBeTruthy();
    fireEvent.click(within(sheet).getByRole("button", { name: "Create Token" }));
    expect(await within(sheet).findByText("tt_abc123")).toBeTruthy();
    expect(within(sheet).getByText(/isn't shown again/)).toBeTruthy();
    expect(called("inspect_protect")[0]?.args).toEqual({ tap: "rt-docs", input: { bearer: true } });
  });
});

describe("Protecting a Quick Share", () => {
  it("is offered once the share is inspected, and says what protects it", async () => {
    const [first] = mockTaps();
    if (!first) throw new Error("fixture");
    mockIPC((cmd, args) => {
      if (cmd === "inspect_taps") {
        return [
          {
            ...first,
            protection: { ...first.protection, password: true, bearerTokens: 2 },
          },
        ];
      }
      return inspectorMock(cmd, (args ?? {}) as Record<string, unknown>) ?? null;
    });
    const { unmount } = wrap(<ProtectShareButton share={{ ...share, inspected: false }} />);
    const off = screen.getByRole("button", { name: /turn on Inspect requests first/ });
    expect(off.hasAttribute("disabled")).toBe(true);
    unmount();

    wrap(
      <>
        <ProtectShareButton share={share} />
        <ShareProtectionNote share={share} />
      </>,
    );
    expect(
      await screen.findByText("Protected on this computer: Password page, 2 bearer tokens"),
    ).toBeTruthy();
    const button = screen.getByRole("button", { name: "Protect This Share" });
    expect(button.getAttribute("aria-pressed")).toBe("true");
    fireEvent.click(button);
    const sheet = await screen.findByRole("dialog", { name: "Protect a.trycloudflare.com" });
    expect(within(sheet).getByText(/Enforced by the inspector on this computer/)).toBeTruthy();
  });
});

describe("Traffic Numbers", () => {
  it("shows a share's numbers from the inspector, bots by the name they give", async () => {
    wrap(<TapStatsSheet tap="qs-1" name="https://a.trycloudflare.com" onClose={() => {}} />);
    expect(await screen.findByText("Bots, as they name themselves")).toBeTruthy();
    expect(screen.getByText("Webhooks")).toBeTruthy();
    expect(screen.getByText("From the local inspector")).toBeTruthy();
    expect(screen.getByText("Per second")).toBeTruthy();
    // Cache isn't measured locally, and that isn't about the plan.
    expect(screen.queryByText(/Not on this domain/)).toBeNull();
    expect(called("inspect_stats")[0]?.args).toEqual({ tap: "qs-1", range: "hour" });
    fireEvent.click(screen.getByRole("radio", { name: "Day" }));
    await waitFor(() => expect(called("inspect_stats").at(-1)?.args["range"]).toBe("day"));
  });

  it("offers only the ranges the history keeps", () => {
    expect(rangesKept(1)).toEqual(["hour"]);
    expect(rangesKept(24)).toEqual(["hour", "day"]);
    expect(rangesKept(24 * 30)).toEqual(["hour", "day", "week", "month"]);
  });
});
