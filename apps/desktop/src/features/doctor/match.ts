import type { Issue, TunnelSummary } from "@/lib/ipc/bindings";

const shown = (issue: Issue) => issue.severity !== "info";

/** Doctor issues about a route's hostname (errors and warnings). */
export function routeIssues(issues: readonly Issue[], hostname: string): Issue[] {
  return issues.filter((i) => shown(i) && i.subject === hostname && !i.check.startsWith("tunnel."));
}

/**
 * Doctor issues about a tunnel: the `tunnel.*` checks. One of this machine's other
 * tunnels is named by id; the default tunnel's issues carry none.
 */
export function tunnelIssues(
  issues: readonly Issue[],
  accountId: string,
  tunnel: Pick<TunnelSummary, "id" | "thisMac" | "isDefault">,
): Issue[] {
  return issues.filter(
    (i) =>
      shown(i) &&
      i.check.startsWith("tunnel.") &&
      i.accountId === accountId &&
      (i.tunnelId === null ? tunnel.thisMac && tunnel.isDefault : i.tunnelId === tunnel.id),
  );
}
