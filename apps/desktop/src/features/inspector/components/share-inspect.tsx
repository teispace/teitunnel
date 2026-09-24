import { useNavigate } from "@tanstack/react-router";
import { ScanSearch } from "lucide-react";
import { useId, useState } from "react";
import { ConfirmDialog } from "@/components/patterns/confirm-dialog";
import { IconButton } from "@/components/ui/icon-button";
import { Switch } from "@/components/ui/switch";
import { Tooltip } from "@/components/ui/tooltip";
import { t } from "@/lib/i18n";
import type { QuickShare } from "@/lib/ipc/bindings";
import {
  type InspectRouteTarget,
  useInspectedRoutes,
  useSetShareInspected,
  useTaps,
} from "../queries";
import { findInspected, findRouteTap } from "./inspect-route-section";
import { InspectRouteSheet } from "./inspect-route-sheet";

/** "Inspect requests" on a Quick Share: its requests in the Inspector (its tap has its id). */
export function InspectShareButton({ share }: { share: QuickShare }) {
  const navigate = useNavigate();
  return (
    <Tooltip content={t("inspector.share.inspect")}>
      <IconButton
        icon={ScanSearch}
        label={t("inspector.share.inspect")}
        variant="secondary"
        size="lg"
        disabled={!share.inspected}
        onClick={() => void navigate({ to: "/inspector", search: { tap: share.id } })}
      />
    </Tooltip>
  );
}

/**
 * The switch that sends a running Quick Share through the inspector or straight to its
 * service. cloudflared restarts either way, so it asks first: the address changes.
 */
export function ShareInspectSwitch({ share }: { share: QuickShare }) {
  const id = useId();
  const change = useSetShareInspected();
  const [asking, setAsking] = useState(false);
  const next = !share.inspected;
  return (
    <span className="flex items-center gap-2">
      <Switch
        id={id}
        checked={change.isPending ? next : share.inspected}
        disabled={change.isPending || share.status.status === "failed"}
        onCheckedChange={() => setAsking(true)}
      />
      <ConfirmDialog
        open={asking}
        onOpenChange={setAsking}
        title={next ? t("inspector.share.onTitle") : t("inspector.share.offTitle")}
        description={t("inspector.share.newAddress")}
        confirmLabel={next ? t("inspector.share.confirmOn") : t("inspector.share.confirmOff")}
        onConfirm={() => change.mutateAsync({ id: share.id, inspect: next })}
      />
      <label htmlFor={id}>{t("inspector.share.toggle")}</label>
    </span>
  );
}

/**
 * "Inspect requests" on a share on your domain: its requests when it's inspected, else
 * the plan that points it at the inspector (a share on your domain is a route).
 */
export function InspectDomainShareButton({
  accountId,
  hostname,
}: {
  accountId: string;
  hostname: string;
}) {
  const navigate = useNavigate();
  const [target, setTarget] = useState<InspectRouteTarget | null>(null);
  const routes = useInspectedRoutes();
  const taps = useTaps();
  const tap = findRouteTap(taps.data ?? [], accountId, hostname, null);
  const inspected = findInspected(routes.data ?? [], accountId, hostname, null);
  return (
    <>
      <Tooltip content={t("inspector.share.inspect")}>
        <IconButton
          icon={ScanSearch}
          label={t("inspector.share.inspect")}
          variant="secondary"
          size="lg"
          disabled={routes.isPending}
          onClick={() =>
            tap
              ? void navigate({ to: "/inspector", search: { tap: tap.id } })
              : inspected
                ? void navigate({ to: "/inspector", search: { host: hostname } })
                : setTarget({ accountId, hostname, path: null, on: true })
          }
        />
      </Tooltip>
      <InspectRouteSheet target={target} onClose={() => setTarget(null)} />
    </>
  );
}
