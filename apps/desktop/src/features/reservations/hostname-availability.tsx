import { StatusDot } from "@/components/ui/status-dot";
import { t } from "@/lib/i18n";
import { availabilityStatus, checkable } from "./format";
import { useHostnameAvailability } from "./queries";

interface HostnameAvailabilityProps {
  accountId: string;
  hostname: string;
}

/**
 * Under a hostname field: whether the name is free, yours, or held by a teammate
 * (their reservation or their machine's route), checked as you type.
 */
export function HostnameAvailability({ accountId, hostname }: HostnameAvailabilityProps) {
  const availability = useHostnameAvailability(accountId, hostname);
  if (!checkable(hostname)) return null;
  const status = availability.data ? availabilityStatus(availability.data) : null;
  return (
    <p aria-live="polite" className="flex min-h-5 items-center gap-1.5 text-callout text-secondary">
      {availability.checking && !status ? (
        <span>{t("reservations.availability.checking")}</span>
      ) : status ? (
        <>
          <StatusDot status={status.dot} />
          <span>{status.label}</span>
        </>
      ) : null}
    </p>
  );
}
