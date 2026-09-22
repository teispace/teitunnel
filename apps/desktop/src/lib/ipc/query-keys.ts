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
  },
  qr: (value: string) => ["qr", value] as const,
  services: {
    all: () => ["services"] as const,
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
      return [queryKeys.settings.all()];
    case "quickShares":
      return [queryKeys.quickShares.all()];
  }
}
