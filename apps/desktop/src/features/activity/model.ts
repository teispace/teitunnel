import type { ActivityEntry, ActivityKind, StepView } from "@/lib/ipc/bindings";

/** What the list shows: everything, only problems, or one kind of change. */
export type Show = "all" | "problems" | ActivityKind;

const kindLabels: Record<ActivityKind, string> = {
  addRoute: "Routes Added",
  updateRoute: "Routes Changed",
  removeRoute: "Routes Removed",
  importRoutes: "Imports",
  deleteRecord: "DNS Cleanups",
  restoreConfig: "Restores",
  removeTunnel: "Tunnel Removals",
  removeLogin: "Login Removals",
};

export const showOptions: readonly { value: Show; label: string }[] = [
  { value: "all", label: "All Changes" },
  { value: "problems", label: "Only Problems" },
  ...(Object.entries(kindLabels) as [ActivityKind, string][]).map(([value, label]) => ({
    value,
    label,
  })),
];

export interface Filter {
  show: Show;
  /** A domain (zone apex) the entry must touch, or null for any. */
  zone: string | null;
  /** Lower-case text to find in the summary, steps or hostnames. */
  query: string;
}

/** Whether `hostname` is `zone` or inside it. */
export function inZone(hostname: string, zone: string): boolean {
  return hostname === zone || hostname.endsWith(`.${zone}`);
}

/** Hostnames an entry touched (older entries only have their summary). */
function hostnamesOf(entry: ActivityEntry): readonly string[] {
  return entry.record?.hostnames ?? [];
}

export function matches(entry: ActivityEntry, { show, zone, query }: Filter): boolean {
  if (show === "problems" && entry.outcome === "applied") return false;
  if (show !== "all" && show !== "problems" && entry.record?.kind !== show) return false;
  if (zone !== null) {
    const hostnames = hostnamesOf(entry);
    const touches =
      hostnames.length > 0
        ? hostnames.some((h) => inZone(h, zone))
        : entry.summary.toLowerCase().includes(zone);
    if (!touches) return false;
  }
  if (!query) return true;
  return [entry.summary, ...entry.detail, ...hostnamesOf(entry)].some((text) =>
    text.toLowerCase().includes(query),
  );
}

/** The steps' commands as one pasteable script, each under its description. */
export function commandScript(steps: readonly StepView[]): string | null {
  const blocks = steps
    .filter((step) => step.command)
    .map((step) => `# ${step.description}\n${step.command}`);
  return blocks.length > 0 ? `${blocks.join("\n\n")}\n` : null;
}
