import { type MessageKey, t, translate } from "@/lib/i18n";
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
  cleanUpHostname: "activity.kind.cleanUpHostname",
  addNetwork: "activity.kind.addNetwork",
  removeNetwork: "activity.kind.removeNetwork",
  createTunnel: "activity.kind.createTunnel",
  balanceRoute: "activity.kind.balanceRoute",
  unbalanceRoute: "activity.kind.unbalanceRoute",
  alert: "activity.kind.alert",
  publishSnapshot: "activity.kind.publishSnapshot",
  updateSnapshot: "activity.kind.updateSnapshot",
  rollbackSnapshot: "activity.kind.rollbackSnapshot",
  deleteSnapshot: "activity.kind.deleteSnapshot",
  reserveHostname: "activity.kind.reserveHostname",
  releaseHostname: "activity.kind.releaseHostname",
  protectHostname: "activity.kind.protectHostname",
  createServiceToken: "activity.kind.createServiceToken",
  revokeServiceToken: "activity.kind.revokeServiceToken",
  rotateServiceToken: "activity.kind.rotateServiceToken",
  offlinePage: "activity.kind.offlinePage",
  webhookInbox: "activity.kind.webhookInbox",
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

/** What was asked, in the user's language (older entries: as stored, in English). */
export function summaryOf(entry: ActivityEntry): string {
  return entry.record?.summary ? translate(entry.record.summary) : entry.summary;
}

/** What a failed undo left in place (older entries: from the stored lines). */
export function leftoversOf(entry: ActivityEntry): string[] {
  const leftovers = entry.record?.leftovers;
  if (leftovers?.length) return leftovers.map(translate);
  return entry.detail.filter((line) => line.startsWith("Left over"));
}

/** Hostnames an entry touched (older entries only have their summary). */
function hostnamesOf(entry: ActivityEntry): readonly string[] {
  return entry.record?.hostnames ?? [];
}

export function matches(entry: ActivityEntry, { show, zone, query }: Filter): boolean {
  // Problems: changes that failed, and alerts (not their resolutions).
  if (show === "problems" && (entry.outcome === "applied" || entry.outcome === "resolved")) {
    return false;
  }
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
  const steps = entry.record?.steps.map((s) => translate(s.step.description)) ?? [];
  // English too, so a search in English still finds entries in another language.
  return [summaryOf(entry), entry.summary, ...steps, ...entry.detail, ...hostnamesOf(entry)].some(
    (text) => text.toLowerCase().includes(query),
  );
}

/** The steps' commands as one pasteable script, each under its description. */
export function commandScript(steps: readonly StepView[]): string | null {
  const blocks = steps
    .filter((step) => step.command)
    .map((step) => `# ${translate(step.description)}\n${step.command}`);
  return blocks.length > 0 ? `${blocks.join("\n\n")}\n` : null;
}
