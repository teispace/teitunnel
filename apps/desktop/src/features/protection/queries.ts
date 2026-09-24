import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Channel } from "@tauri-apps/api/core";
import { useState } from "react";
import {
  commands,
  type Progress,
  type ProtectionChange,
  type SecretCopy,
  type StepState,
} from "@/lib/ipc/bindings";
import { call } from "@/lib/ipc/client";
import { queryKeys, refresh } from "@/lib/ipc/query-keys";

/** What Teitunnel enforces for a hostname at Cloudflare's edge, with the zone's quotas. */
export function useProtection(accountId: string, hostname: string, enabled = true) {
  return useQuery({
    queryKey: queryKeys.protection.hostname(accountId, hostname),
    queryFn: () => call(commands.protectionGet(accountId, hostname)),
    enabled,
    staleTime: 30_000,
    retry: false,
  });
}

/** Teitunnel's service tokens for a hostname (never their secrets). */
export function useServiceTokens(accountId: string, hostname: string, enabled = true) {
  return useQuery({
    queryKey: queryKeys.protection.tokens(accountId, hostname),
    queryFn: () => call(commands.protectionTokens(accountId, hostname)),
    enabled,
    staleTime: 30_000,
    retry: false,
  });
}

/** Plans a protection change for review. */
export function useProtectionPreview(accountId: string) {
  return useMutation({
    mutationFn: (change: ProtectionChange) => call(commands.protectionPreview(accountId, change)),
  });
}

export interface ProtectionApplyVars {
  change: ProtectionChange;
  fingerprint: string;
}

/** Applies a reviewed protection change, tracking each step's state as it streams in. */
export function useProtectionApply(accountId: string) {
  const queryClient = useQueryClient();
  const [steps, setSteps] = useState<Record<number, StepState>>({});
  const mutation = useMutation({
    mutationFn: ({ change, fingerprint }: ProtectionApplyVars) => {
      setSteps({});
      const channel = new Channel<Progress>();
      channel.onmessage = (progress) =>
        setSteps((current) => ({ ...current, [progress.step]: progress.state }));
      return call(commands.protectionApply(accountId, change, fingerprint, false, channel));
    },
    onSettled: () =>
      refresh(queryClient, queryKeys.protection.all(), queryKeys.routes.activity(accountId)),
  });
  return { ...mutation, steps };
}

/** Previews and applies in one go (Undo, where the user already decided). */
export async function applyProtectionDirectly(accountId: string, change: ProtectionChange) {
  const plan = await call(commands.protectionPreview(accountId, change));
  return call(
    commands.protectionApply(accountId, change, plan.fingerprint, false, new Channel<Progress>()),
  );
}

/** Copies a new token's secret (or both headers) to the clipboard, from Rust. */
export function copySecret(tokenId: string, what: SecretCopy) {
  return call(commands.protectionCopySecret(tokenId, what));
}

/** Forgets a new token's secret once its panel closes. */
export function forgetSecret(tokenId: string) {
  return call(commands.protectionForgetSecret(tokenId));
}
