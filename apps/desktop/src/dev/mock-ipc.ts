import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { rawText } from "@/lib/i18n";
import { analyticsMock } from "./mock-analytics";
import { commentsMock } from "./mock-comments";
import { inspectorMock } from "./mock-inspector";
import { localDomainsMock } from "./mock-local-domains";
import { projectsMock } from "./mock-projects";
import { sharingMock } from "./mock-sharing";

/** A message the Rust core would send (`core.*` in the catalog). */
const core = (key: string, args: Record<string, string | number> = {}) => ({
  key: `core.${key}`,
  args,
});

import type {
  Account,
  ActivityEntry,
  AiClientView,
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
  SnapshotView,
  TunnelSummary,
  UpdateStatus,
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
  notifyAlerts: true,
  quietHours: { enabled: false, from: 22 * 60, to: 7 * 60 },
  checkForUpdates: true,
  // Screenshots and design review show the app as it looks after the one-time offer.
  cliOfferDismissed: true,
  exposureCheck: true,
  ignoredIssues: [],
};

let aiClients: AiClientView[] = [
  ["claude-code", "Claude Code", true, true],
  ["claude-desktop", "Claude Desktop", false, false],
  ["cursor", "Cursor", true, false],
  ["vscode", "VS Code", true, false],
  ["codex", "Codex", false, false],
  ["windsurf", "Windsurf", false, false],
  ["zed", "Zed", false, false],
  ["gemini-cli", "Gemini CLI", false, false],
].map(([id, name, detected, connected]) => ({
  id: String(id),
  name: String(name),
  path: `~/.${String(id)}/mcp.json`,
  detected: Boolean(detected),
  connected: Boolean(connected),
  problem: null,
}));

/** `?update` shows a downloaded update (sidebar notice, Settings). */
function updateStatus(): UpdateStatus {
  const ready = new URLSearchParams(window.location.search).has("update");
  return {
    currentVersion: "0.2.0",
    unsupported: null,
    automatic: settings.checkForUpdates,
    lastChecked: now - 12 * 60_000,
    state: ready
      ? { state: "ready", version: "0.2.1", notes: "### Features\n- Load balancing health" }
      : { state: "upToDate" },
    installOnQuit: true,
  };
}

let shares: QuickShare[] = [
  {
    id: "qs-1",
    origin: "http://localhost:5173",
    url: "https://quiet-river-lamp-orbit.trycloudflare.com",
    status: { status: "live" },
    startedAt: now - 12 * 60_000,
    stopAt: now + 48 * 60_000,
    inspected: true,
    paused: false,
    folder: null,
    hostHeader: { value: "localhost:5173", autoFor: "vite" },
    check: {
      hostname: "quiet-river-lamp-orbit.trycloudflare.com",
      status: 200,
      failure: null,
      message: null,
      protected: false,
      eventStream: false,
      transient: false,
    },
  },
  {
    id: "qs-3",
    origin: "http://localhost:3001",
    url: "https://amber-field-cloud-note.trycloudflare.com",
    status: { status: "live" },
    startedAt: now - 2 * 60_000,
    stopAt: null,
    inspected: true,
    paused: false,
    folder: null,
    hostHeader: null,
    check: {
      hostname: "amber-field-cloud-note.trycloudflare.com",
      status: 403,
      failure: {
        type: "hostRejected",
        rejection: {
          server: "rails",
          host: "amber-field-cloud-note.trycloudflare.com",
          hostHeader: "localhost:3001",
          hostHeaderSafe: false,
          configFile: "config/environments/development.rb",
          configLine: 'config.hosts << ".trycloudflare.com"',
        },
      },
      message: { key: "core.verify.hostRejected", args: { server: "Rails" } },
      protected: false,
      eventStream: false,
      transient: false,
    },
  },
  {
    id: "qs-4",
    origin: "http://127.0.0.1:52811",
    url: "https://mellow-stone-paper-kite.trycloudflare.com",
    status: { status: "live" },
    startedAt: now - 6 * 60_000,
    stopAt: null,
    inspected: true,
    paused: false,
    folder: { path: "~/Projects/docs/dist", listing: false, spa: true },
    hostHeader: null,
    check: null,
  },
  {
    id: "qs-2",
    origin: "http://localhost:3000",
    url: null,
    status: { status: "starting" },
    startedAt: now - 3_000,
    stopAt: null,
    inspected: true,
    paused: false,
    folder: null,
    hostHeader: null,
    check: null,
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
    folder: null,
    origin: "http://localhost:5173",
  },
  {
    port: 3000,
    allInterfaces: true,
    pid: 4102,
    process: "node",
    kind: "next",
    project: "marketing",
    folder: null,
    origin: "http://localhost:3000",
  },
  {
    port: 8000,
    allInterfaces: false,
    pid: 4103,
    process: "Python",
    kind: "python",
    project: "api",
    folder: null,
    origin: "http://localhost:8000",
  },
  {
    port: 5000,
    allInterfaces: true,
    pid: 512,
    process: "ControlCenter",
    kind: "system",
    project: null,
    folder: null,
    origin: "http://localhost:5000",
  },
];

const accounts: Account[] = [
  { id: "acc-personal", name: "Teispace", credential: "apiToken", limitedZone: null },
];

const domains: Domain[] = [
  {
    id: "00000000000000000000000000000a01",
    name: "teispace.com",
    status: "active",
    nameServers: ["ada.ns.cloudflare.com", "bob.ns.cloudflare.com"],
    originalNameServers: [],
    plan: "Free Website",
    paused: false,
  },
  {
    id: "00000000000000000000000000000a02",
    name: "teispace.dev",
    status: "active",
    nameServers: ["ada.ns.cloudflare.com", "bob.ns.cloudflare.com"],
    originalNameServers: [],
    plan: "Pro Website",
    paused: false,
  },
  {
    id: "00000000000000000000000000000a03",
    name: "teispace.app",
    status: "pending",
    nameServers: ["kate.ns.cloudflare.com", "rick.ns.cloudflare.com"],
    originalNameServers: ["ns1.teispace.app", "ns2.teispace.app"],
    plan: "Free Website",
    paused: false,
  },
];

const capabilities: Capabilities = {
  zonesRead: "yes",
  tunnelsRead: "yes",
  tunnelsEdit: "yes",
  accessEdit: "no",
  analytics: "yes",
  workersEdit: "yes",
  edgeRules: "yes",
  serviceTokens: "yes",
  d1: "yes",
  zones: domains.map((d) => ({
    zoneId: d.id,
    zoneName: d.name,
    dnsEdit: d.name === "teispace.app" ? "no" : "yes",
    workersRoutes: "yes",
  })),
};

const snapshots: SnapshotView[] = [
  {
    id: "s1",
    accountId: "a1",
    name: "Launch page",
    url: "https://preview.teispace.com",
    hostname: "preview.teispace.com",
    workersDev: false,
    script: "teitunnel-launch-page",
    source: {
      type: "build",
      project: "~/Projects/launch",
      command: "pnpm run build",
      output: "~/Projects/launch/dist",
    },
    spa: true,
    password: false,
    access: null,
    expiresAt: null,
    createdAt: Date.now() - 3 * 86_400_000,
    updatedAt: Date.now() - 20 * 60_000,
    liveVersion: 3,
    versions: 3,
    files: 48,
    bytes: 1_840_000,
    comments: true,
  },
  {
    id: "s2",
    accountId: "a1",
    name: "Design review",
    url: "https://teitunnel-design-review.teispace.workers.dev",
    hostname: "teitunnel-design-review.teispace.workers.dev",
    workersDev: true,
    script: "teitunnel-design-review",
    source: { type: "crawl", url: "http://localhost:5173/" },
    spa: false,
    password: true,
    access: null,
    expiresAt: Date.now() + 6 * 86_400_000,
    createdAt: Date.now() - 86_400_000,
    updatedAt: Date.now() - 86_400_000,
    liveVersion: 1,
    versions: 1,
    files: 12,
    bytes: 312_000,
    comments: false,
  },
];

const snapshotPlan: PlanView = {
  steps: [
    {
      kind: "snapshot",
      description: core("snapshot.step.upload", { count: 48, total: 48, size: "1.8 MB" }),
      command: null,
    },
    {
      kind: "snapshot",
      description: core("snapshot.step.createWorker", { script: "teitunnel-launch" }),
      command: null,
    },
    {
      kind: "snapshotAddress",
      description: core("snapshot.step.attachDomain", { hostname: "preview.teispace.com" }),
      command: null,
    },
  ],
  warnings: [],
  requiresConfirmation: false,
  fingerprint: "snapshot",
};

const tunnelId = "00000000-0000-4000-8000-000000000001";
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
      id: "00000000-0000-4000-8000-000000000002",
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
      balanced: true,
      options: {},
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
      balanced: false,
      options: {},
      zone: "teispace.com",
      dns: { state: "missing" },
      access: null,
      client: null,
    },
    {
      hostname: "teispace.dev",
      path: null,
      origin: "http://localhost:3000",
      local: true,
      tunnelId,
      temporary: false,
      balanced: false,
      options: {},
      zone: "teispace.dev",
      dns: { state: "ok" },
      access: { emails: ["team@teispace.com"], emailDomains: ["teispace.com"] },
      client: null,
    },
    {
      hostname: "ssh.teispace.dev",
      path: null,
      origin: "ssh://localhost:22",
      local: true,
      tunnelId,
      temporary: false,
      balanced: false,
      options: {},
      zone: "teispace.dev",
      dns: { state: "ok" },
      access: { emails: ["team@teispace.com"], emailDomains: [] },
      client: {
        protocol: "ssh",
        command: 'ssh -o ProxyCommand="cloudflared access ssh --hostname %h" ssh.teispace.dev',
        localAddress: null,
        sshConfig: "Host ssh.teispace.dev\n  ProxyCommand cloudflared access ssh --hostname %h",
      },
    },
    {
      hostname: "api.teispace.dev",
      path: "^/v1/",
      origin: "http://localhost:8000",
      local: true,
      tunnelId,
      temporary: false,
      balanced: false,
      options: {},
      zone: "teispace.dev",
      dns: { state: "ok" },
      access: null,
      client: null,
    },
  ],
  zones: [
    { id: "00000000000000000000000000000a01", name: "teispace.com" },
    { id: "00000000000000000000000000000a02", name: "teispace.dev" },
    { id: "00000000000000000000000000000a03", name: "teispace.app" },
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
      description: rawText(
        "Require a login for shop.teispace.app: team@teispace.com, anyone at @teispace.com",
      ),
      command: null,
    },
    {
      kind: "putConfig",
      description: rawText("Update tunnel “MacBook-Pro” to serve 5 routes"),
      command: null,
    },
    {
      kind: "createRecord",
      description: rawText("Point shop.teispace.app at tunnel “MacBook-Pro”"),
      command: null,
    },
    {
      kind: "verify",
      description: rawText("Check https://shop.teispace.app works"),
      command: null,
    },
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
      description: rawText("Point shop.teispace.app at tunnel “MacBook-Pro” (was A 192.0.2.10)"),
      command: null,
    },
    {
      kind: "verify",
      description: rawText("Check https://shop.teispace.app works"),
      command: "curl -I https://shop.teispace.app",
    },
  ],
  warnings: [
    {
      type: "replacesForeignRecord",
      hostname: "shop.teispace.app",
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
              'curl -X PUT -H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" …/cfd_tunnel/00000000/configurations',
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
              "Delete DNS record app.teispace.com (CNAME 00000000.cfargotunnel.com)",
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
          before: rawText("CNAME 00000000.cfargotunnel.com"),
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
    summary: "Add api.teispace.dev (path ^/v1/) → http://localhost:8000",
    outcome: "rolledBack",
    detail: ["Failed: Cloudflare API error: record already exists (code 81053)"],
    record: {
      kind: "addRoute",
      hostnames: ["api.teispace.dev"],
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
            description: rawText("Add DNS record api.teispace.dev → tunnel “MacBook-Pro”"),
            command: "cloudflared tunnel route dns 'MacBook-Pro' api.teispace.dev",
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
          hostname: "api.teispace.dev",
          path: "^/v1/",
          before: null,
          after: rawText("http://localhost:8000"),
        },
        {
          area: "dns",
          hostname: "api.teispace.dev",
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
        case "updates_status":
        case "updates_check":
          return updateStatus();
        case "updates_restart":
          return null;
        case "cli_status":
          return { state: "notInstalled", path: "/opt/homebrew/bin/teitunnel", command: null };
        case "cli_install":
          return { state: "installed", path: "/opt/homebrew/bin/teitunnel" };
        case "ai_clients_status":
        case "ai_clients_connect":
        case "ai_clients_disconnect": {
          const id = payload["clientId"];
          if (typeof id === "string") {
            aiClients = aiClients.map((c) =>
              c.id === id ? { ...c, connected: cmd === "ai_clients_connect" } : c,
            );
          }
          return {
            command: "/Applications/Teitunnel.app/Contents/MacOS/teitunnel-cli",
            clients: aiClients,
          };
        }
        case "settings_set": {
          const patch = payload["patch"] as SettingsPatch;
          settings = {
            theme: patch.theme ?? settings.theme,
            showInMenuBar: patch.showInMenuBar ?? settings.showInMenuBar,
            notifyConnectors: patch.notifyConnectors ?? settings.notifyConnectors,
            notifyQuickShares: patch.notifyQuickShares ?? settings.notifyQuickShares,
            notifyDoctor: patch.notifyDoctor ?? settings.notifyDoctor,
            notifyAlerts: patch.notifyAlerts ?? settings.notifyAlerts,
            quietHours: patch.quietHours ?? settings.quietHours,
            checkForUpdates: patch.checkForUpdates ?? settings.checkForUpdates,
            cliOfferDismissed: patch.cliOfferDismissed ?? settings.cliOfferDismissed,
            exposureCheck: patch.exposureCheck ?? settings.exposureCheck,
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
            inspected: true,
            paused: false,
            folder: null,
            hostHeader: null,
            check: null,
          };
          shares = [share, ...shares];
          return share;
        }
        case "quick_share_set_inspected": {
          const id = payload["id"];
          shares = shares.map((s) =>
            s.id === id ? { ...s, inspected: payload["inspect"] === true } : s,
          );
          return shares.find((s) => s.id === id);
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
                  verify: ["shop.teispace.app"],
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
                  status: payload["hostname"] === "teispace.dev" ? 302 : 200,
                  failure: null,
                  message: null,
                  protected: payload["hostname"] === "teispace.dev",
                }),
              900,
            ),
          );
        case "routes_balance_health":
          return [
            {
              tunnelId,
              name: "MacBook Pro",
              enabled: true,
              healthyRegions: 3,
              regions: 3,
              reason: null,
            },
            {
              tunnelId: "t-server",
              name: "home-lab",
              enabled: true,
              healthyRegions: 2,
              regions: 3,
              reason: "HTTP timeout occurred",
            },
          ];
        case "routes_drift":
          return new URLSearchParams(window.location.search).has("drift")
            ? {
                tunnelId,
                temporary: false,
                balanced: false,
                options: {},
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
                  id: "00000000-0000-4000-8000-00000000c001",
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
              id: "00000000-0000-4000-8000-000000000003",
              name: "home-lab",
              status: "healthy",
              createdAt: new Date(now - 90 * 86_400_000).toISOString(),
              routes: 2,
              connectors: [
                {
                  id: "00000000-0000-4000-8000-00000000c002",
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
              tunnel: "00000000-0000-4000-8000-000000000004",
              accountId: "acc-personal",
              tunnelId: "00000000-0000-4000-8000-000000000004",
              temporary: false,
              balanced: false,
              options: {},
              routes: [
                {
                  hostname: "blog.teispace.com",
                  path: null,
                  service: "http://localhost:2368",
                  unsupported: null,
                },
                {
                  hostname: "grafana.teispace.dev",
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
                  hostname: "nas.teispace.dev",
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
              '# cloudflared configuration for tunnel "MacBook-Pro" (00000000-0000-4000-8000-000000000001), exported by Teitunnel.\ntunnel: 00000000-0000-4000-8000-000000000001\ncredentials-file: /etc/cloudflared/00000000-0000-4000-8000-000000000001.json\ningress:\n  - hostname: "app.teispace.com"\n    service: "http://localhost:5173"\n  - hostname: "api.teispace.dev"\n    path: "^/v1/"\n    service: "http://localhost:8000"\n  - service: "http_status:404"\n',
          };
        case "routes_logs":
          return payload["hostname"] === "api.teispace.dev"
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
              message: "Starting tunnel tunnelID=00000000",
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
              id: "origin.not_listening:acc-personal:api.teispace.dev",
              check: "origin.not_listening",
              severity: "warning",
              accountId: "acc-personal",
              subject: "api.teispace.dev",
              label: rawText("api.teispace.dev"),
              title: core("doctor.originNotListening.title", { port: "8000" }),
              detail: core("doctor.originNotListening.detail"),
              evidence: [rawText("api.teispace.dev → http://localhost:8000")],
              fixes: [],
            },
            {
              id: "dns.orphan_foreign:acc-personal:old.teispace.dev",
              check: "dns.orphan_foreign",
              severity: "warning",
              accountId: "acc-personal",
              subject: "old.teispace.dev",
              label: rawText("old.teispace.dev"),
              title: core("doctor.orphanTunnel.title", { hostname: "old.teispace.dev" }),
              detail: core("doctor.orphanTunnel.detail"),
              evidence: [rawText("old.teispace.dev CNAME 0000…09.cfargotunnel.com (proxied)")],
              fixes: [
                {
                  type: "change",
                  label: core("doctor.fix.deleteRecord"),
                  change: {
                    type: "deleteRecord",
                    zoneId: "00000000000000000000000000000a02",
                    hostname: "old.teispace.dev",
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
        case "snapshots_list":
          return snapshots;
        case "reservations_list":
          return {
            cached: false,
            items: [
              {
                hostname: "design.teispace.com",
                owner: "design@Teispace-iMac",
                until: Date.parse("2026-12-31T00:00:00Z"),
                routed: true,
                mine: false,
                ended: false,
              },
              {
                hostname: "demo.teispace.com",
                owner: "teispace@Teispace-MacBook",
                until: null,
                routed: false,
                mine: true,
                ended: false,
              },
            ],
          };
        case "reservations_availability":
          return String(payload["hostname"]).startsWith("design.")
            ? {
                state: "held",
                hold: {
                  hostname: "design.teispace.com",
                  owner: "design@Teispace-iMac",
                  until: Date.parse("2026-12-31T00:00:00Z"),
                  kind: "reservation",
                },
              }
            : { state: "free" };
        case "snapshots_versions":
          return [
            {
              number: 3,
              createdAt: Date.now() - 20 * 60_000,
              files: 48,
              bytes: 1_840_000,
              live: true,
              spa: true,
              password: false,
            },
            {
              number: 2,
              createdAt: Date.now() - 26 * 3_600_000,
              files: 47,
              bytes: 1_790_000,
              live: false,
              spa: true,
              password: false,
            },
            {
              number: 1,
              createdAt: Date.now() - 3 * 86_400_000,
              files: 45,
              bytes: 1_720_000,
              live: false,
              spa: true,
              password: false,
            },
          ];
        case "snapshots_choose_folder":
          return "~/Projects/launch/dist";
        case "snapshots_prepare_folder":
        case "snapshots_prepare_crawl":
        case "snapshots_prepare_build":
          return {
            id: "prepared-1",
            source: { type: "folder", path: "~/Projects/launch/dist" },
            suggestedName: "launch",
            files: 48,
            bytes: 1_840_000,
            skipped: [{ path: ".env", reason: "secret" }],
            singlePage: true,
            crawl: null,
          };
        case "snapshots_preview":
          return snapshotPlan;
        case "snapshots_apply":
          return new Promise<Outcome>((resolve) =>
            setTimeout(
              () => resolve({ type: "applied", tunnelId: null, verify: [], connectorError: null }),
              600,
            ),
          );
        case "quick_share_qr":
          return '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="4" height="4" fill="currentColor"/><rect x="6" width="4" height="4" fill="currentColor"/><rect y="6" width="4" height="4" fill="currentColor"/></svg>';
        default:
          return (
            commentsMock(cmd, payload) ??
            analyticsMock(cmd, payload) ??
            projectsMock(cmd, payload) ??
            inspectorMock(cmd, payload) ??
            localDomainsMock(cmd, payload) ??
            sharingMock(cmd, payload) ??
            null
          );
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
