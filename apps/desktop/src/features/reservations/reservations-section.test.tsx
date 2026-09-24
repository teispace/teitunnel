import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import type { Domain, PlanView, Reservations } from "@/lib/ipc/bindings";
import { ReservationsSection } from "./reservations-section";

const domain: Domain = {
  id: "z1",
  name: "xyz.com",
  status: "active",
  paused: false,
  nameServers: [],
  originalNameServers: [],
  plan: "Free Website",
};

const reservations: Reservations = {
  cached: false,
  items: [
    {
      hostname: "alice.xyz.com",
      owner: "alice@Alice-MacBook",
      until: Date.parse("2099-12-31T00:00:00Z"),
      routed: false,
      mine: false,
      ended: false,
    },
    {
      hostname: "demo.xyz.com",
      owner: "me@Mac",
      until: null,
      routed: true,
      mine: true,
      ended: false,
    },
    {
      hostname: "other.yx.com",
      owner: "me@Mac",
      until: null,
      routed: false,
      mine: true,
      ended: false,
    },
  ],
};

const core = (key: string, args: Record<string, string | number> = {}) => ({
  key: `core.${key}`,
  args,
});

const takeOver: PlanView = {
  steps: [
    {
      kind: "deleteRecord",
      description: core("plan.step.deleteRecord", {
        hostname: "alice.xyz.com",
        kind: "AAAA",
        content: "100::",
      }),
      command: null,
    },
    {
      kind: "reservation",
      description: core("reservations.step.create", { hostname: "alice.xyz.com" }),
      command: null,
    },
  ],
  warnings: [
    {
      type: "heldBy",
      hostname: "alice.xyz.com",
      owner: "alice@Alice-MacBook",
      until: null,
      kind: "reservation",
    },
  ],
  requiresConfirmation: true,
  fingerprint: "fp-take",
};

const release: PlanView = {
  steps: [
    {
      kind: "reservation",
      description: core("reservations.step.end", { hostname: "demo.xyz.com" }),
      command: null,
    },
  ],
  warnings: [],
  requiresConfirmation: false,
  fingerprint: "fp-release",
};

let calls: { cmd: string; args: Record<string, unknown> }[];

beforeEach(() => {
  calls = [];
  mockWindows("main");
  mockIPC((cmd, args) => {
    const payload = (args ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args: payload });
    switch (cmd) {
      case "reservations_list":
        return reservations;
      case "reservations_availability":
        return String(payload["hostname"]) === "alice.xyz.com"
          ? {
              state: "held",
              hold: {
                hostname: "alice.xyz.com",
                owner: "alice@Alice-MacBook",
                until: null,
                kind: "reservation",
              },
            }
          : { state: "free" };
      case "routes_preview": {
        const change = payload["change"] as { type: string };
        return change.type === "releaseHostname" ? release : takeOver;
      }
      case "routes_apply":
        return { type: "applied", tunnelId: null, verify: [], connectorError: null };
      default:
        return null;
    }
  });
});

function renderSection() {
  render(
    <QueryClientProvider client={createQueryClient()}>
      <ReservationsSection accountId="a1" domain={domain} />
    </QueryClientProvider>,
  );
}

describe("ReservationsSection", () => {
  it("lists the domain's reservations with their holders; only yours can be released", async () => {
    renderSection();
    const list = await screen.findByRole("list", { name: "Reservations" });
    const rows = within(list).getAllByRole("listitem");
    expect(rows).toHaveLength(2);
    expect(within(rows[0] as HTMLElement).getByText(/alice@Alice-MacBook · Until/)).toBeTruthy();
    expect(within(rows[0] as HTMLElement).queryByRole("button")).toBeNull();
    expect(
      within(rows[1] as HTMLElement).getByText("You · No end date · Also routed"),
    ).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Release demo.xyz.com" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "routes_apply")).toBe(true));
    expect(calls.find((c) => c.cmd === "routes_preview")?.args["change"]).toEqual({
      type: "releaseHostname",
      hostname: "demo.xyz.com",
    });
  });

  it("reserves a name, shows who holds it while typing, and asks before taking it over", async () => {
    renderSection();
    fireEvent.click(await screen.findByRole("button", { name: "Reserve" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText("Subdomain"), { target: { value: "alice" } });
    expect(await within(dialog).findByText("Reserved by alice@Alice-MacBook")).toBeTruthy();
    fireEvent.change(within(dialog).getByLabelText("Until (optional)"), {
      target: { value: "2026-12-31" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Review" }));

    expect(
      await within(dialog).findByText(
        "alice.xyz.com is reserved by alice@Alice-MacBook. Going ahead takes it over.",
      ),
    ).toBeTruthy();
    fireEvent.click(within(dialog).getByRole("button", { name: "Take Over" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "routes_apply")).toBe(true));
    const apply = calls.find((c) => c.cmd === "routes_apply");
    expect(apply?.args["change"]).toEqual({
      type: "reserveHostname",
      hostname: "alice.xyz.com",
      until: "2026-12-31",
    });
    expect(apply?.args["confirmed"]).toBe(true);
    expect(apply?.args["fingerprint"]).toBe("fp-take");
  });
});
