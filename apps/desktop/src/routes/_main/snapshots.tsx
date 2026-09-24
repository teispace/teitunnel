import { createFileRoute } from "@tanstack/react-router";
import { SnapshotsPage } from "@/features/snapshots";

interface SnapshotsSearch {
  /** Open the publish sheet to capture this running site (from a Quick Share card). */
  capture?: string;
  /** Open the publish sheet. */
  publish?: boolean;
}

export const Route = createFileRoute("/_main/snapshots")({
  validateSearch: (search: Record<string, unknown>): SnapshotsSearch => ({
    ...(typeof search["capture"] === "string" ? { capture: search["capture"] } : {}),
    ...(search["publish"] === true || search["publish"] === "true" ? { publish: true } : {}),
  }),
  component: SnapshotsRoute,
});

function SnapshotsRoute() {
  const { capture, publish } = Route.useSearch();
  return <SnapshotsPage capture={capture} publish={publish === true} />;
}
