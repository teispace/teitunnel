import { TriangleAlert } from "lucide-react";
import { Button } from "@/components/ui/button";
import { t } from "@/lib/i18n";
import type { RuleChange } from "@/lib/ipc/bindings";
import { useDrift, useKeepTheirs } from "../queries";

function describe(change: RuleChange) {
  const service = (s: string) => s.replace(/^http:\/\/localhost:/, "localhost:");
  if (change.before === null)
    return t("drift.added", { hostname: change.hostname, service: service(change.after ?? "") });
  if (change.after === null) return t("drift.removed", { hostname: change.hostname });
  if (change.before === change.after) return t("drift.settings", { hostname: change.hostname });
  return t("drift.moved", { hostname: change.hostname, service: service(change.after) });
}

/**
 * Shown when this Mac's routes were edited outside Teitunnel (dashboard, another Mac):
 * keep the edit, or put back what Teitunnel set up.
 */
export function DriftBanner({
  accountId,
  onRestore,
}: {
  accountId: string;
  onRestore: () => void;
}) {
  const drift = useDrift(accountId);
  const keep = useKeepTheirs(accountId);
  if (!drift.data) return null;
  const changes = drift.data.changes;
  return (
    <div
      role="status"
      className="mx-3 mt-2 flex gap-2.5 rounded-card bg-warning/10 px-3 py-2.5 text-callout"
    >
      <TriangleAlert
        aria-hidden
        className="mt-0.5 size-4 shrink-0 text-warning"
        strokeWidth={1.75}
      />
      <div className="min-w-0 flex-1">
        <p className="text-headline">{t("drift.title")}</p>
        <ul className="mt-0.5 text-secondary">
          {changes.slice(0, 3).map((change) => (
            <li key={`${change.hostname}${change.path ?? ""}`} className="truncate">
              {describe(change)}
            </li>
          ))}
          {changes.length > 3 ? <li>{t("drift.more", { count: changes.length - 3 })}</li> : null}
        </ul>
        <div className="mt-2 flex gap-2">
          <Button size="sm" disabled={keep.isPending} onClick={() => keep.mutate()}>
            {t("drift.keep")}
          </Button>
          <Button size="sm" onClick={onRestore}>
            {t("drift.restore")}
          </Button>
        </div>
      </div>
    </div>
  );
}
