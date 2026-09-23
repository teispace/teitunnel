import {
  keepPreviousData,
  queryOptions,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { type AccessRule, commands, type QuickShare } from "@/lib/ipc/bindings";
import { call } from "@/lib/ipc/client";
import { queryKeys } from "@/lib/ipc/query-keys";

export const quickSharesQuery = queryOptions({
  queryKey: queryKeys.quickShares.all(),
  queryFn: () => call(commands.quickShareList()),
});

export function useQuickShares() {
  return useQuery(quickSharesQuery);
}

/** Live traffic numbers, polled every 2 s while the card is on screen. */
export function useShareStats(id: string, enabled: boolean) {
  return useQuery({
    queryKey: queryKeys.quickShares.stats(id),
    queryFn: () => call(commands.quickShareStats(id)),
    enabled,
    refetchInterval: 2000,
    placeholderData: keepPreviousData,
    staleTime: 0,
  });
}

/** The newest log lines, polled while the log is open. */
export function useShareLogs(id: string, enabled: boolean) {
  return useQuery({
    queryKey: queryKeys.quickShares.logs(id),
    queryFn: () => call(commands.quickShareLogs(id, 200)),
    enabled,
    refetchInterval: 1000,
    staleTime: 0,
  });
}

export function useQrCode(url: string, enabled: boolean) {
  return useQuery({
    queryKey: queryKeys.qr(url),
    queryFn: () => call(commands.quickShareQr(url)),
    enabled,
  });
}

/** Listening services; refreshed every 5 s only while a picker is open. */
/**
 * Services listening on this Mac. Scanned once when a picker mounts, so the list is
 * ready the moment it opens, then every 5 s only while it's open (`watching`).
 */
export function useLocalServices(watching: boolean) {
  return useQuery({
    queryKey: queryKeys.services.all(),
    queryFn: () => call(commands.servicesList()),
    refetchInterval: watching ? 5000 : false,
    staleTime: 5000,
  });
}

export interface StartShareInput {
  origin: string;
  stopAfterMinutes: number | null;
}

export function useStartShare() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ origin, stopAfterMinutes }: StartShareInput) =>
      call(commands.quickShareStart(origin, stopAfterMinutes)),
    onSuccess: (share) =>
      queryClient.setQueryData<QuickShare[]>(quickSharesQuery.queryKey, (shares = []) => [
        share,
        ...shares.filter((existing) => existing.id !== share.id),
      ]),
  });
}

export function useStopShare() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => call(commands.quickShareStop(id)),
    onMutate: (id) =>
      queryClient.setQueryData<QuickShare[]>(quickSharesQuery.queryKey, (shares = []) =>
        shares.filter((share) => share.id !== id),
      ),
    onSettled: () => queryClient.invalidateQueries({ queryKey: quickSharesQuery.queryKey }),
  });
}

/** Shares on your own domains (temporary routes), oldest first. */
export function useDomainShares() {
  return useQuery({
    queryKey: queryKeys.quickShares.domain(),
    queryFn: () => call(commands.domainSharesList()),
  });
}

export interface DomainShareVars {
  accountId: string;
  hostname: string;
  origin: string;
  stopAfterMinutes: number | null;
  access?: AccessRule | null;
}

/** Shares a service at a hostname on one of the account's domains. */
export function useStartDomainShare() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ accountId, hostname, origin, stopAfterMinutes, access }: DomainShareVars) =>
      call(
        commands.domainSharesStart(accountId, hostname, origin, stopAfterMinutes, access ?? null),
      ),
    onSettled: () =>
      void queryClient.invalidateQueries({ queryKey: queryKeys.quickShares.domain() }),
  });
}

export function useStopDomainShare() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ accountId, hostname }: { accountId: string; hostname: string }) =>
      call(commands.domainSharesStop(accountId, hostname)),
    onSettled: () =>
      void queryClient.invalidateQueries({ queryKey: queryKeys.quickShares.domain() }),
  });
}
