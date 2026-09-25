import { createFileRoute } from "@tanstack/react-router";
import { InspectorPage } from "@/features/inspector";

interface InspectorSearch {
  /** Show this tap's requests (a Quick Share's id, a route's tap; links, cards). */
  tap?: string;
  /** Show this hostname's requests (a route or a share on your domain). */
  host?: string;
}

export const Route = createFileRoute("/_main/inspector")({
  validateSearch: (search: Record<string, unknown>): InspectorSearch => ({
    ...(typeof search["tap"] === "string" ? { tap: search["tap"] } : {}),
    ...(typeof search["host"] === "string" ? { host: search["host"] } : {}),
  }),
  component: InspectorRoute,
});

function InspectorRoute() {
  const { tap, host } = Route.useSearch();
  // A new target starts afresh (filters, selection).
  return <InspectorPage key={`${tap ?? ""}/${host ?? ""}`} tap={tap} host={host} />;
}
