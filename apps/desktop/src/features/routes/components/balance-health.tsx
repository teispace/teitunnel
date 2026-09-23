import { InspectorSection } from "@/components/patterns/inspector";
import { type Status, StatusDot } from "@/components/ui/status-dot";
import { t } from "@/lib/i18n";
import type { EndpointHealth } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { useBalanceHealth } from "../queries";

function describe(endpoint: EndpointHealth): { status: Status; label: string } {
  const { healthyRegions: healthy, regions } = endpoint;
  if (!endpoint.enabled) return { status: "idle", label: t("routes.health.disabled") };
  if (regions === 0) return { status: "connecting", label: t("routes.health.pending") };
  if (healthy === regions)
    return { status: "healthy", label: t("routes.health.healthy", { count: regions }) };
  if (healthy === 0)
    return {
      status: "error",
      label: endpoint.reason
        ? t("routes.health.failingBecause", { reason: endpoint.reason })
        : t("routes.health.failing"),
    };
  return { status: "warning", label: t("routes.health.partly", { healthy, count: regions }) };
}

/**
 * Each machine behind a load-balanced route, as Cloudflare's health checks see it from
 * its regions. Refreshed every 30 s while shown (the checks run about once a minute).
 */
export function BalanceHealth({
  accountId,
  hostname,
  localTunnelIds,
}: {
  accountId: string;
  hostname: string;
  /** This machine's tunnels, to mark its endpoint. */
  localTunnelIds: string[];
}) {
  const health = useBalanceHealth(accountId, hostname);
  return (
    <InspectorSection title={t("routes.inspector.machines")}>
      {health.error ? (
        <p className="text-callout text-secondary">{toIpcError(health.error).message}</p>
      ) : health.data && health.data.length === 0 ? (
        <p className="text-callout text-secondary">{t("routes.health.none")}</p>
      ) : (
        <ul className="flex flex-col gap-2" aria-busy={health.isPending}>
          {(health.data ?? []).map((endpoint) => {
            const { status, label } = describe(endpoint);
            const local = localTunnelIds.includes(endpoint.tunnelId);
            return (
              <li key={endpoint.tunnelId} className="flex items-start gap-2 text-callout">
                <StatusDot status={status} label={label} className="mt-1" />
                <span className="flex min-w-0 flex-col">
                  <span className="truncate">
                    {local ? t("routes.health.local", { name: endpoint.name }) : endpoint.name}
                  </span>
                  <span className="text-secondary">{label}</span>
                </span>
              </li>
            );
          })}
        </ul>
      )}
    </InspectorSection>
  );
}
