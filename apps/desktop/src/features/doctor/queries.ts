import { queryOptions, useQuery } from "@tanstack/react-query";
import { useUiStore } from "@/app/ui-store";
import { commands, type Issue } from "@/lib/ipc/bindings";
import { call } from "@/lib/ipc/client";
import { queryKeys } from "@/lib/ipc/query-keys";

export const doctorQuery = queryOptions({
  queryKey: queryKeys.doctor.all(),
  queryFn: () => call(commands.doctorRun()),
  staleTime: 60_000,
  // Every 5 minutes while the app is open (ARCHITECTURE: Doctor scheduling).
  refetchInterval: 5 * 60_000,
  refetchOnWindowFocus: true,
});

/** Issues, minus the ones the user ignored. */
export function useIssues() {
  const ignored = useUiStore((state) => state.ignoredIssues);
  const query = useQuery(doctorQuery);
  const all: Issue[] = query.data ?? [];
  const visible = all.filter((issue) => !ignored.includes(issue.id));
  return { ...query, issues: visible, ignoredCount: all.length - visible.length };
}
