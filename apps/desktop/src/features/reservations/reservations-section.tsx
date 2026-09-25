import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { InspectorSection } from "@/components/patterns/inspector";
import { Button } from "@/components/ui/button";
import { applyDirectly } from "@/features/routes/queries";
import { t, translate } from "@/lib/i18n";
import type { Domain, Reservation } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { queryKeys, refresh } from "@/lib/ipc/query-keys";
import { formatUntil, ownerName } from "./format";
import { useReservations } from "./queries";
import { ReserveSheet } from "./reserve-sheet";

/** Whether `hostname` is `domain` or under it. */
function inDomain(hostname: string, domain: string) {
  return hostname === domain || hostname.endsWith(`.${domain}`);
}

function detail(reservation: Reservation): string {
  const who = reservation.mine ? t("reservations.yours") : ownerName(reservation.owner);
  const when =
    reservation.until === null
      ? t("reservations.noEnd")
      : reservation.ended
        ? t("reservations.ended", { date: formatUntil(reservation.until) })
        : t("reservations.endsOn", { date: formatUntil(reservation.until) });
  const parts = [who, when];
  if (reservation.routed) parts.push(t("reservations.routed"));
  return parts.join(" · ");
}

/** A domain's reserved hostnames and who holds them, with Reserve and Release. */
export function ReservationsSection({ accountId, domain }: { accountId: string; domain: Domain }) {
  const reservations = useReservations(accountId);
  const queryClient = useQueryClient();
  const [reserving, setReserving] = useState(false);
  const release = useMutation({
    mutationFn: (hostname: string) =>
      applyDirectly(accountId, { type: "releaseHostname", hostname }),
    onSuccess: (outcome, hostname) => {
      if (outcome.type === "applied") toast.success(t("reservations.released", { hostname }));
      else toast.error(t("reservations.failed"), { description: translate(outcome.error) });
    },
    onError: (err) =>
      toast.error(t("reservations.failed"), { description: toIpcError(err).message }),
    onSettled: () => refresh(queryClient, queryKeys.routes.all()),
  });
  const items = (reservations.data?.items ?? []).filter((r) => inDomain(r.hostname, domain.name));

  return (
    <InspectorSection title={t("reservations.title")}>
      <p className="text-callout text-secondary">{t("reservations.detail")}</p>
      {reservations.data?.cached ? (
        <p className="text-callout text-secondary">{t("reservations.cachedNote")}</p>
      ) : null}
      {reservations.error ? (
        <div role="alert" className="flex flex-col text-callout text-error">
          <span>{t("reservations.loadFailed")}</span>
          <span>{toIpcError(reservations.error).message}</span>
        </div>
      ) : items.length === 0 ? (
        <p className="text-body text-secondary">
          {reservations.isPending
            ? t("reservations.availability.checking")
            : t("reservations.none")}
        </p>
      ) : (
        <ul aria-label={t("reservations.list")} className="flex flex-col">
          {items.map((reservation) => (
            <li
              key={reservation.hostname}
              className="flex items-center gap-2 border-separator border-b-hairline py-1.5 last:border-b-0"
            >
              <div className="flex min-w-0 flex-1 flex-col">
                <span className="truncate font-mono text-mono">{reservation.hostname}</span>
                <span className="truncate text-callout text-secondary">{detail(reservation)}</span>
              </div>
              {reservation.mine ? (
                <Button
                  size="sm"
                  aria-label={t("reservations.releaseLabel", { hostname: reservation.hostname })}
                  disabled={release.isPending}
                  onClick={() => release.mutate(reservation.hostname)}
                >
                  {t("reservations.release")}
                </Button>
              ) : null}
            </li>
          ))}
        </ul>
      )}
      <div>
        <Button onClick={() => setReserving(true)} disabled={domain.status !== "active"}>
          {t("reservations.reserve")}
        </Button>
      </div>
      {reserving ? (
        <ReserveSheet
          accountId={accountId}
          zones={[{ id: domain.id, name: domain.name }]}
          open
          onClose={() => setReserving(false)}
        />
      ) : null}
    </InspectorSection>
  );
}
