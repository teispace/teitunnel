import type { Status } from "@/components/ui/status-dot";
import { t } from "@/lib/i18n";
import type { Availability, Hold } from "@/lib/ipc/bindings";

/** A lease's end in the user's time zone (leases are kept in UTC). */
export function formatUntil(ms: number): string {
  return new Date(ms).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

/** Who holds a name: `person@machine`, or "another Teitunnel" when an older version made it. */
export function ownerName(owner: string | null): string {
  return owner ?? t("reservations.availability.someone");
}

/** "Reserved by bob@pc until …", "Routed by bob@pc on another machine". */
export function holdText(hold: Hold): string {
  const owner = ownerName(hold.owner);
  if (hold.kind === "route") return t("reservations.availability.routed", { owner });
  if (hold.until === null) return t("reservations.availability.reservedForever", { owner });
  return t("reservations.availability.reserved", { owner, until: formatUntil(hold.until) });
}

/** The dot and line under a hostname field; `null` when there's nothing to say. */
export function availabilityStatus(availability: Availability): {
  dot: Status;
  label: string;
} | null {
  switch (availability.state) {
    case "free":
      return { dot: "healthy", label: t("reservations.availability.free") };
    case "yours":
      return { dot: "healthy", label: t("reservations.availability.yours") };
    case "held":
      return { dot: "warning", label: holdText(availability.hold) };
    case "foreign":
      return { dot: "warning", label: t("reservations.availability.foreign") };
    case "noZone":
      return null;
  }
}

const HOSTNAME = /^(\*\.)?([a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z][a-z0-9-]*[a-z0-9]$/;

/** Worth asking Cloudflare about: a whole hostname, not something half typed. */
export function checkable(hostname: string): boolean {
  return HOSTNAME.test(hostname.trim().toLowerCase());
}
