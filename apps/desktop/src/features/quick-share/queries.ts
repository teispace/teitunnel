import {
  keepPreviousData,
  queryOptions,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { Channel } from "@tauri-apps/api/core";
import { useState } from "react";
import { commands, type InstallProgress, type QuickShare } from "@/lib/ipc/bindings";
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
export function useLocalServices(enabled: boolean) {
  return useQuery({
    queryKey: queryKeys.services.all(),
    queryFn: () => call(commands.servicesList()),
    enabled,
    refetchInterval: enabled ? 5000 : false,
    staleTime: 0,
  });
}

export function useBinaryStatus() {
  return useQuery({
    queryKey: queryKeys.binary.status(),
    queryFn: () => call(commands.binaryStatus()),
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

/** Installs the managed cloudflared, exposing live progress. */
export function useInstallBinary() {
  const queryClient = useQueryClient();
  const [progress, setProgress] = useState<InstallProgress | null>(null);
  const mutation = useMutation({
    mutationFn: () => {
      const channel = new Channel<InstallProgress>();
      channel.onmessage = setProgress;
      return call(commands.binaryInstall(channel));
    },
    onSuccess: (info) => queryClient.setQueryData(queryKeys.binary.status(), info),
    onSettled: () => setProgress(null),
  });
  return { ...mutation, progress };
}
