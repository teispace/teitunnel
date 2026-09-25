import {
  keepPreviousData,
  queryOptions,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { useState } from "react";
import {
  type AccessRule,
  commands,
  type FolderShare,
  type HostHeaderChoice,
  type QuickShare,
  type Schedule,
  type Verification,
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
  /** Through the inspector (`null`: Settings ▸ Inspector decides). */
  inspect?: boolean | null;
}

const AUTO: HostHeaderChoice = { mode: "auto" };

export function useStartShare() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      origin,
      stopAfterMinutes,
      hostHeader = AUTO,
      inspect = null,
    }: StartShareInput) =>
      call(commands.quickShareStart(origin, stopAfterMinutes, hostHeader, inspect)),
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
  /** The service's project folder: fills in `{project}`, `{branch}`, and remembers the name. */
  folder?: string | null;
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
      folder = null,
    }: DomainShareVars) =>
      call(
        commands.domainSharesStart(
          accountId,
          hostname,
          origin,
          stopAfterMinutes,
          access ?? null,
          hostHeader,
          folder,
        ),
      ),
    onSettled: () => refresh(queryClient, queryKeys.quickShares.domain()),
  });
}

/** Asks for a folder to share (a native panel); `null` when cancelled. */
export const chooseFolder = () => call(commands.sharingChooseFolder());

/** Checks a chosen or dropped folder (it must exist, and not be the disk or home folder). */
export const resolveFolder = (path: string, listing: boolean | null = null, spa = false) =>
  call(commands.sharingFolder(path, listing, spa));

/** Shares a folder at a random address; the URL arrives like any Quick Share's. */
export function useStartFolderShare() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      folder,
      stopAfterMinutes,
    }: {
      folder: FolderShare;
      stopAfterMinutes: number | null;
    }) => call(commands.quickShareStartFolder(folder, stopAfterMinutes)),
    onSuccess: (share) =>
      queryClient.setQueryData<QuickShare[]>(quickSharesQuery.queryKey, (shares = []) => [
        share,
        ...shares.filter((existing) => existing.id !== share.id),
      ]),
  });
}

/** Shares a folder at a hostname on one of the account's domains. */
export function useStartDomainFolderShare() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      accountId,
      hostname,
      folder,
      stopAfterMinutes,
    }: {
      accountId: string;
      hostname: string;
      folder: FolderShare;
      stopAfterMinutes: number | null;
    }) =>
      call(
        commands.sharingStartFolderOnDomain(accountId, hostname, folder, stopAfterMinutes, null),
      ),
    onSettled: () => refresh(queryClient, queryKeys.quickShares.domain()),
  });
}

/** Pauses (the paused page) or resumes a share on your domain; the address stays. */
/** Pauses or resumes a Quick Share (same address; visitors see the paused page). */
export function useSetQuickSharePaused() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, paused }: { id: string; paused: boolean }) =>
      call(commands.quickShareSetPaused(id, paused)),
    onSuccess: (share) =>
      queryClient.setQueryData<QuickShare[]>(quickSharesQuery.queryKey, (shares = []) =>
        shares.map((existing) => (existing.id === share.id ? share : existing)),
      ),
  });
}

export function useSetSharePaused() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      accountId,
      hostname,
      paused,
    }: {
      accountId: string;
      hostname: string;
      paused: boolean;
    }) => call(commands.sharingSetPaused(accountId, hostname, paused)),
    onSettled: () => refresh(queryClient, queryKeys.quickShares.domain()),
  });
}

/** Schedules of shares on your domains (and routes), with when each changes next. */
export function useSchedules() {
  return useQuery({
    queryKey: queryKeys.quickShares.schedules(),
    queryFn: () => call(commands.sharingSchedules()),
    // "Starts at 9:00" goes stale as time passes.
    refetchInterval: 60_000,
  });
}

/** Sets (or, with `null`, removes) a share's schedule. */
export function useSetSchedule() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      accountId,
      hostname,
      schedule,
    }: {
      accountId: string;
      hostname: string;
      schedule: Schedule | null;
    }) => call(commands.sharingSetSchedule(accountId, hostname, schedule)),
    onSettled: () =>
      refresh(queryClient, queryKeys.quickShares.schedules(), queryKeys.quickShares.domain()),
  });
}

/** Names to offer on `domain` for a service in `folder` (or known by its `project`). */
export function useNameSuggestions(
  domain: string | null,
  folder: string | null,
  project: string | null,
) {
  return useQuery({
    queryKey: queryKeys.quickShares.names(domain ?? "", folder, project),
    queryFn: () => call(commands.sharingNameSuggestions(domain ?? "", folder, project)),
    enabled: domain !== null,
    staleTime: 30_000,
  });
}

/** What a hostname with `{project}`, `{branch}` or `{user}` becomes (only for templates). */
export function useExpandedName(hostname: string, folder: string | null) {
  return useQuery({
    queryKey: queryKeys.quickShares.expanded(hostname, folder),
    queryFn: () => call(commands.sharingExpandName(hostname, folder)),
    enabled: hostname.includes("{"),
    retry: false,
    staleTime: 30_000,
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
export function useDomainShareCheck(accountId: string, hostname: string, startedAt: number | null) {
  // Unknown start (an older share): count from when the card appeared.
  const [shown] = useState(Date.now);
  const since = startedAt ?? shown;
  const settling = (check: Verification | undefined) =>
    Boolean(check?.transient) && Date.now() - since < SETTLE_MS;
  const query = useQuery({
    queryKey: queryKeys.domainShareCheck(accountId, hostname),
    queryFn: () => call(commands.routesVerify(accountId, hostname, true)),
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
    // Cloudflare can take a while to connect a new record: check again while it settles.
    refetchInterval: (q) => (settling(q.state.data) ? 5000 : false),
  });
  return { ...query, settling: settling(query.data) };
}

/** How long a new share's transient failures read as "still connecting". */
const SETTLE_MS = 3 * 60_000;

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
