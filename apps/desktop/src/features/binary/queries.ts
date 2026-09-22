import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Channel } from "@tauri-apps/api/core";
import { useState } from "react";
import { commands, type InstallProgress } from "@/lib/ipc/bindings";
import { call } from "@/lib/ipc/client";
import { queryKeys } from "@/lib/ipc/query-keys";

/** The cloudflared in use, or `null` when none is installed. */
export function useBinaryStatus() {
  return useQuery({
    queryKey: queryKeys.binary.status(),
    queryFn: () => call(commands.binaryStatus()),
  });
}

/** Asks GitHub whether a newer cloudflared exists (only when the user asks, or once a day). */
export function useCheckUpdate(enabled: boolean) {
  return useQuery({
    queryKey: queryKeys.binary.update(),
    queryFn: () => call(commands.binaryCheckUpdate()),
    enabled,
    staleTime: 24 * 60 * 60 * 1000,
    retry: false,
  });
}

export function useRevealBinary() {
  return useMutation({ mutationFn: () => call(commands.binaryReveal()) });
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
    onSuccess: (info) => {
      queryClient.setQueryData(queryKeys.binary.status(), info);
      void queryClient.invalidateQueries({ queryKey: queryKeys.binary.update() });
    },
    onSettled: () => setProgress(null),
  });
  return { ...mutation, progress };
}
