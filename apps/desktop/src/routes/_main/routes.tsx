import { createFileRoute } from "@tanstack/react-router";
import { RoutesPage } from "@/features/routes";

interface RoutesSearch {
  /** Open the New Route sheet (⌘N, "New Route" in the palette). */
  add?: boolean;
}

export const Route = createFileRoute("/_main/routes")({
  validateSearch: (search: Record<string, unknown>): RoutesSearch =>
    search["add"] === true || search["add"] === "true" ? { add: true } : {},
  component: RoutesRoute,
});

function RoutesRoute() {
  const { add } = Route.useSearch();
  return <RoutesPage adding={add === true} />;
}
