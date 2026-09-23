import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { rawText } from "@/lib/i18n";

/** A message the Rust core would send (`core.*` in the catalog). */
const core = (key: string, args: Record<string, string | number> = {}) => ({
  key: `core.${key}`,
  args,
});

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
  TunnelSummary,
} from "@/lib/ipc/bindings";

/**
 * Dev-only IPC fixtures, used when the UI runs in a plain browser (WebKit screenshots,
 * design review). Never bundled in release builds: `main.tsx` imports this module only
 * when `import.meta.env.DEV` is true and the Tauri runtime is absent.
 */
const now = Date.now();

let settings: Settings = {
  theme: "system",
  showInMenuBar: true,
  notifyConnectors: true,
  notifyQuickShares: true,
  notifyDoctor: true,
  ignoredIssues: [],
};

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
  { id: "acc-personal", name: "Personal", credential: "apiToken", limitedZone: null },
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
    name: "MacBook-Pro",
    connector: { state: "healthy", connections: 4 },
    isDefault: true,
  },
  tunnels: [
    {
      id: tunnelId,
      name: "MacBook-Pro",
      connector: { state: "healthy", connections: 4 },
      isDefault: true,
    },
    {
      id: "0b8c3f1e-2d4a-4c6b-9e7f-a1b2c3d4e5f6",
      name: "staging",
      connector: null,
      isDefault: false,
    },
  ],
  routes: [
    {
      hostname: "app.teispace.com",
      path: null,
      origin: "http://localhost:5173",
      local: true,
      tunnelId,
      temporary: false,
      zone: "teispace.com",
      dns: { state: "ok" },
      access: null,
      client: null,
    },
    {
      hostname: "docs.teispace.com",
      path: null,
      origin: "http://localhost:4321",
      local: true,
      tunnelId,
      temporary: false,
      zone: "teispace.com",
      dns: { state: "missing" },
      access: null,
      client: null,
    },
    {
      hostname: "xyz.dev",
      path: null,
      origin: "http://localhost:3000",
      local: true,
      tunnelId,
      temporary: false,
      zone: "xyz.dev",
      dns: { state: "ok" },
      access: { emails: ["me@xyz.dev"], emailDomains: ["teispace.com"] },
      client: null,
    },
    {
      hostname: "ssh.xyz.dev",
      path: null,
      origin: "ssh://localhost:22",
      local: true,
      tunnelId,
      temporary: false,
      zone: "xyz.dev",
      dns: { state: "ok" },
      access: { emails: ["me@xyz.dev"], emailDomains: [] },
      client: {
        protocol: "ssh",
        command: 'ssh -o ProxyCommand="cloudflared access ssh --hostname %h" ssh.xyz.dev',
        localAddress: null,
        sshConfig: "Host ssh.xyz.dev\n  ProxyCommand cloudflared access ssh --hostname %h",
      },
    },
    {
      hostname: "api.xyz.dev",
      path: "^/v1/",
      origin: "http://localhost:8000",
      local: true,
      tunnelId,
      temporary: false,
      zone: "xyz.dev",
      dns: { state: "ok" },
      access: null,
      client: null,
    },
  ],
  zones: [
    { id: "023e105f4ecef8ad9ca31a8372d0c353", name: "teispace.com" },
    { id: "9a7806061c88ada191ed06f989cc3dac", name: "xyz.dev" },
    { id: "5c1d1e2f3a4b5c6d7e8f9a0b1c2d3e4f", name: "yx.app" },
  ],
  networks: [{ network: "192.168.1.0/24", private: true, owned: true }],
};

const protectedPlan: PlanView = {
  steps: [
    {
      kind: "loginMethod",
      description: rawText("Add One-time PIN as a way to sign in (a code sent by email)"),
      command: null,
    },
    {
      kind: "accessApp",
      description: rawText("Require a login for shop.yx.app: me@xyz.dev, anyone at @teispace.com"),
      command: null,
    },
    {
      kind: "putConfig",
      description: rawText("Update tunnel “MacBook-Pro” to serve 5 routes"),
      command: null,
    },
    {
      kind: "createRecord",
      description: rawText("Point shop.yx.app at tunnel “MacBook-Pro”"),
      command: null,
    },
    { kind: "verify", description: rawText("Check https://shop.yx.app works"), command: null },
  ],
  warnings: [],
  requiresConfirmation: false,
  fingerprint: "mock",
};

const addPlan: PlanView = {
  steps: [
    {
      kind: "putConfig",
      description: rawText("Update tunnel “MacBook-Pro” to serve 5 routes"),
      command: `curl -X PUT -H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" …/cfd_tunnel/${tunnelId}/configurations`,
    },
    {
      kind: "updateRecord",
      description: rawText("Point shop.yx.app at tunnel “MacBook-Pro” (was A 192.0.2.10)"),
      command: null,
    },
    {
      kind: "verify",
      description: rawText("Check https://shop.yx.app works"),
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
    id: 3,
    at: now - 4 * 60_000,
    summary: "Rename app.teispace.com to web.teispace.com → http://localhost:5173",
    outcome: "applied",
    detail: [],
    record: {
      kind: "updateRoute",
      hostnames: ["app.teispace.com", "web.teispace.com"],
      tunnel: "MacBook-Pro",
      steps: [
        {
          step: {
            kind: "putConfig",
            description: rawText("Update tunnel “MacBook-Pro” to serve 4 routes"),
            command:
              'curl -X PUT -H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" …/cfd_tunnel/6ff42ae2/configurations',
          },
          state: { state: "done" },
        },
        {
          step: {
            kind: "updateRecord",
            description: rawText(
              "Point web.teispace.com at tunnel “MacBook-Pro” (was A 192.0.2.10)",
            ),
            command: null,
          },
          state: { state: "done" },
        },
        {
          step: {
            kind: "deleteRecord",
            description: rawText(
              "Delete DNS record app.teispace.com (CNAME 6ff42ae2.cfargotunnel.com)",
            ),
            command:
              'curl -X DELETE -H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" …/dns_records/r4',
          },
          state: { state: "done" },
        },
      ],
      changes: [
        {
          area: "route",
          hostname: "app.teispace.com",
          path: null,
          before: rawText("http://localhost:5173"),
          after: null,
        },
        {
          area: "route",
          hostname: "web.teispace.com",
          path: null,
          before: null,
          after: rawText("http://localhost:5173 · noTLSVerify"),
        },
        {
          area: "dns",
          hostname: "app.teispace.com",
          path: null,
          before: rawText("CNAME 6ff42ae2.cfargotunnel.com"),
          after: null,
        },
        {
          area: "dns",
          hostname: "web.teispace.com",
          path: null,
          before: rawText("A 192.0.2.10"),
          after: rawText("proxied CNAME to tunnel “MacBook-Pro”"),
        },
      ],
    },
  },
  {
    id: 2,
    at: now - 40 * 60_000,
    summary: "Add api.xyz.dev (path ^/v1/) → http://localhost:8000",
    outcome: "rolledBack",
    detail: ["Failed: Cloudflare API error: record already exists (code 81053)"],
    record: {
      kind: "addRoute",
      hostnames: ["api.xyz.dev"],
      tunnel: "MacBook-Pro",
      steps: [
        {
          step: {
            kind: "putConfig",
            description: rawText("Update tunnel “MacBook-Pro” to serve 5 routes"),
            command: null,
          },
          state: { state: "undone" },
        },
        {
          step: {
            kind: "createRecord",
            description: rawText("Add DNS record api.xyz.dev → tunnel “MacBook-Pro”"),
            command: "cloudflared tunnel route dns 'MacBook-Pro' api.xyz.dev",
          },
          state: {
            state: "failed",
            message: rawText("Cloudflare API error: record already exists (code 81053)"),
          },
        },
      ],
      changes: [
        {
          area: "route",
          hostname: "api.xyz.dev",
          path: "^/v1/",
          before: null,
          after: rawText("http://localhost:8000"),
        },
        {
          area: "dns",
          hostname: "api.xyz.dev",
          path: null,
          before: null,
          after: rawText("proxied CNAME to tunnel “MacBook-Pro”"),
        },
      ],
    },
  },
  {
    id: 1,
    at: now - 3 * 3_600_000,
    summary: "Add app.teispace.com → http://localhost:5173",
    outcome: "applied",
    detail: ["Add DNS record app.teispace.com → tunnel “MacBook-Pro”"],
    record: null,
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
            notifyConnectors: patch.notifyConnectors ?? settings.notifyConnectors,
            notifyQuickShares: patch.notifyQuickShares ?? settings.notifyQuickShares,
            notifyDoctor: patch.notifyDoctor ?? settings.notifyDoctor,
            ignoredIssues: settings.ignoredIssues,
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
        case "routes_preview": {
          const change = payload["change"] as { route?: { access?: unknown } } | undefined;
          return change?.route?.access ? protectedPlan : addPlan;
        }
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
                  status: payload["hostname"] === "xyz.dev" ? 302 : 200,
                  failure: null,
                  message: null,
                  protected: payload["hostname"] === "xyz.dev",
                }),
              900,
            ),
          );
        case "routes_drift":
          return new URLSearchParams(window.location.search).has("drift")
            ? {
                tunnelId,
                temporary: false,
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
              name: "MacBook-Pro",
              status: "healthy",
              createdAt: new Date(now - 9 * 86_400_000).toISOString(),
              routes: 4,
              connectors: [
                {
                  id: "a7edf147-b8b9-4cfa-bbb3-fe698d0c4cca",
                  version: "2026.9.1",
                  originIp: "203.0.113.7",
                  thisMac: true,
                  connections: [
                    { colo: "ams01", openedAt: "" },
                    { colo: "fra08", openedAt: "" },
                  ],
                },
              ],
              thisMac: true,
              isDefault: true,
              connector: { state: "healthy", connections: 4 },
            },
            {
              id: "b1946ac9-2a6f-4e8e-9d51-1f0e7a3c2b11",
              name: "home-lab",
              status: "healthy",
              createdAt: new Date(now - 90 * 86_400_000).toISOString(),
              routes: 2,
              connectors: [
                {
                  id: "5d0f6c1e-3b2a-4f8e-9c7d-2e1f0a9b8c7d",
                  version: "2026.8.0",
                  originIp: "198.51.100.24",
                  thisMac: false,
                  connections: [
                    { colo: "lhr01", openedAt: "" },
                    { colo: "cdg02", openedAt: "" },
                  ],
                },
              ],
              thisMac: false,
              isDefault: false,
              connector: null,
            },
          ] satisfies TunnelSummary[];
        case "tunnels_remote_logs":
          return {
            state: { state: "streaming" },
            lines: [
              {
                time: new Date(now - 60_000).toISOString(),
                level: "info",
                message: "Registered tunnel connection",
                error: null,
              },
              {
                time: new Date(now - 20_000).toISOString(),
                level: "error",
                message: "Request failed",
                error: "dial tcp 127.0.0.1:8123: connect: connection refused",
              },
            ],
          };
        case "import_scan":
          return [
            {
              configPath: "~/.cloudflared/config.yml",
              tunnel: "2b8a3f54-0c0d-4c1e-9f7a-1d2c3b4a5e6f",
              accountId: "acc-personal",
              tunnelId: "2b8a3f54-0c0d-4c1e-9f7a-1d2c3b4a5e6f",
              temporary: false,
              routes: [
                {
                  hostname: "blog.teispace.com",
                  path: null,
                  service: "http://localhost:2368",
                  unsupported: null,
                },
                {
                  hostname: "grafana.xyz.dev",
                  path: null,
                  service: "http://localhost:3001",
                  unsupported: null,
                },
                {
                  hostname: "app.teispace.com",
                  path: null,
                  service: "http://localhost:5173",
                  unsupported: null,
                },
                {
                  hostname: "nas.example.org",
                  path: null,
                  service: "http://192.168.1.20:5000",
                  unsupported: null,
                },
              ],
              hasGlobalOptions: true,
              problem: null,
            },
          ];
        case "foreign_list":
          return [
            {
              pid: 812,
              command: "/opt/homebrew/bin/cloudflared tunnel run --token [redacted]",
              mode: { type: "named", tunnel: null, config: null },
              service: true,
              metrics: "127.0.0.1:20241",
              connections: 4,
            },
          ];
        case "tunnels_always_on":
          return { supported: true, enabled: false };
        case "routes_export":
          return {
            fileName: "config.yml",
            contents:
              '# cloudflared configuration for tunnel "MacBook-Pro" (6ff42ae2-765d-4adf-8112-31c55c1551ef), exported by Teitunnel.\ntunnel: 6ff42ae2-765d-4adf-8112-31c55c1551ef\ncredentials-file: /etc/cloudflared/6ff42ae2-765d-4adf-8112-31c55c1551ef.json\ningress:\n  - hostname: "app.teispace.com"\n    service: "http://localhost:5173"\n  - hostname: "api.xyz.dev"\n    path: "^/v1/"\n    service: "http://localhost:8000"\n  - service: "http_status:404"\n',
          };
        case "routes_logs":
          return payload["hostname"] === "api.xyz.dev"
            ? [
                {
                  time: null,
                  level: "error",
                  message: "Request failed",
                  error:
                    "Unable to reach the origin service. The service may be down or it may not be responding to traffic from cloudflared: dial tcp [::1]:8000: connect: connection refused",
                },
              ]
            : [];
        case "tunnels_logs":
          return [
            {
              time: null,
              level: "info",
              message: "Starting tunnel tunnelID=6ff42ae2",
              error: null,
            },
            {
              time: null,
              level: "info",
              message: "Registered tunnel connection connIndex=0 location=ams01",
              error: null,
            },
            {
              time: null,
              level: "info",
              message: "Registered tunnel connection connIndex=1 location=fra08",
              error: null,
            },
          ];
        case "tunnels_traffic":
          return {
            series: mockSeries(now, 3_600, 1),
            totalRequests: 18_204,
            totalErrors: 12,
            connections: 4,
            rttMs: 18.4,
            locations: ["ams01", "fra08"],
          };
        case "tunnels_traffic_history": {
          const week = payload["range"] === "week";
          return mockSeries(now, week ? 336 : 288, week ? 1_800 : 300);
        }
        case "doctor_run":
          return [
            {
              id: "network.excluded:acc-personal:192.168.1.0/24",
              check: "network.excluded",
              severity: "warning",
              accountId: "acc-personal",
              subject: "192.168.1.0/24",
              label: rawText("192.168.1.0/24"),
              title: core("doctor.networkExcluded.title", { network: "192.168.1.0/24" }),
              detail: core("doctor.networkExcluded.detail", { network: "192.168.1.0/24" }),
              evidence: [core("doctor.networkExcluded.excluded", { range: "192.168.0.0/16" })],
              fixes: [],
            },
            {
              id: "dns.missing:acc-personal:docs.teispace.com",
              check: "dns.missing",
              severity: "error",
              accountId: "acc-personal",
              subject: "docs.teispace.com",
              label: rawText("docs.teispace.com"),
              title: core("doctor.dnsMissing.title", { hostname: "docs.teispace.com" }),
              detail: core("doctor.dnsMissing.detail"),
              evidence: [],
              fixes: [
                {
                  type: "change",
                  label: core("doctor.fix.fixDns"),
                  change: {
                    type: "addRoute",
                    route: {
                      hostname: "docs.teispace.com",
                      path: null,
                      origin: "http://localhost:4321",
                    },
                  },
                },
              ],
            },
            {
              id: "origin.not_listening:acc-personal:api.xyz.dev",
              check: "origin.not_listening",
              severity: "warning",
              accountId: "acc-personal",
              subject: "api.xyz.dev",
              label: rawText("api.xyz.dev"),
              title: core("doctor.originNotListening.title", { port: "8000" }),
              detail: core("doctor.originNotListening.detail"),
              evidence: [rawText("api.xyz.dev → http://localhost:8000")],
              fixes: [],
            },
            {
              id: "dns.orphan_foreign:acc-personal:old.xyz.dev",
              check: "dns.orphan_foreign",
              severity: "warning",
              accountId: "acc-personal",
              subject: "old.xyz.dev",
              label: rawText("old.xyz.dev"),
              title: core("doctor.orphanTunnel.title", { hostname: "old.xyz.dev" }),
              detail: core("doctor.orphanTunnel.detail"),
              evidence: [rawText("old.xyz.dev CNAME 0c1f…e2.cfargotunnel.com (proxied)")],
              fixes: [
                {
                  type: "change",
                  label: core("doctor.fix.deleteRecord"),
                  change: {
                    type: "deleteRecord",
                    zoneId: "9a7806061c88ada191ed06f989cc3dac",
                    hostname: "old.xyz.dev",
                    recordId: "r9",
                  },
                },
              ],
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

/** A plausible traffic series: `count` intervals of `span` seconds ending at `now`. */
function mockSeries(now: number, count: number, span: number) {
  const at: number[] = [];
  const requests: number[] = [];
  const errors: number[] = [];
  const rttMs: (number | null)[] = [];
  for (let i = 0; i < count; i++) {
    // Leave a gap, as when the Mac slept.
    if (i > count * 0.3 && i < count * 0.36) continue;
    const t = i / count;
    const rate = 6 + 5 * Math.sin(t * 9) + 3 * Math.sin(t * 41) + (t > 0.8 ? 7 : 0);
    at.push(now - (count - i) * span * 1000);
    requests.push(Math.max(0, Math.round(rate * span)));
    errors.push(i % 97 === 0 ? Math.max(1, Math.round(span / 20)) : 0);
    rttMs.push(18 + 4 * Math.sin(t * 13));
  }
  const zeros = at.map(() => 0);
  return {
    at,
    span: at.map(() => span),
    requests,
    errors,
    ok: requests.map((n) => Math.round(n * 0.96)),
    redirects: requests.map((n) => Math.round(n * 0.02)),
    clientErrors: requests.map((n) => Math.round(n * 0.015)),
    serverErrors: errors,
    concurrent: zeros,
    connections: at.map(() => 4),
    rttMs,
  };
}
