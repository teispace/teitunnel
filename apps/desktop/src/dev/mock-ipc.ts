import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import type {
  Account,
  ActivityEntry,
  AppInfo,
  Capabilities,
  Domain,
  LocalService,
  Outcome,
  PlanView,
  QuickShare,
  RoutesOverview,
  Settings,
  SettingsPatch,
  ShareStats,
} from "@/lib/ipc/bindings";

/**
 * Dev-only IPC fixtures, used when the UI runs in a plain browser (WebKit screenshots,
 * design review). Never bundled in release builds: `main.tsx` imports this module only
 * when `import.meta.env.DEV` is true and the Tauri runtime is absent.
 */
const now = Date.now();

let settings: Settings = { theme: "system", showInMenuBar: true };

let shares: QuickShare[] = [
  {
    id: "qs-1",
    origin: "http://localhost:5173",
    url: "https://quiet-river-lamp-orbit.trycloudflare.com",
    status: { status: "live" },
    startedAt: now - 12 * 60_000,
    stopAt: now + 48 * 60_000,
  },
  {
    id: "qs-2",
    origin: "http://localhost:3000",
    url: null,
    status: { status: "starting" },
    startedAt: now - 3_000,
    stopAt: null,
  },
];

const services: LocalService[] = [
  {
    port: 5173,
    allInterfaces: false,
    pid: 4101,
    process: "node",
    kind: "vite",
    project: "teitunnel-web",
    origin: "http://localhost:5173",
  },
  {
    port: 3000,
    allInterfaces: true,
    pid: 4102,
    process: "node",
    kind: "next",
    project: "marketing",
    origin: "http://localhost:3000",
  },
  {
    port: 8000,
    allInterfaces: false,
    pid: 4103,
    process: "Python",
    kind: "python",
    project: "api",
    origin: "http://localhost:8000",
  },
  {
    port: 5000,
    allInterfaces: true,
    pid: 512,
    process: "ControlCenter",
    kind: "system",
    project: null,
    origin: "http://localhost:5000",
  },
];

const accounts: Account[] = [
  { id: "acc-personal", name: "Krishna's account", credential: "apiToken", limitedZone: null },
];

const domains: Domain[] = [
  {
    id: "023e105f4ecef8ad9ca31a8372d0c353",
    name: "teispace.com",
    status: "active",
    nameServers: ["ada.ns.cloudflare.com", "bob.ns.cloudflare.com"],
    originalNameServers: [],
    plan: "Free Website",
    paused: false,
  },
  {
    id: "9a7806061c88ada191ed06f989cc3dac",
    name: "xyz.dev",
    status: "active",
    nameServers: ["ada.ns.cloudflare.com", "bob.ns.cloudflare.com"],
    originalNameServers: [],
    plan: "Pro Website",
    paused: false,
  },
  {
    id: "5c1d1e2f3a4b5c6d7e8f9a0b1c2d3e4f",
    name: "yx.app",
    status: "pending",
    nameServers: ["kate.ns.cloudflare.com", "rick.ns.cloudflare.com"],
    originalNameServers: ["ns1.registrar.example", "ns2.registrar.example"],
    plan: "Free Website",
    paused: false,
  },
];

const capabilities: Capabilities = {
  zonesRead: "yes",
  tunnelsRead: "yes",
  tunnelsEdit: "yes",
  accessEdit: "no",
  zones: domains.map((d) => ({
    zoneId: d.id,
    zoneName: d.name,
    dnsEdit: d.name === "yx.app" ? "no" : "yes",
  })),
};

const tunnelId = "6ff42ae2-765d-4adf-8112-31c55c1551ef";
const routesOverview: RoutesOverview = {
  tunnel: {
    id: tunnelId,
    name: "Krishnas-MacBook-Pro",
    connector: { state: "healthy", connections: 4 },
  },
  routes: [
    {
      hostname: "app.teispace.com",
      path: null,
      origin: "http://localhost:5173",
      local: true,
      zone: "teispace.com",
      dns: { state: "ok" },
    },
    {
      hostname: "docs.teispace.com",
      path: null,
      origin: "http://localhost:4321",
      local: true,
      zone: "teispace.com",
      dns: { state: "missing" },
    },
    {
      hostname: "xyz.dev",
      path: null,
      origin: "http://localhost:3000",
      local: true,
      zone: "xyz.dev",
      dns: { state: "ok" },
    },
    {
      hostname: "api.xyz.dev",
      path: "^/v1/",
      origin: "http://localhost:8000",
      local: true,
      zone: "xyz.dev",
      dns: { state: "ok" },
    },
  ],
  zones: [
    { id: "023e105f4ecef8ad9ca31a8372d0c353", name: "teispace.com" },
    { id: "9a7806061c88ada191ed06f989cc3dac", name: "xyz.dev" },
    { id: "5c1d1e2f3a4b5c6d7e8f9a0b1c2d3e4f", name: "yx.app" },
  ],
};

const addPlan: PlanView = {
  steps: [
    {
      kind: "putConfig",
      description: "Update tunnel “Krishnas-MacBook-Pro” to serve 5 routes",
      command: `curl -X PUT -H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" …/cfd_tunnel/${tunnelId}/configurations`,
    },
    {
      kind: "updateRecord",
      description: "Point shop.yx.app at tunnel “Krishnas-MacBook-Pro” (was A 192.0.2.10)",
      command: null,
    },
    {
      kind: "verify",
      description: "Check https://shop.yx.app works",
      command: "curl -I https://shop.yx.app",
    },
  ],
  warnings: [
    {
      type: "replacesForeignRecord",
      hostname: "shop.yx.app",
      kind: "A",
      content: "192.0.2.10",
    },
  ],
  requiresConfirmation: true,
  fingerprint: "mock",
};

const activity: ActivityEntry[] = [
  {
    id: 2,
    at: now - 4 * 60_000,
    summary: "Add api.xyz.dev (path ^/v1/) → http://localhost:8000",
    outcome: "applied",
    detail: [],
  },
  {
    id: 1,
    at: now - 3 * 3_600_000,
    summary: "Add app.teispace.com → http://localhost:5173",
    outcome: "applied",
    detail: [],
  },
];

const stats: ShareStats = { requests: 1284, errors: 3 };
const appInfo: AppInfo = {
  version: "0.0.0-dev",
  platform: "macos",
  arch: "aarch64",
  dataDir: "~/Library/Application Support/com.teispace.teitunnel",
};

export function installMockIpc(): void {
  mockWindows("main");
  mockIPC(
    (cmd, args) => {
      const payload = (args ?? {}) as Record<string, unknown>;
      switch (cmd) {
        case "app_info":
          return appInfo;
        case "settings_get":
          return settings;
        case "settings_set": {
          const patch = payload["patch"] as SettingsPatch;
          settings = {
            theme: patch.theme ?? settings.theme,
            showInMenuBar: patch.showInMenuBar ?? settings.showInMenuBar,
          };
          return settings;
        }
        case "binary_status":
          return {
            path: "/opt/homebrew/bin/cloudflared",
            source: "system",
            version: "2026.9.1",
            supported: true,
          };
        case "services_list":
          return services;
        case "quick_share_list":
          return shares;
        case "quick_share_start": {
          const origin = String(payload["origin"]);
          const share: QuickShare = {
            id: `qs-${shares.length + 1}`,
            origin: /^\d+$/.test(origin) ? `http://localhost:${origin}` : origin,
            url: null,
            status: { status: "starting" },
            startedAt: Date.now(),
            stopAt: null,
          };
          shares = [share, ...shares];
          return share;
        }
        case "quick_share_stop":
          shares = shares.filter((s) => s.id !== payload["id"]);
          return null;
        case "accounts_list":
          return accounts;
        case "accounts_capabilities":
          return capabilities;
        case "accounts_detect_cert":
          return true;
        case "domains_list":
          return domains;
        case "routes_overview":
          return routesOverview;
        case "routes_preview":
          return addPlan;
        case "routes_apply":
          return new Promise<Outcome>((resolve) =>
            setTimeout(
              () =>
                resolve({
                  type: "applied",
                  tunnelId,
                  verify: ["shop.yx.app"],
                  connectorError: null,
                }),
              600,
            ),
          );
        case "routes_verify":
          return new Promise((resolve) =>
            setTimeout(
              () =>
                resolve({
                  hostname: String(payload["hostname"]),
                  status: 200,
                  failure: null,
                  message: null,
                }),
              900,
            ),
          );
        case "routes_drift":
          return new URLSearchParams(window.location.search).has("drift")
            ? {
                tunnelId,
                appliedVersion: 7,
                currentVersion: 8,
                changes: [
                  {
                    hostname: "admin.teispace.com",
                    path: null,
                    before: null,
                    after: "http://localhost:9000",
                  },
                ],
              }
            : null;
        case "tunnels_list":
          return [
            {
              id: tunnelId,
              name: "Krishnas-MacBook-Pro",
              status: "healthy",
              createdAt: new Date(now - 9 * 86_400_000).toISOString(),
              routes: 4,
              connections: [
                { colo: "ams01", version: "2026.9.1", originIp: "203.0.113.7", openedAt: "" },
                { colo: "fra08", version: "2026.9.1", originIp: "203.0.113.7", openedAt: "" },
              ],
              thisMac: true,
              connector: { state: "healthy", connections: 4 },
            },
            {
              id: "b1946ac9-2a6f-4e8e-9d51-1f0e7a3c2b11",
              name: "home-lab",
              status: "inactive",
              createdAt: new Date(now - 90 * 86_400_000).toISOString(),
              routes: null,
              connections: [],
              thisMac: false,
              connector: null,
            },
          ];
        case "routes_activity":
          return activity;
        case "quick_share_stats":
          return stats;
        case "quick_share_logs":
          return [
            {
              time: null,
              level: "info",
              message: "Requesting new quick Tunnel on trycloudflare.com...",
              error: null,
            },
            { time: null, level: "info", message: "Registered tunnel connection", error: null },
          ];
        case "quick_share_qr":
          return '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="4" height="4" fill="currentColor"/><rect x="6" width="4" height="4" fill="currentColor"/><rect y="6" width="4" height="4" fill="currentColor"/></svg>';
        default:
          return null;
      }
    },
    { shouldMockEvents: true },
  );
}
