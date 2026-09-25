import { Check, Minus, X } from "lucide-react";
import { Spinner } from "@/components/ui/spinner";
import { cn } from "@/lib/cn";
import { t } from "@/lib/i18n";
import type { Grant } from "@/lib/ipc/bindings";
import { useCapabilities } from "../queries";
import { PermissionFix, type PermissionNeed } from "./permission-fix";

function GrantIcon({ grant }: { grant: Grant }) {
  const Icon = grant === "yes" ? Check : grant === "no" ? X : Minus;
  const label = {
    yes: t("capabilities.allowed"),
    no: t("capabilities.notAllowed"),
    unknown: t("capabilities.unknown"),
    notSetUp: t("capabilities.notSetUp"),
  }[grant];
  const tone = {
    yes: "text-healthy",
    no: "text-error",
    unknown: "text-tertiary",
    notSetUp: "text-warning",
  }[grant];
  return <Icon aria-label={label} className={cn("size-3.5 shrink-0", tone)} strokeWidth={2.5} />;
}

function Row({ grant, label }: { grant: Grant; label: string }) {
  return (
    <li className="flex items-start gap-2 py-0.5 text-callout">
      <span className="mt-px">
        <GrantIcon grant={grant} />
      </span>
      <span className="min-w-0">{label}</span>
    </li>
  );
}

/** What a credential can do, checked live against Cloudflare (read-only probes). */
export function CapabilityList({ accountId, zoneId }: { accountId: string; zoneId?: string }) {
  const { data: caps, isPending, error } = useCapabilities(accountId);
  if (isPending) {
    return (
      <div className="flex items-center gap-2 py-1 text-callout text-secondary">
        <Spinner className="size-3.5" /> {t("capabilities.checking")}
      </div>
    );
  }
  if (error || !caps)
    return <p className="text-callout text-secondary">{t("capabilities.failed")}</p>;
  const zone = caps.zones.find((z) => z.zoneId === zoneId)?.zoneName;
  // What to add, as a fix: only what a check found missing is shown.
  const needs: PermissionNeed[] =
    zoneId === undefined
      ? [
          { kind: "zones" },
          { kind: "tunnels" },
          ...caps.zones.map((z) => ({ kind: "dns" as const, zone: z.zoneName })),
          { kind: "access" },
          { kind: "workers" },
        ]
      : [{ kind: "tunnels" }, ...(zone ? [{ kind: "dns" as const, zone }] : [])];
  return (
    <div className="flex flex-col gap-3">
      <ul className="flex flex-col">
        {zoneId === undefined ? (
          <Row grant={caps.zonesRead} label={t("capabilities.zones")} />
        ) : null}
        <Row grant={caps.tunnelsEdit} label={t("capabilities.tunnels")} />
        {caps.zones
          .filter((zone) => zoneId === undefined || zone.zoneId === zoneId)
          .map((zone) => (
            <Row
              key={zone.zoneId}
              grant={zone.dnsEdit}
              label={t("capabilities.dns", { zone: zone.zoneName })}
            />
          ))}
        {zoneId === undefined ? (
          <>
            <Row grant={caps.accessEdit} label={t("capabilities.access")} />
            <Row grant={caps.workersEdit} label={t("capabilities.workers")} />
            <Row grant={caps.edgeRules} label={t("capabilities.edgeRules")} />
            <Row grant={caps.serviceTokens} label={t("capabilities.serviceTokens")} />
            <Row grant={caps.d1} label={t("capabilities.d1")} />
          </>
        ) : null}
      </ul>
      <PermissionFix accountId={accountId} needs={needs} />
    </div>
  );
}
