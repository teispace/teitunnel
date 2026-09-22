import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import type {
  Account,
  AppInfo,
  Capabilities,
  Domain,
  LocalService,
  QuickShare,
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
