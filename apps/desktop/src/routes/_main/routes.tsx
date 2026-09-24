import { createFileRoute } from "@tanstack/react-router";
import { RoutesPage } from "@/features/routes";

interface RoutesSearch {
  /** Open the New Route sheet (⌘N, "New Route" in the palette). */
  add?: boolean;
  /** Select this hostname (`teitunnel://open?route=…`, the control connection). */
  route?: string;
}

export const Route = createFileRoute("/_main/routes")({
  validateSearch: (search: Record<string, unknown>): RoutesSearch => ({
    ...(search["add"] === true || search["add"] === "true" ? { add: true } : {}),
    ...(typeof search["route"] === "string" ? { route: search["route"] } : {}),
  }),
  component: RoutesRoute,
});

function RoutesRoute() {
  const { add, route } = Route.useSearch();
  // A new target starts a fresh selection.
  return <RoutesPage key={route ?? ""} adding={add === true} focus={route} />;
}
