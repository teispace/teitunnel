import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import { TooltipProvider } from "@/components/ui/tooltip";
import type { Change, PlanView, RoutesOverview, RouteView } from "@/lib/ipc/bindings";
import { RoutesPage } from "./routes-page";

const zones = [
  { id: "z1", name: "xyz.com" },
  { id: "z2", name: "yx.com" },
];

let routes: RouteView[];
let calls: { cmd: string; args: Record<string, unknown> }[];
let drift: boolean;

const plan = (change: Change): PlanView => {
  if (change.type === "addRoute" && change.route.hostname.startsWith("bad.")) {
    throw {
      code: "invalidInput",
      message: "Include the domain, like app.example.com.",
      hint: null,
      field: "hostname",
    };
  }
  const foreign = change.type === "addRoute" && change.route.hostname === "old.xyz.com";
  return {
    steps: [
      { kind: "putConfig", description: "Update tunnel “Mac” to serve 2 routes", command: null },
      ...(change.type === "removeRoute"
        ? [{ kind: "deleteRecord" as const, description: "Delete DNS record", command: null }]
        : [{ kind: "verify" as const, description: "Check it works", command: null }]),
    ],
    warnings: foreign
      ? [
          {
            type: "replacesForeignRecord",
            hostname: "old.xyz.com",
            kind: "A",
            content: "192.0.2.1",
          },
        ]
      : [],
    requiresConfirmation: foreign,
    fingerprint: "fp",
  };
};

beforeEach(() => {
  routes = [
    {
      hostname: "app.xyz.com",
      path: null,
      origin: "http://localhost:3000",
      local: true,
      zone: "xyz.com",
      dns: { state: "ok" },
    },
  ];
  calls = [];
  drift = false;
  mockWindows("main");
  mockIPC((cmd, args) => {
    const payload = (args ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args: payload });
    switch (cmd) {
      case "accounts_list":
        return [{ id: "acc", name: "Me", credential: "apiToken", limitedZone: null }];
      case "routes_overview":
        return {
          tunnel: { id: "t1", name: "Mac", connector: { state: "healthy", connections: 4 } },
          routes,
          zones,
        } satisfies RoutesOverview;
      case "routes_preview":
        return plan(payload["change"] as Change);
      case "routes_apply": {
        const change = payload["change"] as Change;
        if (change.type === "addRoute") {
          routes = [
            ...routes,
            {
              hostname: change.route.hostname,
              path: null,
              origin: `http://localhost:${change.route.origin}`,
              local: true,
              zone: "yx.com",
              dns: { state: "ok" },
            },
          ];
          return {
            type: "applied",
            tunnelId: "t1",
            verify: [change.route.hostname],
            connectorError: null,
          };
        }
        if (change.type === "removeRoute")
          routes = routes.filter((r) => r.hostname !== change.hostname);
        return { type: "applied", tunnelId: "t1", verify: [], connectorError: null };
      }
      case "routes_verify":
        return { hostname: payload["hostname"], status: 200, failure: null, message: null };
      case "routes_drift":
        return drift
          ? {
              tunnelId: "t1",
              appliedVersion: 1,
              currentVersion: 2,
              changes: [
                { hostname: "dash.xyz.com", path: null, before: null, after: "http://localhost:9" },
              ],
            }
          : null;
      case "routes_keep_theirs":
        drift = false;
        return null;
      case "routes_logs":
        return payload["hostname"] === "app.xyz.com"
          ? [
              {
                time: null,
                level: "error",
                message: "Request failed",
                error: "dial tcp 127.0.0.1:3000: connect: connection refused",
              },
            ]
          : [];
      case "routes_activity":
      case "services_list":
        return [];
      default:
        return null;
    }
  });
});

function renderPage() {
  return render(
    <QueryClientProvider client={createQueryClient()}>
      <TooltipProvider>
        <RoutesPage />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

async function openAddSheet() {
  renderPage();
  await screen.findByRole("option", { name: /app\.xyz\.com/ });
  fireEvent.click(screen.getByRole("button", { name: "New route" }));
  return screen.findByRole("dialog", { name: "New Route" });
}

describe("RoutesPage", () => {
  it("lists routes grouped by domain with their status", async () => {
    renderPage();
    const row = await screen.findByRole("option", { name: /app\.xyz\.com/ });
    expect(within(row).getByRole("img", { name: "Live" })).toBeTruthy();
    expect(screen.getByText("xyz.com", { selector: "[role=presentation]" })).toBeTruthy();
    expect(screen.getByRole("heading", { name: "app.xyz.com" })).toBeTruthy();
    // The route's own log lines, as the backend filtered them.
    expect(await screen.findByText(/connection refused/)).toBeTruthy();
    expect(calls.some((c) => c.cmd === "routes_logs" && c.args["hostname"] === "app.xyz.com")).toBe(
      true,
    );
  });

  it("adds a route: form → review → apply → verified", async () => {
    const dialog = await openAddSheet();
    fireEvent.change(within(dialog).getByRole("combobox", { name: "Port or address" }), {
      target: { value: "5000" },
    });
    fireEvent.change(within(dialog).getByRole("textbox", { name: "Subdomain" }), {
      target: { value: "api" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Review" }));

    expect(await within(dialog).findByText("Update tunnel “Mac” to serve 2 routes")).toBeTruthy();
    const preview = calls.find((c) => c.cmd === "routes_preview");
    expect(preview?.args["change"]).toEqual({
      type: "addRoute",
      route: { hostname: "api.xyz.com", origin: "5000", path: null },
    });

    fireEvent.click(within(dialog).getByRole("button", { name: "Add Route" }));
    expect(await within(dialog).findByText("It works")).toBeTruthy();
    const apply = calls.find((c) => c.cmd === "routes_apply");
    expect(apply?.args["fingerprint"]).toBe("fp");
    expect(calls.some((c) => c.cmd === "routes_verify" && c.args["wait"] === true)).toBe(true);
    // The list behind the sheet refreshed with the new route.
    await screen.findByRole("option", { name: /api\.xyz\.com/, hidden: true });
  });

  it("requires confirmation before replacing someone else's record", async () => {
    const dialog = await openAddSheet();
    fireEvent.change(within(dialog).getByRole("combobox", { name: "Port or address" }), {
      target: { value: "5000" },
    });
    fireEvent.change(within(dialog).getByRole("textbox", { name: "Subdomain" }), {
      target: { value: "old" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Review" }));
    expect(await within(dialog).findByText(/already has an A record/)).toBeTruthy();
    const apply = within(dialog).getByRole("button", { name: "Add Route" });
    expect(apply.hasAttribute("disabled")).toBe(true);
    fireEvent.click(within(dialog).getByRole("checkbox", { name: "Replace the existing records" }));
    expect(apply.hasAttribute("disabled")).toBe(false);
  });

  it("shows input errors next to the field", async () => {
    const dialog = await openAddSheet();
    fireEvent.change(within(dialog).getByRole("combobox", { name: "Port or address" }), {
      target: { value: "5000" },
    });
    // A hostname the backend rejects (the mock keys on it).
    const subdomain = within(dialog).getByRole("textbox", { name: "Subdomain" });
    fireEvent.change(subdomain, { target: { value: "bad" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Review" }));
    expect(
      await within(dialog).findByText("Include the domain, like app.example.com."),
    ).toBeTruthy();
    expect(subdomain.getAttribute("aria-invalid")).toBe("true");
  });

  it("removes a route after reviewing the plan", async () => {
    renderPage();
    await screen.findByRole("option", { name: /app\.xyz\.com/ });
    fireEvent.click(screen.getByRole("button", { name: "Remove route" }));
    const dialog = await screen.findByRole("dialog", { name: "Remove Route" });
    expect(await within(dialog).findByText("Delete DNS record")).toBeTruthy();
    fireEvent.click(within(dialog).getByRole("button", { name: "Remove" }));
    await waitFor(() => expect(screen.queryByRole("option", { name: /app\.xyz\.com/ })).toBeNull());
  });

  it("offers to keep or restore outside edits", async () => {
    drift = true;
    renderPage();
    expect(await screen.findByText("Routes were changed outside Teitunnel")).toBeTruthy();
    expect(screen.getByText(/dash\.xyz\.com was added/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Keep Changes" }));
    await waitFor(() =>
      expect(screen.queryByText("Routes were changed outside Teitunnel")).toBeNull(),
    );
  });
});
