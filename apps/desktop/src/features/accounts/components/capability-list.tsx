import { Check, Minus, X } from "lucide-react";
import { Spinner } from "@/components/ui/spinner";
import { cn } from "@/lib/cn";
import { t } from "@/lib/i18n";
import type { Grant } from "@/lib/ipc/bindings";
import { useCapabilities } from "../queries";

function GrantIcon({ grant }: { grant: Grant }) {
  const Icon = grant === "yes" ? Check : grant === "no" ? X : Minus;
  return (
    <Icon
      aria-label={
        grant === "yes"
          ? t("capabilities.allowed")
          : grant === "no"
            ? t("capabilities.notAllowed")
            : t("capabilities.unknown")
      }
      className={cn(
        "size-3.5 shrink-0",
        grant === "yes" ? "text-healthy" : grant === "no" ? "text-error" : "text-tertiary",
      )}
      strokeWidth={2.5}
    />
  );
}

function Row({ grant, label, hint }: { grant: Grant; label: string; hint?: string }) {
  return (
    <li className="flex items-start gap-2 py-0.5 text-callout">
      <span className="mt-px">
        <GrantIcon grant={grant} />
      </span>
      <span className="min-w-0">
        {label}
        {grant === "no" && hint ? <span className="block text-secondary">{hint}</span> : null}
      </span>
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
  return (
    <ul className="flex flex-col">
      {zoneId === undefined ? (
        <Row
          grant={caps.zonesRead}
          label={t("capabilities.zones")}
          hint={t("capabilities.zonesHint")}
        />
      ) : null}
      <Row
        grant={caps.tunnelsEdit}
        label={t("capabilities.tunnels")}
        hint={t("capabilities.tunnelsHint")}
      />
      {caps.zones
        .filter((zone) => zoneId === undefined || zone.zoneId === zoneId)
        .map((zone) => (
          <Row
            key={zone.zoneId}
            grant={zone.dnsEdit}
            label={t("capabilities.dns", { zone: zone.zoneName })}
            hint={t("capabilities.dnsHint")}
          />
        ))}
      {zoneId === undefined ? (
        <Row
          grant={caps.accessEdit}
          label={t("capabilities.access")}
          hint={t("capabilities.accessHint")}
        />
      ) : null}
    </ul>
  );
}
