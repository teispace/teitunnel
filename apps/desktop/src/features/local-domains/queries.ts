import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  type AdminTask,
  type CaFormat,
  commands,
  type LocalDomainFix,
  type LocalDomainInput,
  type TrustOptions,
} from "@/lib/ipc/bindings";
import { call } from "@/lib/ipc/client";
import { queryKeys, refresh } from "@/lib/ipc/query-keys";

/** How often a fix in progress is checked again (trust, the `.test` resolver). */
export const RECHECK_MS = 3000;

/** The domains, listeners, `.test` names and the CA (no prompts; bounded lookups). */
export function useLocalDomains(options: { recheck?: boolean } = {}) {
  return useQuery({
    queryKey: queryKeys.localDomains.status(),
    queryFn: () => call(commands.localDomainsStatus()),
    staleTime: 10_000,
    refetchInterval: options.recheck ? RECHECK_MS : false,
  });
}

/** Where the CA is trusted (runs the system's tools, so only when it's shown). */
export function useTrust(options: { enabled?: boolean; recheck?: boolean } = {}) {
  return useQuery({
    queryKey: queryKeys.localDomains.trust(),
    queryFn: () => call(commands.localDomainsTrustStatus()),
    enabled: options.enabled ?? true,
    staleTime: 30_000,
    refetchInterval: options.recheck ? RECHECK_MS : false,
  });
}

function useLocalMutation<V, R>(run: (vars: V) => Promise<R>) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: run,
    onSettled: () => refresh(queryClient, queryKeys.localDomains.all(), queryKeys.doctor.all()),
  });
}

export function useAddLocalDomain() {
  return useLocalMutation((input: LocalDomainInput) => call(commands.localDomainsAdd(input)));
}

export function useUpdateLocalDomain() {
  return useLocalMutation((input: LocalDomainInput) => call(commands.localDomainsUpdate(input)));
}

export function useSetInspect() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ name, inspect }: { name: string; inspect: boolean }) =>
      call(commands.localDomainsSetInspect(name, inspect)),
    onSettled: () => refresh(queryClient, queryKeys.localDomains.all(), queryKeys.inspector.all()),
  });
}

export function useRemoveLocalDomain() {
  return useLocalMutation((name: string) => call(commands.localDomainsRemove(name)));
}

export function useSetLan() {
  return useLocalMutation((lan: boolean) => call(commands.localDomainsSetLan(lan)));
}

export function useRestart() {
  return useLocalMutation(() => call(commands.localDomainsRestart()));
}

export function useTrustCa() {
  return useLocalMutation((options: TrustOptions) => call(commands.localDomainsTrust(options)));
}

export function useUntrustCa() {
  return useLocalMutation((forget: boolean) => call(commands.localDomainsUntrust(forget)));
}

export function useRunAsAdmin() {
  return useLocalMutation((task: AdminTask) => call(commands.localDomainsRunAsAdmin(task)));
}

export function useLocalFix() {
  return useLocalMutation((action: LocalDomainFix) => call(commands.localDomainsFix(action)));
}

/** Saves the CA certificate for a phone; resolves to the path, or `null` when cancelled. */
export function saveCaCertificate(format: CaFormat) {
  return call(commands.localDomainsSaveCa(format));
}
