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

const loginKey = ["settings", "openAtLogin"] as const;

/** Whether Teitunnel opens at login (a system login item, not an app setting). */
export function useOpenAtLogin() {
  return useQuery({ queryKey: loginKey, queryFn: () => call(commands.appOpenAtLogin()) });
}

export function useSetOpenAtLogin() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (enabled: boolean) => call(commands.appSetOpenAtLogin(enabled)),
    onSettled: () => void queryClient.invalidateQueries({ queryKey: loginKey }),
  });
}
