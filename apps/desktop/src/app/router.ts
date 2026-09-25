import type { QueryClient } from "@tanstack/react-query";
import { createRouter } from "@tanstack/react-router";
import { LoadingState } from "@/components/patterns/loading-state";
import { routeTree } from "@/routeTree.gen";
import { RouteError } from "./route-error";

export interface RouterContext {
  queryClient: QueryClient;
}

export function createAppRouter(queryClient: QueryClient) {
  return createRouter({
    routeTree,
    context: { queryClient },
    defaultPreload: "intent",
    // Local data is instant; never flash a pending state for it.
    defaultPendingMs: 300,
    // A screen whose code is still loading says so instead of staying empty.
    defaultPendingComponent: LoadingState,
    scrollRestoration: true,
    defaultErrorComponent: RouteError,
  });
}

declare module "@tanstack/react-router" {
  interface Register {
    router: ReturnType<typeof createAppRouter>;
  }
}
