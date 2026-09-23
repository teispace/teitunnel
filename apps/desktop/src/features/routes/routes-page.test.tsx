import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createQueryClient } from "@/app/query-client";
import { TooltipProvider } from "@/components/ui/tooltip";
import { rawText } from "@/lib/i18n";
import type {
  Change,
  EndpointHealth,
  PlanView,
  RoutesOverview,
  RouteView,
  TunnelView,
} from "@/lib/ipc/bindings";
import { RoutesPage } from "./routes-page";

const zones = [
  { id: "z1", name: "xyz.com" },
  { id: "z2", name: "yx.com" },
];

let routes: RouteView[];
let calls: { cmd: string; args: Record<string, unknown> }[];
let drift: boolean;
/** A second tunnel of this Mac's, when a test wants one. */
let extra: TunnelView | null;
let accessEdit: "yes" | "no";
let accessDenied: boolean;
let refusal: { key: string; args: Record<string, string>; field?: string } | null;
let originDown: boolean;

const plan = (change: Change): PlanView => {
  if (refusal) {
    const { field = null, ...message } = refusal;
    throw { code: "permissionDenied", message, hint: null, field };
  }
  if (accessDenied) {
    throw {
      code: "permissionDenied",
      message: { key: "core.error.observe.accessPermission", args: {} },
      hint: null,
      field: null,
    };
  }
  if (change.type === "addRoute" && change.route.hostname.startsWith("bad.")) {
    throw {
      code: "invalidInput",
      message: { key: "core.error.hostname.noDomain", args: {} },
      hint: null,
      field: "hostname",
    };
  }
  const foreign = change.type === "addRoute" && change.route.hostname === "old.xyz.com";
  return {
    steps: [
      {
        kind: "putConfig",
        description: rawText("Update tunnel “Mac” to serve 2 routes"),
        command: null,
      },
      ...(change.type === "removeRoute"
        ? [
            {
              kind: "deleteRecord" as const,
              description: rawText("Delete DNS record"),
              command: null,
            },
          ]
        : [{ kind: "verify" as const, description: rawText("Check it works"), command: null }]),
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

const mac = {
  id: "t1",
  name: "Mac",
  connector: { state: "healthy", connections: 4 },
  isDefault: true,
} as const;

beforeEach(() => {
  routes = [
    {
      hostname: "app.xyz.com",
      path: null,
      origin: "http://localhost:3000",
      local: true,
      tunnelId: "t1",
      temporary: false,
      balanced: false,
      zone: "xyz.com",
      dns: { state: "ok" },
      access: null,
      client: null,
    },
  ];
  calls = [];
  drift = false;
  extra = null;
  accessEdit = "yes";
  accessDenied = false;
  refusal = null;
  originDown = false;
  mockWindows("main");
  mockIPC((cmd, args) => {
    const payload = (args ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args: payload });
    switch (cmd) {
      case "accounts_list":
        return [{ id: "acc", name: "Me", credential: "apiToken", limitedZone: null }];
      case "routes_overview":
        return {
          tunnel: mac,
          tunnels: extra ? [mac, extra] : [mac],
          routes,
          zones,
          networks: [],
        } satisfies RoutesOverview;
      case "doctor_run":
        return originDown
          ? [
              {
                id: "origin.not_listening:acc:app.xyz.com",
                check: "origin.not_listening",
                severity: "error",
                accountId: "acc",
                subject: "app.xyz.com",
                label: rawText("app.xyz.com"),
                title: { key: "core.doctor.originNotListening.title", args: { port: "3000" } },
                detail: { key: "core.doctor.originNotListening.detail", args: {} },
                evidence: [],
                fixes: [],
                tunnelId: null,
              },
            ]
          : [];
      case "routes_preview":
        return plan(payload["change"] as Change);
      case "routes_balance_health":
        return [
          {
            tunnelId: "t1",
            name: "Mac",
            enabled: true,
            healthyRegions: 3,
            regions: 3,
            reason: null,
          },
          {
            tunnelId: "t9",
            name: "server",
            enabled: true,
            healthyRegions: 0,
            regions: 3,
            reason: "HTTP timeout occurred",
          },
        ] satisfies EndpointHealth[];
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
              tunnelId: "t1",
              temporary: false,
              balanced: false,
              zone: "yx.com",
              dns: { state: "ok" },
              access: change.route.access ?? null,
              client: null,
            },
          ];
          return {
            type: "applied",
            tunnelId: "t1",
            temporary: false,
            balanced: false,
            verify: [change.route.hostname],
            connectorError: null,
          };
        }
        if (change.type === "removeRoute")
          routes = routes.filter((r) => r.hostname !== change.hostname);
        return { type: "applied", tunnelId: "t1", verify: [], connectorError: null };
      }
      case "routes_verify": {
        const guarded = routes.some((r) => r.hostname === payload["hostname"] && r.access);
        return {
          hostname: payload["hostname"],
          status: guarded ? 302 : 200,
          failure: null,
          message: null,
          protected: guarded,
        };
      }
      case "routes_drift":
        return drift
          ? {
              tunnelId: "t1",
              temporary: false,
              balanced: false,
              appliedVersion: 1,
              currentVersion: 2,
              changes: [
                { hostname: "dash.xyz.com", path: null, before: null, after: "http://localhost:9" },
              ],
            }
          : null;
      case "accounts_capabilities":
        return { zonesRead: "yes", tunnelsRead: "yes", tunnelsEdit: "yes", accessEdit, zones: [] };
      case "routes_keep_theirs":
        drift = false;
        return null;
      case "routes_export":
        return {
          fileName: payload["format"] === "terraform" ? "teitunnel.tf" : "config.yml",
          contents: `# ${String(payload["format"])}\ntunnel: t1\n`,
        };
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
  it("shows how to connect to an SSH route instead of a URL", async () => {
    routes = [
      {
        hostname: "ssh.xyz.com",
        path: null,
        origin: "ssh://localhost:22",
        local: true,
        tunnelId: "t1",
        temporary: false,
        balanced: false,
        zone: "xyz.com",
        dns: { state: "ok" },
        access: null,
        client: {
          protocol: "ssh",
          command: 'ssh -o ProxyCommand="cloudflared access ssh --hostname %h" ssh.xyz.com',
          localAddress: null,
          sshConfig: "Host ssh.xyz.com\n  ProxyCommand cloudflared access ssh --hostname %h",
        },
      },
    ];
    renderPage();
    expect(await screen.findByRole("heading", { name: "Connect" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Copy Command" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Copy SSH config" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: /^Open/ })).toBeNull();
    expect(screen.queryByRole("button", { name: "Test" })).toBeNull();
  });

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
      route: { hostname: "api.xyz.com", origin: "5000", path: null, access: null },
    });

    fireEvent.click(within(dialog).getByRole("button", { name: "Add Route" }));
    expect(await within(dialog).findByText("It works")).toBeTruthy();
    const apply = calls.find((c) => c.cmd === "routes_apply");
    expect(apply?.args["fingerprint"]).toBe("fp");
    expect(calls.some((c) => c.cmd === "routes_verify" && c.args["wait"] === true)).toBe(true);
    // The list behind the sheet refreshed with the new route.
    await screen.findByRole("option", { name: /api\.xyz\.com/, hidden: true });
  });

  it("puts a new route on the tunnel chosen, and says which tunnel carries each", async () => {
    extra = { id: "t2", name: "staging", connector: null, isDefault: false };
    routes = [...routes, { ...(routes[0] as RouteView), hostname: "beta.xyz.com", tunnelId: "t2" }];
    renderPage();
    const beta = await screen.findByRole("option", { name: /beta\.xyz\.com/ });
    expect(within(beta).getByText(/· staging/)).toBeTruthy();
    // Its tunnel isn't running, so the route isn't live even though the other one is.
    expect(within(beta).getByRole("img", { name: "Connector stopped" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "New route" }));
    const dialog = await screen.findByRole("dialog", { name: "New Route" });
    fireEvent.change(within(dialog).getByRole("combobox", { name: "Port or address" }), {
      target: { value: "5000" },
    });
    // The picker starts on the default tunnel.
    const picker = within(dialog).getByRole("combobox", { name: "Tunnel" });
    expect(picker.textContent).toContain("Mac");
    fireEvent.click(picker);
    fireEvent.click(await screen.findByRole("option", { name: "staging" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Review" }));
    await within(dialog).findByText("Update tunnel “Mac” to serve 2 routes");
    expect(calls.find((c) => c.cmd === "routes_preview")?.args["tunnelId"]).toBe("t2");
  });

  it("load balances a route through a reviewed plan, and marks it", async () => {
    renderPage();
    await screen.findByRole("option", { name: /app\.xyz\.com/ });
    fireEvent.click(screen.getByRole("button", { name: "Load Balance…" }));
    const dialog = await screen.findByRole("dialog", { name: "Load Balance Route" });
    await within(dialog).findByText("Update tunnel “Mac” to serve 2 routes");
    const preview = calls.find((c) => c.cmd === "routes_preview");
    expect(preview?.args).toMatchObject({
      tunnelId: "t1",
      change: { type: "balanceRoute", hostname: "app.xyz.com" },
    });
  });

  it("offers to stop load balancing a balanced route", async () => {
    routes = [{ ...(routes[0] as RouteView), balanced: true }];
    renderPage();
    const row = await screen.findByRole("option", { name: /app\.xyz\.com/ });
    expect(within(row).getByText("Balanced")).toBeTruthy();
    // Each machine's health from Cloudflare's checks, this one marked.
    expect(await screen.findByText("Mac (this Mac)")).toBeTruthy();
    expect(screen.getByText("Healthy in all 3 regions")).toBeTruthy();
    expect(screen.getByText("Failing its health checks: HTTP timeout occurred")).toBeTruthy();
    expect(calls.find((c) => c.cmd === "routes_balance_health")?.args).toMatchObject({
      accountId: "acc",
      hostname: "app.xyz.com",
    });
    fireEvent.click(screen.getByRole("button", { name: "Stop Load Balancing…" }));
    const dialog = await screen.findByRole("dialog", { name: "Stop Load Balancing" });
    await within(dialog).findByText("Update tunnel “Mac” to serve 2 routes");
    expect(calls.find((c) => c.cmd === "routes_preview")?.args["change"]).toEqual({
      type: "unbalanceRoute",
      hostname: "app.xyz.com",
    });
  });

  it("puts a login in front of a new route", async () => {
    const dialog = await openAddSheet();
    fireEvent.change(within(dialog).getByRole("combobox", { name: "Port or address" }), {
      target: { value: "5000" },
    });
    fireEvent.change(within(dialog).getByRole("textbox", { name: "Subdomain" }), {
      target: { value: "admin" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Advanced" }));
    fireEvent.click(within(dialog).getByRole("checkbox", { name: "Require a login" }));
    fireEvent.change(within(dialog).getByRole("textbox", { name: "Who can sign in" }), {
      target: { value: "me@xyz.com, @team.io" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Review" }));
    await within(dialog).findByText("Update tunnel “Mac” to serve 2 routes");
    const preview = calls.find((c) => c.cmd === "routes_preview");
    expect(preview?.args["change"]).toMatchObject({
      route: { access: { emails: ["me@xyz.com"], emailDomains: ["team.io"] } },
    });

    fireEvent.click(within(dialog).getByRole("button", { name: "Add Route" }));
    expect(await within(dialog).findByText("Protected by a login")).toBeTruthy();
    const row = await screen.findByRole("option", { name: /admin\.xyz\.com/, hidden: true });
    expect(within(row).getByRole("img", { name: "Requires a login", hidden: true })).toBeTruthy();
  });

  it("shows how to add the Access permission instead of a dead end", async () => {
    accessEdit = "no";
    const dialog = await openAddSheet();
    fireEvent.change(within(dialog).getByRole("combobox", { name: "Port or address" }), {
      target: { value: "5000" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Advanced" }));
    fireEvent.click(within(dialog).getByRole("checkbox", { name: "Require a login" }));
    expect(await within(dialog).findByText("The token needs 2 more permissions")).toBeTruthy();
    // What was typed stays; only Review waits for the permission.
    expect(within(dialog).getByRole("textbox", { name: "Who can sign in" })).toBeTruthy();
    expect(within(dialog).getByRole("button", { name: "Review" }).hasAttribute("disabled")).toBe(
      true,
    );

    // The permission is added in Cloudflare; returning to the app picks it up.
    accessEdit = "yes";
    fireEvent(window, new Event("focus"));
    await waitFor(() =>
      expect(within(dialog).queryByText("The token needs 2 more permissions")).toBeNull(),
    );
  });

  it("turns Cloudflare's Access refusal into the fix, then reviews again", async () => {
    accessDenied = true;
    renderPage();
    await screen.findByRole("option", { name: /app\.xyz\.com/ });
    fireEvent.click(screen.getByRole("button", { name: "Remove route" }));
    const dialog = await screen.findByRole("dialog", { name: "Remove Route" });
    accessEdit = "no";
    expect(await within(dialog).findByText("The token needs 2 more permissions")).toBeTruthy();
    expect(within(dialog).queryByText(/can't manage logins\. In Cloudflare/)).toBeNull();
    expect(within(dialog).queryByText("Reading your Cloudflare account…")).toBeNull();

    accessDenied = false;
    accessEdit = "yes";
    fireEvent(window, new Event("focus"));
    expect(await within(dialog).findByText("Delete DNS record")).toBeTruthy();
  });

  it("sends the user to set up Zero Trust, then continues on return", async () => {
    refusal = { key: "core.error.plan.zeroTrustNotSetUp", args: {} };
    const dialog = await openAddSheet();
    fireEvent.change(within(dialog).getByRole("combobox", { name: "Port or address" }), {
      target: { value: "5000" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Review" }));
    expect(await within(dialog).findByText("Set up Cloudflare Zero Trust")).toBeTruthy();

    refusal = null;
    fireEvent(window, new Event("focus"));
    expect(await within(dialog).findByText("Update tunnel “Mac” to serve 2 routes")).toBeTruthy();
  });

  it("links to adding a domain when the hostname's domain isn't in Cloudflare", async () => {
    refusal = {
      key: "core.error.plan.noZone",
      args: { hostname: "app.new.dev" },
      field: "hostname",
    };
    const dialog = await openAddSheet();
    fireEvent.change(within(dialog).getByRole("combobox", { name: "Port or address" }), {
      target: { value: "5000" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Review" }));
    expect(await within(dialog).findByText(/app\.new\.dev isn't in any/)).toBeTruthy();
    fireEvent.click(within(dialog).getByRole("button", { name: /Add a Domain to Cloudflare/ }));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "plugin:opener|open_url")?.args["url"]).toBe(
        "https://dash.cloudflare.com/?to=/acc/add-site",
      ),
    );
  });

  it("lists the route's permissions when Cloudflare refuses", async () => {
    refusal = { key: "core.error.cloudflare.permission", args: { detail: "10000" } };
    renderPage();
    await screen.findByRole("option", { name: /app\.xyz\.com/ });
    fireEvent.click(screen.getByRole("button", { name: "Remove route" }));
    const dialog = await screen.findByRole("dialog", { name: "Remove Route" });
    expect(await within(dialog).findByText("The token needs 2 more permissions")).toBeTruthy();
    expect(within(dialog).getByText("Account · Cloudflare Tunnel · Edit")).toBeTruthy();
    expect(within(dialog).getByText(/Include xyz\.com under Zone Resources/)).toBeTruthy();
    expect(within(dialog).queryByText(/Cloudflare refused/)).toBeNull();
  });

  it("keeps a route's login when editing it, and can remove it", async () => {
    routes = [
      { ...(routes[0] as RouteView), access: { emails: ["me@xyz.com"], emailDomains: [] } },
    ];
    renderPage();
    expect(await screen.findByText("me@xyz.com")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Edit route" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit Route" });
    const allowed = within(dialog).getByRole("textbox", { name: "Who can sign in" });
    expect((allowed as HTMLTextAreaElement).value).toBe("me@xyz.com");
    fireEvent.click(within(dialog).getByRole("checkbox", { name: "Require a login" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Review" }));
    await within(dialog).findByText("Update tunnel “Mac” to serve 2 routes");
    const preview = calls.find((c) => c.cmd === "routes_preview");
    expect(preview?.args["change"]).toMatchObject({ type: "updateRoute", route: { access: null } });
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
    expect(
      await within(dialog).findByText(/already has a DNS record \(A 192\.0\.2\.1\)/),
    ).toBeTruthy();
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

  it("exports the routes as config.yml or Terraform and copies them", async () => {
    const writeText = vi.fn(() => Promise.resolve());
    Object.assign(navigator, { clipboard: { writeText } });
    renderPage();
    fireEvent.click(await screen.findByRole("button", { name: "Export" }));
    const sheet = await screen.findByRole("dialog", { name: "Export" });
    const preview = await within(sheet).findByRole("textbox", { name: "config.yml contents" });
    expect((preview as HTMLTextAreaElement).value).toContain("# configYaml");
    fireEvent.click(within(sheet).getByRole("radio", { name: "Terraform" }));
    await waitFor(() =>
      expect(
        (
          within(sheet).getByRole("textbox", {
            name: "teitunnel.tf contents",
          }) as HTMLTextAreaElement
        ).value,
      ).toContain("# terraform"),
    );
    fireEvent.click(within(sheet).getByRole("button", { name: "Copy" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("# terraform\ntunnel: t1\n"));
  });

  it("explains a Doctor problem with the route in its inspector", async () => {
    originDown = true;
    renderPage();
    expect(
      await screen.findByText(
        "Start the app this route sends traffic to; visitors see an error until it runs.",
      ),
    ).toBeTruthy();
    expect(screen.getAllByText("Nothing is listening on port 3000").length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: "Details…" })).toBeTruthy();
  });
});
