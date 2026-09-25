import { useMutation } from "@tanstack/react-query";

/**
 * A Refresh button's state: busy while a refresh it started runs, and not during the
 * background polling that keeps the view current.
 */
export function useManualRefetch(refetch: () => Promise<unknown>) {
  const { mutate, isPending } = useMutation({ mutationFn: () => refetch() });
  return { refresh: () => mutate(), refreshing: isPending };
}
