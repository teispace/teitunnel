import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Channel } from "@tauri-apps/api/core";
import { useState } from "react";
import { type Change, commands, type Progress, type StepState } from "@/lib/ipc/bindings";
import { call } from "@/lib/ipc/client";
import { queryKeys } from "@/lib/ipc/query-keys";

export function useRoutesOverview(accountId: string | null) {
  return useQuery({
    queryKey: queryKeys.routes.overview(accountId ?? ""),
    queryFn: () => call(commands.routesOverview(accountId ?? "")),
    enabled: accountId !== null,
    staleTime: 10_000,
    refetchOnWindowFocus: true,
    // Connector state changes on its own; keep it fresh while the view is open.
    refetchInterval: 5_000,
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

export function usePreview(accountId: string) {
  return useMutation({
    mutationFn: (change: Change) => call(commands.routesPreview(accountId, change)),
  });
}

export interface ApplyVars {
  change: Change;
  fingerprint: string;
  confirmed: boolean;
}

/** Applies a reviewed change and tracks each step's state as it streams in. */
export function useApply(accountId: string) {
  const queryClient = useQueryClient();
  const [steps, setSteps] = useState<Record<number, StepState>>({});
  const mutation = useMutation({
    mutationFn: ({ change, fingerprint, confirmed }: ApplyVars) => {
      setSteps({});
      const channel = new Channel<Progress>();
      channel.onmessage = (progress) =>
        setSteps((current) => ({ ...current, [progress.step]: progress.state }));
      return call(commands.routesApply(accountId, change, fingerprint, confirmed, channel));
    },
    onSettled: () => void queryClient.invalidateQueries({ queryKey: queryKeys.routes.all() }),
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
    onSettled: () => void queryClient.invalidateQueries({ queryKey: queryKeys.routes.all() }),
  });
}

/** Previews and applies a change in one go (Undo, where the user already decided). */
export async function applyDirectly(accountId: string, change: Change) {
  const plan = await call(commands.routesPreview(accountId, change));
  return call(
    commands.routesApply(accountId, change, plan.fingerprint, false, new Channel<Progress>()),
  );
}

export function useTunnels(accountId: string | null) {
  return useQuery({
    queryKey: queryKeys.routes.tunnels(accountId ?? ""),
    queryFn: () => call(commands.tunnelsList(accountId ?? "")),
    enabled: accountId !== null,
    staleTime: 10_000,
    refetchInterval: 10_000,
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
    onSettled: () => void queryClient.invalidateQueries({ queryKey: queryKeys.routes.all() }),
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
      void queryClient.invalidateQueries({ queryKey: queryKeys.routes.foreign() });
      void queryClient.invalidateQueries({ queryKey: queryKeys.doctor.all() });
    },
  });
}

/** This Mac's connector's newest log lines, polled while shown. */
export function useTunnelLogs(tunnelId: string, enabled: boolean) {
  return useQuery({
    queryKey: ["routes", "logs", tunnelId],
    queryFn: () => call(commands.tunnelsLogs(tunnelId, 1000)),
    enabled,
    refetchInterval: 2000,
    staleTime: 0,
  });
}

/** This Mac's connector traffic for a tunnel, refreshed with each 10 s sample. */
export function useTraffic(tunnelId: string, enabled: boolean) {
  return useQuery({
    queryKey: ["routes", "traffic", tunnelId],
    queryFn: () => call(commands.tunnelsTraffic(tunnelId)),
    enabled,
    refetchInterval: 10_000,
    staleTime: 0,
  });
}

/** Whether this Mac's connector runs as a service (keeps running after quit). */
export function useAlwaysOn(accountId: string) {
  return useQuery({
    queryKey: ["routes", "alwaysOn", accountId],
    queryFn: () => call(commands.tunnelsAlwaysOn(accountId)),
  });
}

export function useSetAlwaysOn(accountId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (enabled: boolean) => call(commands.tunnelsSetAlwaysOn(accountId, enabled)),
    onSettled: () => void queryClient.invalidateQueries({ queryKey: queryKeys.routes.all() }),
  });
}
