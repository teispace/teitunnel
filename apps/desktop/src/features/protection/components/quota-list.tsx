import { ProgressBar } from "@/components/ui/progress-bar";
import { cn } from "@/lib/cn";
import { t } from "@/lib/i18n";
import type { QuotaView } from "@/lib/ipc/bindings";
import { quotaLabels } from "../model";

/** How much of each of the zone's rule quotas is used (Teitunnel's rules and others'). */
export function QuotaList({ quotas, zone }: { quotas: readonly QuotaView[]; zone: string }) {
  return (
    <section aria-label={t("protection.quota.title", { zone })} className="flex flex-col gap-1.5">
      <h4 className="text-callout text-secondary">{t("protection.quota.title", { zone })}</h4>
      <ul className="flex flex-col gap-1.5">
        {quotas.map((quota) => {
          const label = t(quotaLabels[quota.quota]);
          const full = quota.used >= quota.limit;
          return (
            <li key={quota.quota} className="flex items-center gap-2 text-callout">
              <span className="w-32 shrink-0">{label}</span>
              <ProgressBar
                value={quota.limit === 0 ? 1 : quota.used / quota.limit}
                label={t("protection.quota.used", {
                  label,
                  used: quota.used,
                  limit: quota.limit,
                })}
                className="flex-1"
              />
              <span className={cn("w-12 shrink-0 text-right tabular", full && "text-warning")}>
                {t("protection.quota.count", { used: quota.used, limit: quota.limit })}
              </span>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
