import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  type Browser,
  commands,
  type IntegrationsPatch,
  type McpSettings,
} from "@/lib/ipc/bindings";
import { call } from "@/lib/ipc/client";
import { queryKeys } from "@/lib/ipc/query-keys";

const aiClientsKey = ["integrations", "aiClients"] as const;

/**
 * AI apps on this computer: installed, connected, last used. Checked again when the
 * window comes back into focus (the person may have just installed one).
 */
export function useAiClients() {
  return useQuery({
    queryKey: aiClientsKey,
    queryFn: () => call(commands.aiClientsStatus()),
    refetchOnWindowFocus: true,
  });
}

/** Connects (or updates) or disconnects an AI app: only Teitunnel's entry changes. */
export function useSetAiClientConnected() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, connect }: { id: string; connect: boolean }) =>
      call(connect ? commands.aiClientsConnect(id) : commands.aiClientsDisconnect(id)),
    onSuccess: (view) => queryClient.setQueryData(aiClientsKey, view),
  });
}

/** Starts the app's configured server and checks it answers. */
export function useTestAiClient() {
  return useMutation({
    mutationFn: (id: string) => call(commands.aiClientsTest(id)),
  });
}

/** Shows an AI app's settings file in the file manager. */
export function useRevealAiClient() {
  return useMutation({
    mutationFn: (id: string) => call(commands.aiClientsReveal(id)),
  });
}

/** AI agents connected through `teitunnel mcp` now, and their approvals waiting. */
export function useAiAgents() {
  return useQuery({
    queryKey: queryKeys.agents.all(),
    queryFn: () => call(commands.aiAgents()),
  });
}

/**
 * Apps signed in with OAuth to MCP servers shared from this computer. A connection
 * appears once the app finishes signing in, which no event announces: checked again
 * every 15 s while shown.
 */
export function useMcpConnections() {
  return useQuery({
    queryKey: queryKeys.agents.mcp(),
    queryFn: () => call(commands.mcpConnections()),
    refetchInterval: 15_000,
  });
}

/** Disconnects an app from a shared MCP server. */
export function useMcpDisconnect() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => call(commands.mcpDisconnect(id)),
    onSettled: () => queryClient.invalidateQueries({ queryKey: queryKeys.agents.mcp() }),
  });
}

const browserHostKey = ["integrations", "browserHost"] as const;

/**
 * Browsers on this computer, whether each can start the extension's helper, and when
 * the extension last connected (checked again on focus: it's set up in the browser).
 */
export function useBrowserHost() {
  return useQuery({
    queryKey: browserHostKey,
    queryFn: () => call(commands.browserHostStatus()),
    refetchOnWindowFocus: true,
  });
}

/** Sets the helper up (or removes it) in one browser, or all when `browser` is null. */
export function useSetBrowserHost() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ install, browser }: { install: boolean; browser: Browser | null }) =>
      call(install ? commands.browserHostInstall(browser) : commands.browserHostUninstall(browser)),
    onSuccess: (view) => queryClient.setQueryData(browserHostKey, view),
  });
}

const integrationsKey = ["integrations", "control"] as const;

/** The control connection, links and always-allowed programs. */
export function useIntegrations() {
  return useQuery({ queryKey: integrationsKey, queryFn: () => call(commands.integrationsGet()) });
}

/** Turns the control connection or links on or off, or changes the global shortcut. */
export function useUpdateIntegrations() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (patch: IntegrationsPatch) => call(commands.integrationsSet(patch)),
    onSuccess: (value) => queryClient.setQueryData(integrationsKey, value),
  });
}

/** Stops always allowing a program. */
export function useRevokeClient() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (name: string) => call(commands.integrationsRevoke(name)),
    onSuccess: (value) => queryClient.setQueryData(integrationsKey, value),
  });
}

const mcpSettingsKey = ["integrations", "mcpSettings"] as const;

/** What AI agents may do, and the OAuth policy of shared MCP servers (`<data>/mcp.json`). */
export function useMcpSettings() {
  return useQuery({ queryKey: mcpSettingsKey, queryFn: () => call(commands.mcpSettingsGet()) });
}

/** Saves a change to them; AI apps use it the next time they start the server. */
export function useSaveMcpSettings() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (settings: McpSettings) => call(commands.mcpSettingsSave(settings)),
    onSuccess: (saved) => queryClient.setQueryData(mcpSettingsKey, saved),
  });
}
