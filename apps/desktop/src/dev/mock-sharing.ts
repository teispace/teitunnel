import type {
  AiAgentsView,
  DomainShare,
  NameSuggestion,
  RouteSchedule,
  Schedule,
} from "@/lib/ipc/bindings";

/** Mock sharing extras for development (pause, schedules, names, folders, agents). */

const now = Date.now();
const office: Schedule = {
  days: ["mon", "tue", "wed", "thu", "fri"],
  from: "09:00",
  to: "18:00",
  timeZone: null,
};

let domainShares: DomainShare[] = [
  {
    accountId: "acc-personal",
    hostname: "shop.dev.teispace.com",
    origin: "http://127.0.0.1:52790",
    owner: "app",
    expiresAt: null,
    createdAt: now - 42 * 60_000,
    source: "http://localhost:5173",
    folder: false,
    paused: true,
    schedule: office,
  },
  {
    accountId: "acc-personal",
    hostname: "docs.teispace.com",
    origin: "http://127.0.0.1:52811",
    owner: "app",
    expiresAt: now + 55 * 60_000,
    createdAt: now - 5 * 60_000,
    source: "~/Projects/docs/dist",
    folder: true,
    paused: false,
    schedule: null,
  },
];

function schedules(): RouteSchedule[] {
  return domainShares
    .filter((s) => s.schedule !== null)
    .map((s) => ({
      accountId: s.accountId,
      hostname: s.hostname,
      schedule: s.schedule as Schedule,
      on: !s.paused,
      nextChange: now + 14 * 3_600_000,
    }));
}

const suggestions = (domain: string): NameSuggestion[] => [
  { template: `{project}.${domain}`, hostname: `teitunnel-web.${domain}`, remembered: false },
  {
    template: `{branch}-{project}.${domain}`,
    hostname: `login-fix-teitunnel-web.${domain}`,
    remembered: false,
  },
  {
    template: `{user}-{project}.${domain}`,
    hostname: `demo-teitunnel-web.${domain}`,
    remembered: false,
  },
];

/** `?agents` shows a connected agent with a change waiting for an answer. */
function agents(): AiAgentsView {
  if (!new URLSearchParams(window.location.search).has("agents")) {
    return { agents: [], approvals: [] };
  }
  return {
    agents: [
      { name: "claude-code", version: "2.1.4", mode: "ask", connectedAt: now - 8 * 60_000 },
      { name: "cursor", version: null, mode: "read-only", connectedAt: now - 70 * 60_000 },
    ],
    approvals: [
      {
        agent: "claude-code",
        title: "Add api.teispace.com → http://localhost:8000",
        askedAt: now - 20_000,
      },
    ],
  };
}

export function sharingMock(cmd: string, payload: Record<string, unknown>): unknown {
  switch (cmd) {
    case "domain_shares_list":
      return domainShares;
    case "sharing_schedules":
      return schedules();
    case "sharing_set_paused":
      domainShares = domainShares.map((s) =>
        s.hostname === payload["hostname"] ? { ...s, paused: Boolean(payload["paused"]) } : s,
      );
      return null;
    case "sharing_set_schedule":
      domainShares = domainShares.map((s) =>
        s.hostname === payload["hostname"]
          ? { ...s, schedule: (payload["schedule"] as Schedule | null) ?? null }
          : s,
      );
      return null;
    case "sharing_name_suggestions":
      return suggestions(String(payload["domain"] ?? ""));
    case "sharing_expand_name":
      return String(payload["hostname"] ?? "")
        .replace("{project}", "teitunnel-web")
        .replace("{branch}", "login-fix")
        .replace("{user}", "demo");
    case "sharing_choose_folder":
      return "~/Projects/docs/dist";
    case "sharing_folder":
      return { path: payload["path"], listing: payload["listing"], spa: payload["spa"] };
    case "ai_agents":
      return agents();
    case "integrations_get":
    case "integrations_set":
    case "integrations_revoke":
      return {
        controlEnabled: true,
        deepLinksEnabled: true,
        clients: [
          { name: "vscode", version: "0.2.0", approvedAt: now - 3 * 86_400_000 },
          { name: "raycast", version: "0.2.0", approvedAt: now - 86_400_000 },
        ],
        shortcut: { enabled: true, keys: "CommandOrControl+Alt+Shift+S", action: "shareDevServer" },
      };
    case "inspect_openapi_save":
      return {
        path: "~/Downloads/teitunnel-openapi.json",
        summary: { requests: 42, skipped: 7, paths: 6, operations: 9, hosts: ["api.teispace.com"] },
      };
    default:
      return undefined;
  }
}
