import { useState } from "react";
import { InspectorSection } from "@/components/patterns/inspector";
import { Button } from "@/components/ui/button";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { Skeleton } from "@/components/ui/skeleton";
import { PermissionFix } from "@/features/accounts";
import { t } from "@/lib/i18n";
import type { AnalyticsRange, RouteView } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { rangeOptions } from "../model";
import { useRouteAnalytics } from "../queries";
import { TrafficStats } from "./traffic-stats";
import { UptimeSection } from "./uptime-section";

/** The route inspector's Analytics: uptime from this Mac, traffic from Cloudflare. */
export function RouteAnalytics({ accountId, route }: { accountId: string; route: RouteView }) {
  const [range, setRange] = useState<AnalyticsRange>("day");
  const stats = useRouteAnalytics(accountId, route.hostname, route.path, range);
  const error = stats.error ? toIpcError(stats.error) : null;
  return (
    <InspectorSection title={t("analytics.section")}>
      <SegmentedControl
        label={t("analytics.rangeLabel")}
        size="sm"
        segments={rangeOptions()}
        value={range}
        onValueChange={setRange}
      />
      {route.local ? (
        <UptimeSection hostname={route.hostname} path={route.path} range={range} />
      ) : null}
      {error?.code === "permissionDenied" ? (
        <PermissionFix
          accountId={accountId}
          needs={[{ kind: "analytics" }]}
          refused
          onReady={() => void stats.refetch()}
        />
      ) : error ? (
        <div role="alert" className="flex items-center gap-2">
          <p className="min-w-0 flex-1 text-callout text-secondary">
            {error.code === "unavailable" ? error.message : t("analytics.loadFailed")}
          </p>
          <Button size="sm" pending={stats.isFetching} onClick={() => void stats.refetch()}>
            {t("common.tryAgain")}
          </Button>
        </div>
      ) : stats.data ? (
        <TrafficStats stats={stats.data} range={range} />
      ) : (
        <div
          role="status"
          aria-busy
          aria-label={t("analytics.section")}
          className="flex flex-col gap-2"
        >
          <Skeleton className="h-28" />
          <Skeleton className="h-4 w-2/3" />
        </div>
      )}
    </InspectorSection>
  );
}
