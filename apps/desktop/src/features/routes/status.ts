import type { Status } from "@/components/ui/status-dot";
import type { ConnectorState, Issue, RouteView, TunnelView } from "@/lib/ipc/bindings";

export function connectorStatus(state: ConnectorState | null): { dot: Status; label: string } {
  switch (state?.state) {
    case "healthy":
      return { dot: "healthy", label: "Connected" };
    case "starting":
    case "connecting":
      return { dot: "connecting", label: "Connecting" };
    case "degraded":
      return { dot: "warning", label: "Connection lost" };
    case "crashed":
      return { dot: "connecting", label: "Restarting" };
    case "crashLoop":
      return { dot: "error", label: "Keeps stopping" };
    default:
      return { dot: "idle", label: "Connector stopped" };
  }
}

/** One dot and label per route: the worst of its DNS, this Mac's connector and any
 * Doctor issue about its hostname (e.g. nothing listening on the origin's port). */
export function routeStatus(
  route: RouteView,
  tunnel: TunnelView | null,
  issues: readonly Issue[] = [],
): { dot: Status; label: string } {
  if (route.dns.state === "missing") return { dot: "warning", label: "No DNS record" };
  if (route.dns.state === "elsewhere") return { dot: "warning", label: "DNS points elsewhere" };
  const connector = connectorStatus(tunnel?.connector ?? null);
  if (connector.dot !== "healthy") return connector;
  const issue = issues.find((i) => i.subject === route.hostname && i.severity !== "info");
  if (issue) return { dot: issue.severity === "error" ? "error" : "warning", label: issue.title };
  return { dot: "healthy", label: "Live" };
}
