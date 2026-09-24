import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Channel } from "@tauri-apps/api/core";
import { type Change, commands, type Progress, type RouteView } from "@/lib/ipc/bindings";
import { call } from "@/lib/ipc/client";
import { queryKeys, refresh } from "@/lib/ipc/query-keys";

/** A route of the account, from the routes overview (shared with the Routes screen). */
export function useRoute(accountId: string | null, hostname: string, path: string | null = null) {
  return useQuery({
    queryKey: queryKeys.routes.overview(accountId ?? ""),
    queryFn: () => call(commands.routesOverview(accountId ?? "")),
    enabled: accountId !== null,
    staleTime: 10_000,
    select: (overview) =>
      overview.routes.find((r) => r.hostname === hostname && r.path === path) ?? null,
  });
}

/** The route, changed only to send `host` as its Host header (everything else kept). */
export function withHostHeader(route: RouteView, host: string): Change {
  return {
    type: "updateRoute",
    hostname: route.hostname,
    path: route.path,
    route: {
      hostname: route.hostname,
      path: route.path,
      origin: route.origin,
      access: route.access,
      options: { ...route.options, httpHostHeader: host },
    },
  };
}

/**
 * Makes a route send the Host header its dev server expects: a plan for that one
 * setting, applied through the engine like any change (the click is the approval).
 */
export function useSendHostOnRoute(accountId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async ({ route, host }: { route: RouteView; host: string }) => {
      const change = withHostHeader(route, host);
      const plan = await call(commands.routesPreview(accountId, route.tunnelId, change));
      return call(
        commands.routesApply(
          accountId,
          route.tunnelId,
          change,
          plan.fingerprint,
          false,
          new Channel<Progress>(),
        ),
      );
    },
    onSettled: () => refresh(queryClient, queryKeys.routes.all()),
  });
}
