import { queryOptions, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { useUiStore } from "@/app/ui-store";
import { settingsQuery, useSettings } from "@/features/settings/queries";
import { commands, type Issue } from "@/lib/ipc/bindings";
import { call } from "@/lib/ipc/client";
import { queryKeys, refresh } from "@/lib/ipc/query-keys";

export const doctorQuery = queryOptions({
  queryKey: queryKeys.doctor.all(),
  queryFn: () => call(commands.doctorRun()),
  staleTime: 60_000,
  // Every 5 minutes while the app is open (ARCHITECTURE: Doctor scheduling).
  refetchInterval: 5 * 60_000,
  refetchOnWindowFocus: true,
});

const RANK: Record<Issue["severity"], number> = { error: 0, warning: 1, info: 2 };

/**
 * Problems first, then warnings, then notes, keeping each source's order within a
 * severity: the app adds local-domain findings after the Cloudflare ones, and the list
 * groups by severity, so an unsorted list showed a group twice.
 */
export function bySeverity(issues: readonly Issue[]): Issue[] {
  return issues
    .map((issue, index) => ({ issue, index }))
    .sort((a, b) => RANK[a.issue.severity] - RANK[b.issue.severity] || a.index - b.index)
    .map(({ issue }) => issue);
}

/**
 * Issues, minus the ones the user ignored (kept in settings, so background Doctor runs
 * don't notify about them either).
 */
export function useIssues() {
  const settings = useSettings().data;
  const ignored = settings?.ignoredIssues ?? [];
  useMoveLocalIgnores(settings !== undefined);
  const query = useQuery(doctorQuery);
  const all: Issue[] = query.data ?? [];
  const visible = bySeverity(all.filter((issue) => !ignored.includes(issue.id)));
  const hidden = all.filter((issue) => ignored.includes(issue.id)).map((issue) => issue.id);
  return { ...query, issues: visible, ignoredCount: hidden.length, ignoredIds: hidden };
}

/** Ignores (or stops ignoring) issues by id. */
export function useSetIgnored() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ ids, ignored }: { ids: string[]; ignored: boolean }) =>
      call(commands.doctorSetIgnored(ids, ignored)),
    onSuccess: (settings) => queryClient.setQueryData(settingsQuery.queryKey, settings),
  });
}

/**
 * Before ignores moved to settings they lived in this window's local storage; move any
 * left there once, then forget them.
 */
function useMoveLocalIgnores(ready: boolean) {
  const legacy = useUiStore((state) => state.legacyIgnoredIssues);
  const clear = useUiStore((state) => state.clearLegacyIgnoredIssues);
  const { mutate, isPending } = useSetIgnored();
  useEffect(() => {
    if (!ready || legacy.length === 0 || isPending) return;
    mutate({ ids: legacy, ignored: true }, { onSuccess: clear });
  }, [ready, legacy, isPending, mutate, clear]);
}

/** Fixes that may run without review (the backend re-checks each with a fresh plan). */
export function hasSafeCandidates(issues: readonly Issue[]) {
  return issues.some((issue) => {
    const fix = issue.fixes[0];
    return (
      fix?.type === "change" &&
      (fix.change.type === "addRoute" || fix.change.type === "deleteRecord")
    );
  });
}

export function useFixSafe() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => call(commands.doctorFixSafe()),
    onSettled: () => refresh(queryClient, queryKeys.doctor.all(), queryKeys.routes.all()),
  });
}
