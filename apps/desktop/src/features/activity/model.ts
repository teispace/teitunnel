import { type MessageKey, t } from "@/lib/i18n";
import type { ActivityEntry, ActivityKind, StepView } from "@/lib/ipc/bindings";

/** What the list shows: everything, only problems, or one kind of change. */
export type Show = "all" | "problems" | ActivityKind;

const kindLabels: Record<ActivityKind, MessageKey> = {
  addRoute: "activity.kind.addRoute",
  updateRoute: "activity.kind.updateRoute",
  removeRoute: "activity.kind.removeRoute",
  importRoutes: "activity.kind.importRoutes",
  deleteRecord: "activity.kind.deleteRecord",
  restoreConfig: "activity.kind.restoreConfig",
  removeTunnel: "activity.kind.removeTunnel",
  removeLogin: "activity.kind.removeLogin",
  addNetwork: "activity.kind.addNetwork",
  removeNetwork: "activity.kind.removeNetwork",
};

/** The Show menu (built on use: labels need the language). */
export const showOptions = (): { value: Show; label: string }[] => [
  { value: "all", label: t("activity.show.all") },
  { value: "problems", label: t("activity.show.problems") },
  ...(Object.entries(kindLabels) as [ActivityKind, MessageKey][]).map(([value, label]) => ({
    value,
    label: t(label),
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
