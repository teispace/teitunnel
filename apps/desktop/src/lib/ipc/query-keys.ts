import type { EntityKind } from "./bindings";

/** The only place query keys are built. Keys are hierarchical: invalidating a prefix covers children. */
export const queryKeys = {
  app: {
    info: () => ["app", "info"] as const,
  },
  settings: {
    all: () => ["settings"] as const,
  },
} as const;

/** Query key prefixes to invalidate when an entity of `kind` changes. */
export function keysForEntity(kind: EntityKind): readonly (readonly string[])[] {
  switch (kind) {
    case "settings":
      return [queryKeys.settings.all()];
  }
}
