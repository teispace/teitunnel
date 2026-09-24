import type { QueryClient, QueryKey } from "@tanstack/react-query";
import type { EntityKind } from "./bindings";

/** The only place query keys are built. Keys are hierarchical: invalidating a prefix covers children. */
export const queryKeys = {
  app: {
    info: () => ["app", "info"] as const,
  },
  settings: {
    all: () => ["settings"] as const,
  },
  quickShares: {
    all: () => ["quickShares"] as const,
    stats: (id: string) => ["quickShares", "stats", id] as const,
    logs: (id: string) => ["quickShares", "logs", id] as const,
    /** Shares on your own domains. */
    domain: () => ["quickShares", "domain"] as const,
  },
  qr: (value: string) => ["qr", value] as const,
  services: {
    all: () => ["services"] as const,
  },
  accounts: {
    all: () => ["accounts"] as const,
    capabilities: (id: string) => ["accounts", "capabilities", id] as const,
    cert: () => ["accounts", "cert"] as const,
  },
  domains: {
    all: () => ["domains"] as const,
    list: (accountId: string) => ["domains", accountId] as const,
  },
  routes: {
    all: () => ["routes"] as const,
    overview: (accountId: string) => ["routes", "overview", accountId] as const,
    drift: (accountId: string) => ["routes", "drift", accountId] as const,
    activity: (accountId: string) => ["routes", "activity", accountId] as const,
    tunnels: (accountId: string) => ["routes", "tunnels", accountId] as const,
    foreign: () => ["routes", "foreign"] as const,
  },
  doctor: {
    all: () => ["doctor"] as const,
  },
  /** Checks of a share on your domain through Cloudflare (run once; refetched on demand). */
  domainShareCheck: (accountId: string, hostname: string) =>
    ["domainShareCheck", accountId, hostname] as const,
  analytics: {
    all: () => ["analytics"] as const,
    summary: (accountId: string, range: string, hostnames: readonly string[]) =>
      ["analytics", "summary", accountId, range, hostnames.join(",")] as const,
    route: (accountId: string, hostname: string, path: string | null, range: string) =>
      ["analytics", "route", accountId, hostname, path ?? "", range] as const,
  },
  uptime: {
    all: () => ["uptime"] as const,
    list: () => ["uptime", "list"] as const,
    route: (hostname: string, path: string | null, range: string) =>
      ["uptime", "route", hostname, path ?? "", range] as const,
  },
  alerts: {
    rules: () => ["alerts", "rules"] as const,
  },
  updates: {
    status: () => ["updates", "status"] as const,
  },
  binary: {
    status: () => ["binary", "status"] as const,
    update: () => ["binary", "update"] as const,
  },
} as const;

/** Query key prefixes to invalidate when an entity of `kind` changes. */
export function keysForEntity(kind: EntityKind): readonly (readonly string[])[] {
  switch (kind) {
    case "settings":
      return [queryKeys.settings.all(), queryKeys.alerts.rules()];
    case "quickShares":
      return [queryKeys.quickShares.all()];
    case "accounts":
      return [
        queryKeys.accounts.all(),
        queryKeys.domains.all(),
        queryKeys.routes.all(),
        queryKeys.doctor.all(),
      ];
    case "routes":
      // Alerts also announce themselves as a routes change: uptime moved.
      return [queryKeys.routes.all(), queryKeys.doctor.all(), queryKeys.uptime.all()];
    case "updates":
      return [queryKeys.updates.status()];
  }
}

/**
 * Refetches what a change touched and resolves once the new data is in. Mutations return
 * it from `onSuccess`/`onSettled`, so they stay pending (their buttons busy) until the
 * screen shows the result, instead of the old data lingering after the button came back.
 */
export function refresh(client: QueryClient, ...keys: readonly QueryKey[]): Promise<unknown> {
  return Promise.all(keys.map((queryKey) => client.invalidateQueries({ queryKey })));
}
