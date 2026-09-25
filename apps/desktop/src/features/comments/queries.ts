import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { commands, type TapId } from "@/lib/ipc/bindings";
import { call } from "@/lib/ipc/client";
import { queryKeys, refresh } from "@/lib/ipc/query-keys";

/** Every share, route and Snapshot with comments, newest activity first. */
export function useCommentSubjects() {
  return useQuery({
    queryKey: queryKeys.comments.subjects(),
    queryFn: () => call(commands.commentsSubjects()),
    staleTime: 15_000,
  });
}

/** A subject's threads (Snapshots' from Cloudflare); reading them marks them read. */
export function useThreads(key: string | null) {
  return useQuery({
    queryKey: queryKeys.comments.threads(key ?? ""),
    queryFn: () => call(commands.commentsThreads(key ?? "")),
    enabled: key !== null,
    staleTime: 10_000,
    retry: false,
  });
}

/** The owner's reply on a thread. */
export function useReply(key: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ thread, body }: { thread: string; body: string }) =>
      call(commands.commentsReply(key, thread, body)),
    onSuccess: () => refresh(queryClient, queryKeys.comments.all()),
  });
}

/** Resolves or reopens a thread. */
export function useResolve(key: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ thread, resolved }: { thread: string; resolved: boolean }) =>
      call(commands.commentsResolve(key, thread, resolved)),
    onSuccess: () => refresh(queryClient, queryKeys.comments.all()),
  });
}

/** Removes a subject (and the comments kept on this computer for it) from the list. */
export function useForget() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (key: string) => call(commands.commentsForget(key)),
    onSuccess: () => refresh(queryClient, queryKeys.comments.all()),
  });
}

/** Turns comments on a Quick Share or an inspected route on or off. */
export function useSetTapComments() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ tap, on }: { tap: TapId; on: boolean }) =>
      call(commands.commentsSetTap(tap, on)),
    onSuccess: () =>
      refresh(
        queryClient,
        queryKeys.inspector.all(),
        queryKeys.comments.all(),
        queryKeys.quickShares.all(),
      ),
  });
}
