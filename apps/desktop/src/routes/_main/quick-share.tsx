import { createFileRoute } from "@tanstack/react-router";
import { QuickSharePage } from "@/features/quick-share";

interface QuickShareSearch {
  /** Focus the Share field (⇧⌘N, "Share a Local Port…" in the menu bar). */
  compose?: boolean;
}

export const Route = createFileRoute("/_main/quick-share")({
  validateSearch: (search: Record<string, unknown>): QuickShareSearch =>
    search["compose"] === true || search["compose"] === "true" ? { compose: true } : {},
  component: QuickShareRoute,
});

function QuickShareRoute() {
  const { compose } = Route.useSearch();
  return <QuickSharePage compose={compose === true} />;
}
