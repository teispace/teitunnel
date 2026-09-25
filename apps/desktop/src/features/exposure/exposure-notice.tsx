import { Spinner } from "@/components/ui/spinner";
import { t } from "@/lib/i18n";
import { ExposureCallout } from "./exposure-callout";
import { findingsOf, useExposure } from "./queries";

/**
 * The exposure check next to a route's plan: shown while it runs, then only when it
 * found something. Applying is the "anyway"; it never blocks.
 */
export function ExposureNotice({ origin }: { origin: string }) {
  const check = useExposure(origin);
  if (check.isPending) {
    return (
      <p className="flex items-center gap-2 text-callout text-secondary" role="status">
        <Spinner className="size-3" /> {t("exposure.checking")}
      </p>
    );
  }
  const report = findingsOf(check.data);
  return report ? <ExposureCallout report={report} kind="route" /> : null;
}
