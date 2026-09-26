import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Channel } from "@tauri-apps/api/core";
import { useState } from "react";
import {
  commands,
  type FrontChange,
  type InboxVerify,
  type Progress,
  type StepState,
} from "@/lib/ipc/bindings";
import { call } from "@/lib/ipc/client";
import { queryKeys, refresh } from "@/lib/ipc/query-keys";

/** Teitunnel's offline pages and webhook inboxes (this computer's records, no network). */
export function useFronts() {
  return useQuery({
    queryKey: queryKeys.fronts.list(),
    queryFn: () => call(commands.frontsList(null)),
    staleTime: 30_000,
  });
}

/** A webhook inbox's recent webhooks (read from the account's D1 database). */
export function useInboxItems(accountId: string, hostname: string, path: string, enabled = true) {
  return useQuery({
    queryKey: queryKeys.fronts.inbox(accountId, hostname, path),
    queryFn: () => call(commands.inboxItems(accountId, hostname, path)),
    enabled,
    staleTime: 15_000,
    refetchInterval: 30_000,
    retry: false,
  });
}

/** Delivers waiting webhooks now. */
export function useDeliverNow(accountId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => call(commands.inboxDeliver(accountId)),
    onSettled: () => refresh(queryClient, queryKeys.fronts.all()),
  });
}

/** Which senders have a signing secret saved for `hostname` (never the secrets). */
export function useInboxSecrets(hostname: string) {
  return useQuery({
    queryKey: queryKeys.fronts.secrets(hostname),
    queryFn: () => call(commands.frontsInboxSecrets(hostname)),
  });
}

/** Saves a signing secret for a hostname's verifying inboxes (into the keychain). */
export function useSaveInboxSecret(hostname: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ verify, secret }: { verify: InboxVerify; secret: string }) =>
      call(commands.frontsInboxSecretSet(hostname, verify, secret)),
    onSettled: () => refresh(queryClient, queryKeys.fronts.secrets(hostname)),
  });
}

/** Plans an offline page or inbox change for review. */
export function useFrontPreview(accountId: string) {
  return useMutation({
    mutationFn: (change: FrontChange) => call(commands.frontsPreview(accountId, change)),
  });
}

export interface FrontApplyVars {
  change: FrontChange;
  fingerprint: string;
}

/** Applies a reviewed change, tracking each step's state as it streams in. */
export function useFrontApply(accountId: string) {
  const queryClient = useQueryClient();
  const [steps, setSteps] = useState<Record<number, StepState>>({});
  const mutation = useMutation({
    mutationFn: ({ change, fingerprint }: FrontApplyVars) => {
      setSteps({});
      const channel = new Channel<Progress>();
      channel.onmessage = (progress) =>
        setSteps((current) => ({ ...current, [progress.step]: progress.state }));
      return call(commands.frontsApply(accountId, change, fingerprint, false, channel));
    },
    onSettled: () =>
      refresh(queryClient, queryKeys.fronts.all(), queryKeys.routes.activity(accountId)),
  });
  return { ...mutation, steps };
}

/** The change that puts things back as they are now (asked before applying, for Undo). */
export function undoChange(accountId: string, change: FrontChange) {
  return call(commands.frontsUndoChange(accountId, change));
}

/** Previews and applies in one go (Undo, where the user already decided). */
export async function applyFrontDirectly(accountId: string, change: FrontChange) {
  const plan = await call(commands.frontsPreview(accountId, change));
  return call(
    commands.frontsApply(accountId, change, plan.fingerprint, false, new Channel<Progress>()),
  );
}
