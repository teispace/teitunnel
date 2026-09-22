import { queryOptions, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { commands, type SettingsPatch } from "@/lib/ipc/bindings";
import { call } from "@/lib/ipc/client";
import { queryKeys } from "@/lib/ipc/query-keys";

export const settingsQuery = queryOptions({
  queryKey: queryKeys.settings.all(),
  queryFn: () => call(commands.settingsGet()),
});

export function useSettings() {
  return useQuery(settingsQuery);
}

/** Saves a settings change. Other windows update via the `EntityChanged` event. */
export function useUpdateSettings() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (patch: SettingsPatch) => call(commands.settingsSet(patch)),
    onSuccess: (settings) => queryClient.setQueryData(settingsQuery.queryKey, settings),
  });
}
