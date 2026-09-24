import {
  keepPreviousData,
  queryOptions,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import {
  type AccessRule,
  commands,
  type HostHeaderChoice,
  type QuickShare,
} from "@/lib/ipc/bindings";
import { call } from "@/lib/ipc/client";
import { queryKeys, refresh } from "@/lib/ipc/query-keys";

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
  hostHeader?: HostHeaderChoice;
}

const AUTO: HostHeaderChoice = { mode: "auto" };

export function useStartShare() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ origin, stopAfterMinutes, hostHeader = AUTO }: StartShareInput) =>
      call(commands.quickShareStart(origin, stopAfterMinutes, hostHeader, null)),
    onSuccess: (share) =>
      queryClient.setQueryData<QuickShare[]>(quickSharesQuery.queryKey, (shares = []) => [
        share,
        ...shares.filter((existing) => existing.id !== share.id),
      ]),
  });
}

/** Restarts a share sending `host` as its Host header (`null`: none); it gets a new URL. */
export function useSetShareHostHeader() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, host }: { id: string; host: string | null }) =>
      call(commands.quickShareSetHostHeader(id, host)),
    onSuccess: (share) =>
      queryClient.setQueryData<QuickShare[]>(quickSharesQuery.queryKey, (shares = []) =>
        shares.map((existing) => (existing.id === share.id ? share : existing)),
      ),
  });
}

/** Checks a live share through Cloudflare again; the result lands on the share. */
export function useCheckShare() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => call(commands.quickShareCheck(id)),
    onSettled: () => refresh(queryClient, quickSharesQuery.queryKey),
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
    onSettled: () => refresh(queryClient, quickSharesQuery.queryKey),
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
  hostHeader?: HostHeaderChoice;
}

/** Shares a service at a hostname on one of the account's domains. */
export function useStartDomainShare() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      accountId,
      hostname,
      origin,
      stopAfterMinutes,
      access,
      hostHeader = AUTO,
    }: DomainShareVars) =>
      call(
        commands.domainSharesStart(
          accountId,
          hostname,
          origin,
          stopAfterMinutes,
          access ?? null,
          hostHeader,
        ),
      ),
    onSettled: () => refresh(queryClient, queryKeys.quickShares.domain()),
  });
}

export function useStopDomainShare() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ accountId, hostname }: { accountId: string; hostname: string }) =>
      call(commands.domainSharesStop(accountId, hostname)),
    onSettled: () => refresh(queryClient, queryKeys.quickShares.domain()),
  });
}

/**
 * A share on your domain, checked once through Cloudflare (waiting for the connector and
 * propagation, like a new route). `refetch` checks again.
 */
export function useDomainShareCheck(accountId: string, hostname: string) {
  return useQuery({
    queryKey: queryKeys.domainShareCheck(accountId, hostname),
    queryFn: () => call(commands.routesVerify(accountId, hostname, true)),
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
  });
}

/** Quick Shares running in terminals (`teitunnel share`); refreshed every 5 s. */
export function useTerminalShares() {
  return useQuery({
    queryKey: [...queryKeys.quickShares.all(), "terminals"],
    queryFn: () => call(commands.quickShareCliList()),
    refetchInterval: 5000,
  });
}

export function useStopTerminalShare() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (owner: string) => call(commands.quickShareCliStop(owner)),
    onSettled: () => refresh(queryClient, queryKeys.quickShares.all()),
  });
}
