import { QueryClient } from "@tanstack/react-query";

/**
 * Local IPC data is invalidated by `EntityChanged` events, so it never goes stale by
 * time and is never refetched on focus. Network-backed queries opt into shorter
 * `staleTime` individually.
 */
export function createQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: {
        staleTime: Number.POSITIVE_INFINITY,
        refetchOnWindowFocus: false,
        refetchOnReconnect: false,
        retry: false,
      },
      mutations: { retry: false },
    },
  });
}
