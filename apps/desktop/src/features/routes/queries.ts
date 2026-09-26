import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Channel } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import {
  type Change,
  commands,
  type HistoryRange,
  type Progress,
  type StepState,
  type Traffic,
} from "@/lib/ipc/bindings";
import { call, toIpcError } from "@/lib/ipc/client";
import { queryKeys, refresh } from "@/lib/ipc/query-keys";
import { appendSeries } from "@/lib/traffic";

/** How often views that read Cloudflare refresh on their own while open. */
const CLOUDFLARE_POLL_MS = 30_000;

export function useRoutesOverview(accountId: string | null) {
  return useQuery({
    queryKey: queryKeys.routes.overview(accountId ?? ""),
    queryFn: () => call(commands.routesOverview(accountId ?? "")),
    enabled: accountId !== null,
    staleTime: 10_000,
    refetchOnWindowFocus: true,
    // Connector changes arrive as events; this catches changes made on Cloudflare
    // elsewhere (each refresh reads the account's tunnels and DNS).
    refetchInterval: CLOUDFLARE_POLL_MS,
  });
}

export function useDrift(accountId: string | null) {
  return useQuery({
    queryKey: queryKeys.routes.drift(accountId ?? ""),
    queryFn: () => call(commands.routesDrift(accountId ?? "")),
    enabled: accountId !== null,
    staleTime: 30_000,
    refetchOnWindowFocus: true,
  });
}

export function useActivity(accountId: string | null) {
  return useQuery({
    queryKey: queryKeys.routes.activity(accountId ?? ""),
    queryFn: () => call(commands.routesActivity(accountId ?? "")),
    enabled: accountId !== null,
  });
}

export interface PreviewVars {
  change: Change;
  /** One of this Mac's tunnels; `null`: the default one. */
  tunnelId: string | null;
}

/** Plans a change on one of this Mac's tunnels. */
export function usePreview(accountId: string) {
  return useMutation({
    mutationFn: ({ change, tunnelId }: PreviewVars) =>
      call(commands.routesPreview(accountId, tunnelId, change)),
  });
}

export interface ApplyVars extends PreviewVars {
  fingerprint: string;
  confirmed: boolean;
}

/** Applies a reviewed change and tracks each step's state as it streams in. */
export function useApply(accountId: string) {
  const queryClient = useQueryClient();
  const [steps, setSteps] = useState<Record<number, StepState>>({});
  const mutation = useMutation({
    mutationFn: ({ change, tunnelId, fingerprint, confirmed }: ApplyVars) => {
      setSteps({});
      const channel = new Channel<Progress>();
      channel.onmessage = (progress) =>
        setSteps((current) => ({ ...current, [progress.step]: progress.state }));
      return call(
        commands.routesApply(accountId, tunnelId, change, fingerprint, confirmed, channel),
      );
    },
    onSettled: () => refresh(queryClient, queryKeys.routes.all()),
  });
  return { ...mutation, steps };
}

export function useVerify(accountId: string) {
  return useMutation({
    mutationFn: ({ hostname, wait }: { hostname: string; wait: boolean }) =>
      call(commands.routesVerify(accountId, hostname, wait)),
  });
}

export function useKeepTheirs(accountId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => call(commands.routesKeepTheirs(accountId)),
    onSettled: () => refresh(queryClient, queryKeys.routes.all()),
  });
}

/** Previews and applies a change in one go (Undo, where the user already decided). */
export async function applyDirectly(
  accountId: string,
  change: Change,
  tunnelId: string | null = null,
) {
  const plan = await call(commands.routesPreview(accountId, tunnelId, change));
  return call(
    commands.routesApply(
      accountId,
      tunnelId,
      change,
      plan.fingerprint,
      false,
      new Channel<Progress>(),
    ),
  );
}

export function useTunnels(accountId: string | null) {
  return useQuery({
    queryKey: queryKeys.routes.tunnels(accountId ?? ""),
    queryFn: () => call(commands.tunnelsList(accountId ?? "")),
    enabled: accountId !== null,
    staleTime: 10_000,
    refetchOnWindowFocus: true,
    refetchInterval: CLOUDFLARE_POLL_MS,
  });
}

/** Start/stop this Mac's connector, or clean a tunnel's stale connections. */
export function useTunnelAction(accountId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      action,
      tunnelId,
    }: {
      action: "start" | "stop" | "clean";
      tunnelId: string;
    }) => {
      switch (action) {
        case "start":
          return call(commands.tunnelsStart(accountId));
        case "stop":
          return call(commands.tunnelsStop(accountId, tunnelId));
        case "clean":
          return call(commands.tunnelsClean(accountId, tunnelId));
      }
    },
    onSettled: () => refresh(queryClient, queryKeys.routes.all()),
  });
}

/** cloudflared setups on this Mac that routes can be imported from. */
export function useLocalSetups(enabled: boolean) {
  return useQuery({
    queryKey: ["import", "scan"],
    queryFn: () => call(commands.importScan()),
    enabled,
    staleTime: 60_000,
  });
}

/** cloudflared processes on this Mac that Teitunnel didn't start. */
export function useForeignConnectors(enabled: boolean) {
  return useQuery({
    queryKey: queryKeys.routes.foreign(),
    queryFn: () => call(commands.foreignList()),
    enabled,
    refetchInterval: 10_000,
  });
}

export function useStopForeign() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (pid: number) => call(commands.foreignStop(pid)),
    onSettled: () => {
      // The Doctor re-runs its checks in the background; the list is what this changed.
      void queryClient.invalidateQueries({ queryKey: queryKeys.doctor.all() });
      return refresh(queryClient, queryKeys.routes.foreign());
    },
  });
}

/** This Mac's connector's newest log lines, polled while shown. */
/** Saves log lines to Downloads (redacted) and shows the file in Finder. */
export function useSaveLog() {
  const { mutate } = useMutation({
    mutationFn: (lines: string[]) => call(commands.appSaveLog(lines)),
    onSuccess: () => toast.success("Saved the log to Downloads"),
    onError: (error) => toast.error(toIpcError(error).message),
  });
  return mutate;
}

/** Log lines about one route's requests (filtered in the backend by its ingress rule). */
export function useRouteLogs(accountId: string, hostname: string, path: string | null) {
  return useQuery({
    queryKey: ["routes", "routeLogs", accountId, hostname, path],
    queryFn: () => call(commands.routesLogs(accountId, hostname, path, 500)),
    refetchInterval: 2000,
    staleTime: 0,
  });
}

/** Each machine's health behind a load-balanced route (Cloudflare checks about once a minute). */
export function useBalanceHealth(accountId: string, hostname: string) {
  return useQuery({
    queryKey: ["routes", "balanceHealth", accountId, hostname],
    queryFn: () => call(commands.routesBalanceHealth(accountId, hostname)),
    refetchInterval: 30_000,
    staleTime: 15_000,
    retry: false,
  });
}

export interface RemoteConnector {
  accountId: string;
  tunnelId: string;
  connectorId: string;
}

/**
 * A connector's live logs from another machine, relayed by Cloudflare. Polling keeps
 * the backend's stream open (it stops by itself 30 s after the last poll); unmounting
 * stops it right away.
 */
export function useRemoteLogs(target: RemoteConnector | null) {
  const query = useQuery({
    queryKey: ["routes", "remoteLogs", target],
    queryFn: () =>
      call(
        commands.tunnelsRemoteLogs(
          target?.accountId ?? "",
          target?.tunnelId ?? "",
          target?.connectorId ?? "",
          1000,
        ),
      ),
    enabled: target !== null,
    refetchInterval: (q) => (q.state.data?.state.state === "ended" ? false : 1500),
    staleTime: 0,
    gcTime: 0,
  });
  const accountId = target?.accountId;
  const tunnelId = target?.tunnelId;
  const connectorId = target?.connectorId;
  useEffect(() => {
    if (!accountId || !tunnelId || !connectorId) return;
    return () => {
      void commands.tunnelsRemoteLogsStop(accountId, tunnelId, connectorId);
    };
  }, [accountId, tunnelId, connectorId]);
  const retry = async () => {
    if (!target) return;
    await commands.tunnelsRemoteLogsStop(target.accountId, target.tunnelId, target.connectorId);
    await query.refetch();
  };
  return { ...query, retry };
}

export function useTunnelLogs(tunnelId: string, enabled: boolean) {
  return useQuery({
    queryKey: ["routes", "logs", tunnelId],
    queryFn: () => call(commands.tunnelsLogs(tunnelId, 1000)),
    enabled,
    refetchInterval: 2000,
    staleTime: 0,
  });
}

/**
 * A connector's live traffic, polled every second while shown (which also keeps the
 * backend sampling at 1 s, D-046). Each poll fetches only samples newer than the last
 * one held, and appends them.
 */
export function useLiveTraffic(tunnelId: string | null, intervalMs = 1_000) {
  const client = useQueryClient();
  const queryKey = ["routes", "traffic", tunnelId];
  return useQuery({
    queryKey,
    enabled: tunnelId !== null,
    queryFn: async (): Promise<Traffic | null> => {
      const held = client.getQueryData<Traffic | null>(queryKey);
      const since = held?.series.at.at(-1) ?? null;
      const next = await call(commands.tunnelsTraffic(tunnelId ?? "", since));
      if (!next) return null;
      return held ? { ...next, series: appendSeries(held.series, next.series) } : next;
    },
    refetchInterval: intervalMs,
    staleTime: 0,
    // Drop the hour of samples soon after the view closes.
    gcTime: 30_000,
  });
}

/** A tunnel's traffic over the last day or week, from per-minute history. */
export function useTrafficHistory(tunnelId: string, range: HistoryRange | null) {
  return useQuery({
    queryKey: ["routes", "trafficHistory", tunnelId, range],
    queryFn: () => call(commands.tunnelsTrafficHistory(tunnelId, range ?? "day")),
    enabled: range !== null,
    refetchInterval: 60_000,
    placeholderData: (previous) => previous,
  });
}

/** Whether one of this Mac's connectors runs as a service (keeps running after quit). */
export function useAlwaysOn(accountId: string, tunnelId: string | null = null) {
  return useQuery({
    queryKey: ["routes", "alwaysOn", accountId, tunnelId],
    queryFn: () => call(commands.tunnelsAlwaysOn(accountId, tunnelId)),
  });
}

export function useSetAlwaysOn(accountId: string, tunnelId: string | null = null) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (enabled: boolean) =>
      call(commands.tunnelsSetAlwaysOn(accountId, tunnelId, enabled)),
    onSettled: () => refresh(queryClient, queryKeys.routes.all()),
  });
}

/** Runs an existing tunnel of the account on this Mac too (nothing changes in Cloudflare). */
export function useAdoptTunnel(accountId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (tunnelId: string) => call(commands.tunnelsAdopt(accountId, tunnelId)),
    onSettled: () => refresh(queryClient, queryKeys.routes.all()),
  });
}
