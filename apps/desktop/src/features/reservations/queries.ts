import { useQuery } from "@tanstack/react-query";
import { commands } from "@/lib/ipc/bindings";
import { call } from "@/lib/ipc/client";
import { queryKeys } from "@/lib/ipc/query-keys";
import { useDebounced } from "@/lib/use-debounced";
import { checkable } from "./format";

/** The account's reserved hostnames (DNS is the truth; the cache answers offline). */
export function useReservations(accountId: string | null) {
  return useQuery({
    queryKey: queryKeys.routes.reservations(accountId ?? ""),
    queryFn: () => call(commands.reservationsList(accountId ?? "")),
    enabled: accountId !== null,
    staleTime: 30_000,
    refetchOnWindowFocus: true,
  });
}

/** Whether a hostname is free, yours or someone else's, checked once typing pauses. */
export function useHostnameAvailability(accountId: string | null, hostname: string) {
  const settled = useDebounced(hostname.trim().toLowerCase(), 400);
  const query = useQuery({
    queryKey: queryKeys.routes.availability(accountId ?? "", settled),
    queryFn: () => call(commands.reservationsAvailability(accountId ?? "", settled)),
    enabled: accountId !== null && checkable(settled),
    staleTime: 15_000,
    retry: false,
  });
  return {
    ...query,
    /** Still typing, or asking Cloudflare. */
    checking: settled !== hostname.trim().toLowerCase() || query.isFetching,
  };
}
