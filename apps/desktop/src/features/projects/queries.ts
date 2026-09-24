import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { commands } from "@/lib/ipc/bindings";
import { call } from "@/lib/ipc/client";
import { queryKeys, refresh } from "@/lib/ipc/query-keys";

/** How often an open project's file is looked at for edits. */
export const WATCH_INTERVAL_MS = 2000;

/** Projects this computer knows. */
export function useProjects() {
  return useQuery({
    queryKey: queryKeys.projects.list(),
    queryFn: () => call(commands.projectsList()),
    staleTime: 30_000,
  });
}

/** A project's file read and planned (nothing changes). */
export function useProjectStatus(path: string | null) {
  return useQuery({
    queryKey: queryKeys.projects.status(path ?? ""),
    queryFn: () => call(commands.projectsStatus(path ?? "")),
    enabled: path !== null,
    staleTime: 10_000,
  });
}

/** When the project file last changed, polled while the project is open. */
export function useProjectModified(path: string | null) {
  return useQuery({
    queryKey: queryKeys.projects.modified(path ?? ""),
    queryFn: () => call(commands.projectsModified(path ?? "")),
    enabled: path !== null,
    refetchInterval: WATCH_INTERVAL_MS,
  });
}

/** The system's folder panel; resolves to `null` when cancelled. */
export function chooseProjectFolder() {
  return call(commands.projectsChooseFolder());
}

export function useAddProject() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (path: string) => call(commands.projectsAdd(path)),
    onSuccess: () => refresh(queryClient, queryKeys.projects.all()),
  });
}

export function useRemoveProject() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (path: string) => call(commands.projectsRemove(path)),
    onSuccess: () => refresh(queryClient, queryKeys.projects.all()),
  });
}

export interface ApplyVars {
  path: string;
  fingerprint: string;
  confirmed: boolean;
}

/** Applies a reviewed plan (refused if it changed since it was shown). */
export function useApplyProject() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ path, fingerprint, confirmed }: ApplyVars) =>
      call(commands.projectsApply(path, fingerprint, confirmed)),
    onSettled: () =>
      refresh(
        queryClient,
        queryKeys.projects.all(),
        queryKeys.routes.all(),
        queryKeys.quickShares.all(),
        queryKeys.snapshots.all(),
      ),
  });
}
