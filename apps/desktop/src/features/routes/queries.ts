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
