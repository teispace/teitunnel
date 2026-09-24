import type { Status } from "@/components/ui/status-dot";
import { routeIssues } from "@/features/doctor/match";
import { ownerName } from "@/features/reservations/format";
import { t, translate } from "@/lib/i18n";
import type { ConnectorState, Issue, RouteView, TunnelView } from "@/lib/ipc/bindings";

export function connectorStatus(state: ConnectorState | null): { dot: Status; label: string } {
  switch (state?.state) {
    case "healthy":
      return { dot: "healthy", label: t("status.connected") };
    case "starting":
    case "connecting":
      return { dot: "connecting", label: t("status.connecting") };
    case "degraded":
      return { dot: "warning", label: t("status.connectionLost") };
    case "crashed":
      return { dot: "connecting", label: t("status.restarting") };
    case "crashLoop":
      return { dot: "error", label: t("status.keepsStopping") };
    default:
      return { dot: "idle", label: t("status.stopped") };
  }
}

/** One dot and label per route: the worst of its DNS, this Mac's connector and any
 * Doctor issue about its hostname (e.g. nothing listening on the origin's port). */
export function routeStatus(
  route: RouteView,
  tunnel: TunnelView | null,
  issues: readonly Issue[] = [],
): { dot: Status; label: string } {
  if (route.dns.state === "missing") return { dot: "warning", label: t("status.noDns") };
  if (route.dns.state === "elsewhere")
    return {
      dot: "warning",
      label: route.dns.heldBy
        ? t("status.dnsHeldBy", { owner: ownerName(route.dns.heldBy.owner) })
        : t("status.dnsElsewhere"),
    };
  const connector = connectorStatus(tunnel?.connector ?? null);
  if (connector.dot !== "healthy") return connector;
  const [issue] = routeIssues(issues, route.hostname);
  if (issue)
    return { dot: issue.severity === "error" ? "error" : "warning", label: translate(issue.title) };
  return { dot: "healthy", label: t("status.live") };
}
