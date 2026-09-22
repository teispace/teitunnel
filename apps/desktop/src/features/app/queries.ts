import { queryOptions, useQuery } from "@tanstack/react-query";
import { commands } from "@/lib/ipc/bindings";
import { call } from "@/lib/ipc/client";
import { queryKeys } from "@/lib/ipc/query-keys";

export const appInfoQuery = queryOptions({
  queryKey: queryKeys.app.info(),
  queryFn: () => call(commands.appInfo()),
});

export function useAppInfo() {
  return useQuery(appInfoQuery);
}
