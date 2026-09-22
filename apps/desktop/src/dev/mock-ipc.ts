import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import type {
  AppInfo,
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
