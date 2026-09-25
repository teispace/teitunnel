import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Channel } from "@tauri-apps/api/core";
import { useState } from "react";
import { quickSharesQuery } from "@/features/quick-share/queries";
import {
  commands,
  type ExchangeId,
  type ExchangeQuery,
  type InspectorSettingsPatch,
  type Progress,
  type ProtectionInput,
  type QuickShare,
  type ReplayInput,
  type Resume,
  type StepState,
  type TapId,
  type TapPatch,
  type TrafficFormat,
  type WebhookSender,
} from "@/lib/ipc/bindings";
import { call } from "@/lib/ipc/client";
import { queryKeys, refresh } from "@/lib/ipc/query-keys";

const keys = {
  settings: () => [...queryKeys.inspector.all(), "settings"] as const,
  taps: () => [...queryKeys.inspector.all(), "taps"] as const,
  known: () => [...queryKeys.inspector.all(), "known"] as const,
  routes: () => [...queryKeys.inspector.all(), "routes"] as const,
  exchange: (id: string, version: string) =>
    [...queryKeys.inspector.all(), "exchange", id, version] as const,
  webhookSecrets: (tap: string) => [...queryKeys.inspector.all(), "webhookSecrets", tap] as const,
  metrics: (tap: string) => [...queryKeys.inspector.all(), "metrics", tap] as const,
  export: (ids: readonly string[], format: TrafficFormat, redact: boolean) =>
    [...queryKeys.inspector.all(), "export", ids.join(","), format, redact] as const,
};

export const inspectorKeys = keys;

/** The inspector's settings (Settings ▸ Inspector). */
export function useInspectorSettings() {
  return useQuery({
    queryKey: keys.settings(),
    queryFn: () => call(commands.inspectSettingsGet()),
  });
}

export function useUpdateInspectorSettings() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (patch: InspectorSettingsPatch) => call(commands.inspectSettingsSet(patch)),
    onSuccess: (settings) => queryClient.setQueryData(keys.settings(), settings),
  });
}

/** Running taps (inspected shares and routes), with their settings. */
export function useTaps() {
  return useQuery({ queryKey: keys.taps(), queryFn: () => call(commands.inspectTaps()) });
}

/** Every tap captures refer to: running, stopped, or from the history. */
export function useKnownTaps() {
  return useQuery({ queryKey: keys.known(), queryFn: () => call(commands.inspectKnownTaps()) });
}

/** Routes pointed at an inspector, in every account. */
export function useInspectedRoutes() {
  return useQuery({ queryKey: keys.routes(), queryFn: () => call(commands.inspectRoutes()) });
}

/**
 * One request in full, masked. `version` changes as the request progresses (state,
 * duration), so a finished response is read again. Revealed copies are never cached:
 * see `useReveal`.
 */
export function useExchange(id: ExchangeId | null, version: string) {
  return useQuery({
    queryKey: keys.exchange(id ?? "", version),
    queryFn: () => call(commands.inspectExchange(id ?? "", false)),
    enabled: id !== null,
    placeholderData: (previous, query) =>
      // Keep showing the same request while its newer state loads (not another one).
      query && previous?.view.id === id ? previous : undefined,
    staleTime: 10_000,
  });
}

/**
 * Reads one request with its secrets, on the person's click. Deliberately not a query or
 * a mutation: nothing caches it; it lives only in the detail pane's state until hidden
 * or another request is selected.
 */
export function revealExchange(id: ExchangeId) {
  return call(commands.inspectExchange(id, true));
}

/**
 * A request held at a breakpoint, as it would go on (`null` once it went on). Unmasked,
 * since it's what goes on and can be changed, so, like a revealed request, it's read on
 * demand and never cached.
 */
export function heldExchange(id: ExchangeId) {
  return call(commands.inspectPausedExchange(id));
}

/** Lets a held request go on: as it is, changed, answered from here or dropped. */
export function useResume() {
  return useMutation({
    mutationFn: ({ id, resume }: { id: ExchangeId; resume: Resume }) =>
      call(commands.inspectResume(id, resume)),
  });
}

/** Lets every held request (of one tap, or all) go on unchanged. */
export function useResumeAll() {
  return useMutation({
    mutationFn: (tap: TapId | null) => call(commands.inspectResumeAll(tap)),
  });
}

/** A page of requests (masked), newest first. */
export function fetchExchanges(query: ExchangeQuery) {
  return call(commands.inspectExchanges(query));
}

export function useReplay() {
  return useMutation({
    mutationFn: ({ id, input }: { id: ExchangeId; input: ReplayInput }) =>
      call(commands.inspectReplay(id, input)),
  });
}

/** The export's text, for the preview and Copy. */
export function useExport(ids: readonly ExchangeId[], format: TrafficFormat, redact: boolean) {
  return useQuery({
    queryKey: keys.export(ids, format, redact),
    queryFn: () => call(commands.inspectExport([...ids], format, redact)),
    enabled: ids.length > 0,
    placeholderData: keepPreviousData,
    // An unredacted export holds secrets: never keep it around.
    gcTime: redact ? 60_000 : 0,
    staleTime: 0,
  });
}

export function useExportSave() {
  return useMutation({
    mutationFn: ({
      ids,
      format,
      redact,
    }: {
      ids: readonly ExchangeId[];
      format: TrafficFormat;
      redact: boolean;
    }) => call(commands.inspectExportSave([...ids], format, redact)),
  });
}

export function useClearExchanges() {
  return useMutation({
    mutationFn: (tap: TapId | null) => call(commands.inspectClear(tap)),
  });
}

export function useConfigureTap() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ tap, patch }: { tap: TapId; patch: TapPatch }) =>
      call(commands.inspectConfigure(tap, patch)),
    onSuccess: () => refresh(queryClient, keys.taps()),
  });
}

export function useProtectTap() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ tap, input }: { tap: TapId; input: ProtectionInput }) =>
      call(commands.inspectProtect(tap, input)),
    onSuccess: () => refresh(queryClient, keys.taps()),
  });
}

/** A tap's counters and latency, polled while shown. */
export function useTapMetrics(tap: TapId | null) {
  return useQuery({
    queryKey: keys.metrics(tap ?? ""),
    queryFn: () => call(commands.inspectMetrics(tap ?? "")),
    enabled: tap !== null,
    refetchInterval: 2000,
    placeholderData: keepPreviousData,
    retry: false,
  });
}

/** Webhook senders with a saved signing secret (never the secrets). */
export function useWebhookSecrets(tap: TapId | null) {
  return useQuery({
    queryKey: keys.webhookSecrets(tap ?? ""),
    queryFn: () => call(commands.inspectWebhookSecrets(tap ?? "")),
    enabled: tap !== null,
    retry: false,
  });
}

/** Saves or removes a signing secret; the request's check is read again. */
export function useSetWebhookSecret() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      tap,
      provider,
      secret,
    }: {
      tap: TapId;
      provider: WebhookSender;
      /** `null` removes it. */
      secret: string | null;
    }) =>
      call(
        secret === null
          ? commands.inspectWebhookSecretRemove(tap, provider)
          : commands.inspectWebhookSecretSet(tap, provider, secret),
      ),
    onSuccess: (_, { tap }) =>
      refresh(queryClient, keys.webhookSecrets(tap), [
        ...queryKeys.inspector.all(),
        "exchange",
      ] as const),
  });
}

/** Turns inspection of a running Quick Share on or off (it restarts with a new URL). */
export function useSetShareInspected() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, inspect }: { id: string; inspect: boolean }) =>
      call(commands.quickShareSetInspected(id, inspect)),
    onSuccess: (share) => {
      queryClient.setQueryData<QuickShare[]>(quickSharesQuery.queryKey, (shares = []) =>
        shares.map((existing) => (existing.id === share.id ? share : existing)),
      );
      return refresh(queryClient, queryKeys.inspector.all());
    },
  });
}

export interface InspectRouteTarget {
  accountId: string;
  hostname: string;
  path: string | null;
  /** Inspect (`true`) or go back to the route's own service. */
  on: boolean;
}

/** Plans inspecting a route, or ending it, for review. */
export function useInspectRoutePreview() {
  return useMutation({
    mutationFn: ({ accountId, hostname, path, on }: InspectRouteTarget) =>
      call(commands.inspectRoutePreview(accountId, hostname, path, on)),
  });
}

/** Applies a reviewed inspection plan, tracking each step's state as it streams in. */
export function useInspectRouteApply() {
  const queryClient = useQueryClient();
  const [steps, setSteps] = useState<Record<number, StepState>>({});
  const mutation = useMutation({
    mutationFn: ({
      target,
      fingerprint,
      confirmed,
    }: {
      target: InspectRouteTarget;
      fingerprint: string;
      confirmed: boolean;
    }) => {
      setSteps({});
      const channel = new Channel<Progress>();
      channel.onmessage = (progress) =>
        setSteps((current) => ({ ...current, [progress.step]: progress.state }));
      return call(
        commands.inspectRouteApply(
          target.accountId,
          target.hostname,
          target.path,
          target.on,
          fingerprint,
          confirmed,
          channel,
        ),
      );
    },
    onSettled: (_, __, { target }) =>
      refresh(
        queryClient,
        queryKeys.inspector.all(),
        queryKeys.routes.all(),
        queryKeys.doctor.all(),
        queryKeys.routes.activity(target.accountId),
      ),
  });
  return { ...mutation, steps };
}
