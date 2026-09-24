import { currentLanguage, t } from "@/lib/i18n";
import type { SnapshotSource, SnapshotView } from "@/lib/ipc/bindings";

/** A size for people: `980 bytes`, `12.4 KB`, `3.1 MB` (decimal units, like Finder). */
export function formatBytes(bytes: number): string {
  if (bytes < 1000) return t("snapshots.bytes", { count: bytes });
  const units = ["kilobyte", "megabyte", "gigabyte", "terabyte"] as const;
  let value = bytes / 1000;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit += 1;
  }
  return new Intl.NumberFormat(currentLanguage(), {
    style: "unit",
    unit: units[unit],
    unitDisplay: "short",
    maximumFractionDigits: 1,
  }).format(value);
}

/** Where a Snapshot's files come from, in a few words. */
export function sourceLabel(source: SnapshotSource | null): string {
  if (!source) return "—";
  switch (source.type) {
    case "folder":
      return source.path;
    case "build":
      return t("snapshots.source.build", { command: source.command, project: source.project });
    case "crawl":
      return t("snapshots.source.crawl", { url: source.url });
  }
}

/** Who can open it. */
export function protectionLabel(snapshot: SnapshotView): string {
  if (snapshot.access) {
    const people = [...snapshot.access.emails, ...snapshot.access.emailDomains.map((d) => `@${d}`)];
    return t("snapshots.protection.loginFor", { people: people.join(", ") });
  }
  return snapshot.password ? t("snapshots.protection.password") : t("snapshots.protection.none");
}

/** A local site to capture, from a share's origin: `3000` → `http://localhost:3000`. */
export function siteUrl(origin: string): string {
  const trimmed = origin.trim();
  if (/^\d+$/.test(trimmed)) return `http://localhost:${trimmed}`;
  if (/^[a-z]+:\/\//i.test(trimmed)) return trimmed;
  return `http://${trimmed}`;
}
