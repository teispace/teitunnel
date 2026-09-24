import { queryOptions, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { commands, type IntegrationsPatch, type SettingsPatch } from "@/lib/ipc/bindings";
import { call } from "@/lib/ipc/client";
import { queryKeys, refresh } from "@/lib/ipc/query-keys";

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
    onSettled: () => refresh(queryClient, loginKey),
  });
}

const cliKey = ["settings", "cli"] as const;

/** Whether `teitunnel` is on the PATH (D-077). */
export function useCliStatus() {
  return useQuery({ queryKey: cliKey, queryFn: () => call(commands.cliStatus()) });
}

const aiClientsKey = ["settings", "aiClients"] as const;

/** AI tools on this computer and whether each is connected to Teitunnel's MCP server. */
export function useAiClients() {
  return useQuery({ queryKey: aiClientsKey, queryFn: () => call(commands.aiClientsStatus()) });
}

/** Connects or disconnects an AI tool (edits only Teitunnel's entry in its settings). */
export function useSetAiClientConnected() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, connect }: { id: string; connect: boolean }) =>
      call(connect ? commands.aiClientsConnect(id) : commands.aiClientsDisconnect(id)),
    onSuccess: (view) => queryClient.setQueryData(aiClientsKey, view),
  });
}

const integrationsKey = ["settings", "integrations"] as const;

/** Settings ▸ Integrations: the control connection, links and always-allowed programs. */
export function useIntegrations() {
  return useQuery({ queryKey: integrationsKey, queryFn: () => call(commands.integrationsGet()) });
}

/** Turns the control connection or links on or off. */
export function useUpdateIntegrations() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (patch: IntegrationsPatch) => call(commands.integrationsSet(patch)),
    onSuccess: (value) => queryClient.setQueryData(integrationsKey, value),
  });
}

/** Stops always allowing a program; stays busy until the list no longer has it. */
export function useRevokeClient() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (name: string) => call(commands.integrationsRevoke(name)),
    onSuccess: (value) => queryClient.setQueryData(integrationsKey, value),
  });
}

/** Installs or removes the command line tool. */
export function useSetCliInstalled() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (install: boolean) =>
      call(install ? commands.cliInstall() : commands.cliUninstall()),
    onSuccess: (state) => queryClient.setQueryData(cliKey, state),
  });
}
