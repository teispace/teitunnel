import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { type AlertRules, type AnalyticsRange, commands } from "@/lib/ipc/bindings";
import { call } from "@/lib/ipc/client";
import { queryKeys, refresh } from "@/lib/ipc/query-keys";

/**
 * How often a visible view refreshes each range. The backend caches Cloudflare's answers
 * for as long (D-046-style polling: TanStack pauses it while the window is hidden).
 */
export const REFRESH_MS: Record<AnalyticsRange, number> = {
  hour: 60_000,
  day: 5 * 60_000,
  week: 15 * 60_000,
  month: 30 * 60_000,
};

/** Every hostname's edge traffic side by side. */
export function useAnalyticsSummary(
  accountId: string | null,
  hostnames: readonly string[],
  range: AnalyticsRange,
) {
  return useQuery({
    queryKey: queryKeys.analytics.summary(accountId ?? "", range, hostnames),
    queryFn: () => call(commands.analyticsSummary(accountId ?? "", [...hostnames], range)),
    enabled: accountId !== null && hostnames.length > 0,
    staleTime: REFRESH_MS[range],
    refetchInterval: REFRESH_MS[range],
    placeholderData: keepPreviousData,
    retry: false,
  });
}

/** One route's edge traffic in detail. */
export function useRouteAnalytics(
  accountId: string,
  hostname: string,
  path: string | null,
  range: AnalyticsRange,
) {
  return useQuery({
    queryKey: queryKeys.analytics.route(accountId, hostname, path, range),
    queryFn: () => call(commands.analyticsRoute(accountId, hostname, path, range)),
    staleTime: REFRESH_MS[range],
    refetchInterval: REFRESH_MS[range],
    placeholderData: keepPreviousData,
    retry: false,
  });
}

/** Uptime of every route this Mac serves (checks run once a minute). */
export function useUptimeList() {
  return useQuery({
    queryKey: queryKeys.uptime.list(),
    queryFn: () => call(commands.uptimeList()),
    staleTime: 30_000,
    refetchInterval: 60_000,
  });
}

/** One route's uptime strip, response times and incidents (null: not served here). */
export function useUptimeRoute(hostname: string, path: string | null, range: AnalyticsRange) {
  return useQuery({
    queryKey: queryKeys.uptime.route(hostname, path, range),
    queryFn: () => call(commands.uptimeRoute(hostname, path, range)),
    staleTime: 30_000,
    refetchInterval: 60_000,
    placeholderData: keepPreviousData,
  });
}

export function useAlertRules() {
  return useQuery({
    queryKey: queryKeys.alerts.rules(),
    queryFn: () => call(commands.alertsGet()),
  });
}

/** Saves the alert rules; the switch or menu shows the new value right away. */
export function useSetAlertRules() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (rules: AlertRules) => call(commands.alertsSet(rules)),
    onMutate: (rules) => queryClient.setQueryData(queryKeys.alerts.rules(), rules),
    onSettled: () => refresh(queryClient, queryKeys.alerts.rules()),
  });
}
