import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { commands } from "@/lib/ipc/bindings";
import { call } from "@/lib/ipc/client";
import { queryKeys } from "@/lib/ipc/query-keys";

/** Where app updates stand; refreshed by `EntityChanged { kind: "updates" }`. */
export function useUpdateStatus() {
  return useQuery({
    queryKey: queryKeys.updates.status(),
    queryFn: () => call(commands.updatesStatus()),
  });
}

/** Checks now (and downloads what it finds), even with automatic checks off. */
export function useCheckForUpdates() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => call(commands.updatesCheck()),
    onSuccess: (status) => queryClient.setQueryData(queryKeys.updates.status(), status),
  });
}

/** Quits, installs the downloaded update and opens the new version. */
export function useRestartToUpdate() {
  return useMutation({ mutationFn: () => commands.updatesRestart() });
}
