import { useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { InspectorSection } from "@/components/patterns/inspector";
import { Button } from "@/components/ui/button";
import { useIssues } from "@/features/doctor/queries";
import { t } from "@/lib/i18n";
import type { InspectedRoute, Issue, TapView } from "@/lib/ipc/bindings";
import { type InspectRouteTarget, useInspectedRoutes, useTaps } from "../queries";
import { InspectRouteSheet } from "./inspect-route-sheet";

const samePath = (a: string | null, b: string | null) => (a ?? "") === (b ?? "");

/** The remembered inspection of a route, if it's pointed at an inspector. */
export function findInspected(
  routes: readonly InspectedRoute[],
  accountId: string,
  hostname: string,
  path: string | null,
) {
  return routes.find(
    (r) => r.accountId === accountId && r.hostname === hostname && samePath(r.path, path),
  );
}

/** This app's running tap for a route, if any. */
export function findRouteTap(
  taps: readonly TapView[],
  accountId: string,
  hostname: string,
  path: string | null,
) {
  return taps.find(
    (tap) =>
      tap.scope.kind === "route" &&
      tap.scope.accountId === accountId &&
      tap.scope.hostname === hostname &&
      samePath(tap.scope.path, path),
  );
}

/** Whether the Doctor says a route points at an inspector that isn't running. */
export const isOrphan = (issues: readonly Issue[], hostname: string) =>
  issues.some((issue) => issue.check === "inspect.orphan" && issue.subject === hostname);

interface InspectRouteSectionProps {
  accountId: string;
  hostname: string;
  path: string | null;
  /** Served by this computer (only those can be inspected). */
  local: boolean;
}

/**
 * A route's requests, in its inspector pane: Inspect this route (a reviewed plan), then
 * Show Requests and Stop Inspecting; Restore the Route when an inspector left it behind.
 */
export function InspectRouteSection({
  accountId,
  hostname,
  path,
  local,
}: InspectRouteSectionProps) {
  const [target, setTarget] = useState<InspectRouteTarget | null>(null);
  const navigate = useNavigate();
  const routes = useInspectedRoutes();
  const taps = useTaps();
  const { issues } = useIssues();
  const inspected = findInspected(routes.data ?? [], accountId, hostname, path);
  const tap = findRouteTap(taps.data ?? [], accountId, hostname, path);
  const orphan = inspected !== undefined && !tap && isOrphan(issues, hostname);

  return (
    <InspectorSection title={t("inspector.route.section")}>
      <p className="text-callout text-secondary">
        {orphan
          ? t("inspector.route.orphan")
          : inspected
            ? t("inspector.route.inspecting")
            : local
              ? t("inspector.route.help")
              : t("inspector.route.notLocal")}
      </p>
      <div className="flex flex-wrap gap-2">
        {inspected ? (
          <>
            {tap ? (
              <Button
                size="sm"
                onClick={() => void navigate({ to: "/inspector", search: { tap: tap.id } })}
              >
                {t("inspector.route.show")}
              </Button>
            ) : null}
            <Button size="sm" onClick={() => setTarget({ accountId, hostname, path, on: false })}>
              {orphan ? t("inspector.route.restore") : t("inspector.route.stop")}
            </Button>
          </>
        ) : (
          <Button
            size="sm"
            disabled={!local || routes.isPending}
            onClick={() => setTarget({ accountId, hostname, path, on: true })}
          >
            {t("inspector.route.inspect")}
          </Button>
        )}
      </div>
      <InspectRouteSheet target={target} restore={orphan} onClose={() => setTarget(null)} />
    </InspectorSection>
  );
}
