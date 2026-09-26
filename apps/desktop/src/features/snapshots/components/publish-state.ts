import type { PermissionNeed } from "@/features/accounts";
import { joinHostname, parseAllowed } from "@/features/routes";
import type {
  PreparedView,
  SnapshotChange,
  SnapshotOptions,
  SnapshotView,
  ZoneRef,
} from "@/lib/ipc/bindings";
import type { DetailsState } from "./details-step";
import type { SourceState } from "./source-step";

/** What the sheet does: publish a new Snapshot, or a new version of one. */
export type PublishMode =
  | { kind: "new"; accountId: string; siteUrl?: string }
  | { kind: "update"; snapshot: SnapshotView };

/** Where the files come from at first: the Snapshot's last source when updating. */
export function initialSource(mode: PublishMode): SourceState {
  if (mode.kind === "new") {
    return mode.siteUrl
      ? { kind: "site", folder: null, project: null, url: mode.siteUrl }
      : { kind: "folder", folder: null, project: null, url: "http://localhost:5173" };
  }
  const source = mode.snapshot.source;
  switch (source?.type) {
    case "folder":
      return { kind: "folder", folder: source.path, project: null, url: "" };
    case "crawl":
      return { kind: "site", folder: null, project: null, url: source.url };
    default:
      return { kind: "keep", folder: null, project: null, url: "" };
  }
}

/** The settings at first: the Snapshot's own when updating. */
export function initialDetails(mode: PublishMode, zones: readonly ZoneRef[]): DetailsState {
  const snapshot = mode.kind === "update" ? mode.snapshot : null;
  return {
    name: "",
    address: zones.length > 0 ? "domain" : "workersDev",
    hostname: joinHostname("preview", zones[0]?.name ?? ""),
    protection: snapshot?.access ? "login" : snapshot?.password ? "password" : "none",
    password: "",
    allowed: snapshot?.access
      ? [...snapshot.access.emails, ...snapshot.access.emailDomains.map((d) => `@${d}`)].join(", ")
      : "",
    spa: snapshot?.spa ?? false,
    expires: "never",
    comments: snapshot?.comments ?? false,
  };
}

/** What to collect for `source`; `null` when there's nothing to collect (keep, or not chosen yet). */
export function collectVars(
  source: SourceState,
):
  | { kind: "site"; url: string }
  | { kind: "build"; dir: string }
  | { kind: "folder"; path: string }
  | null {
  switch (source.kind) {
    case "site":
      return { kind: "site", url: source.url };
    case "build":
      return source.project ? { kind: "build", dir: source.project.dir } : null;
    case "folder":
      return source.folder ? { kind: "folder", path: source.folder } : null;
    case "keep":
      return null;
  }
}

/** Whether the source step can go on. */
export function sourceReady(source: SourceState): boolean {
  switch (source.kind) {
    case "folder":
      return source.folder !== null;
    case "build":
      return source.project !== null;
    case "site":
      return source.url.trim() !== "";
    case "keep":
      return true;
  }
}

export function snapshotOptions(details: DetailsState): SnapshotOptions {
  return {
    spa: details.spa,
    password:
      details.protection !== "password"
        ? { type: "remove" }
        : details.password
          ? { type: "set", password: details.password }
          : { type: "keep" },
    access: details.protection === "login" ? parseAllowed(details.allowed) : null,
    expiresInDays: details.expires === "never" ? null : Number(details.expires),
    comments: details.comments,
  };
}

/**
 * The change to review: a new version of the Snapshot being updated, or a new Snapshot
 * from the collected files (`null` until they're collected).
 */
export function changeFor(
  mode: PublishMode,
  details: DetailsState,
  prepared: PreparedView | null,
  zones: readonly ZoneRef[],
): SnapshotChange | null {
  if (mode.kind === "update") {
    return {
      type: "update",
      snapshot: mode.snapshot.id,
      prepared: prepared?.id ?? null,
      options: snapshotOptions(details),
    };
  }
  if (!prepared) return null;
  return {
    type: "publish",
    prepared: prepared.id,
    name: details.name,
    address:
      details.address === "domain" && zones.length > 0
        ? { type: "domain", hostname: details.hostname }
        : { type: "workersDev" },
    options: snapshotOptions(details),
  };
}

/** The permissions publishing with `details` needs. */
export function permissionNeeds(
  details: DetailsState,
  zones: readonly ZoneRef[],
  updating: boolean,
): PermissionNeed[] {
  const zone = zones.find(
    (z) => details.hostname === z.name || details.hostname.endsWith(`.${z.name}`),
  );
  return [
    { kind: "workers" },
    ...(!updating && details.address === "domain" && zone
      ? [{ kind: "workersRoutes" as const, zone: zone.name }]
      : []),
    ...(details.protection === "login" ? [{ kind: "access" as const }] : []),
  ];
}
