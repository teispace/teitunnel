import { queryOptions, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useUiStore } from "@/app/ui-store";
import { type Account, commands } from "@/lib/ipc/bindings";
import { call } from "@/lib/ipc/client";
import { queryKeys } from "@/lib/ipc/query-keys";

export const accountsQuery = queryOptions({
  queryKey: queryKeys.accounts.all(),
  queryFn: () => call(commands.accountsList()),
});

export function useAccounts() {
  return useQuery(accountsQuery);
}

/** The account shown in Domains/Routes: the remembered one if still connected, else the first. */
export function useActiveAccount(): Account | null {
  const { data: accounts = [] } = useAccounts();
  const activeId = useUiStore((state) => state.activeAccountId);
  return accounts.find((a) => a.id === activeId) ?? accounts[0] ?? null;
}

export function useCertDetected(enabled: boolean) {
  return useQuery({
    queryKey: queryKeys.accounts.cert(),
    queryFn: () => call(commands.accountsDetectCert()),
    enabled,
  });
}

export function useCapabilities(accountId: string | null) {
  return useQuery({
    queryKey: queryKeys.accounts.capabilities(accountId ?? ""),
    queryFn: () => call(commands.accountsCapabilities(accountId ?? "")),
    enabled: accountId !== null,
    staleTime: 10 * 60 * 1000,
  });
}

export function useDomains(accountId: string | null) {
  return useQuery({
    queryKey: queryKeys.domains.list(accountId ?? ""),
    queryFn: () => call(commands.domainsList(accountId ?? "")),
    enabled: accountId !== null,
    staleTime: 60 * 1000,
    refetchOnWindowFocus: true,
  });
}

function useAccountMutation<TVars, TResult>(fn: (vars: TVars) => Promise<TResult>) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: fn,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.accounts.all() });
      void queryClient.invalidateQueries({ queryKey: queryKeys.domains.all() });
    },
  });
}

export function useAddToken() {
  return useAccountMutation((token: string) => call(commands.accountsAddToken(token)));
}

export function useImportCert() {
  return useAccountMutation(() => call(commands.accountsImportCert()));
}

export function useRemoveAccount() {
  return useAccountMutation((id: string) => call(commands.accountsRemove(id)));
}

export function openTokenPage() {
  return call(commands.accountsOpenTokenPage());
}
