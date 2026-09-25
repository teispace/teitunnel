import { createFileRoute } from "@tanstack/react-router";
import { CommentsPage } from "@/features/comments";

interface CommentsSearch {
  /** Open this subject (`share:…`, `route:…`, `snapshot:…`). */
  subject?: string;
}

export const Route = createFileRoute("/_main/comments")({
  validateSearch: (search: Record<string, unknown>): CommentsSearch =>
    typeof search["subject"] === "string" ? { subject: search["subject"] } : {},
  component: CommentsRoute,
});

function CommentsRoute() {
  const { subject } = Route.useSearch();
  return <CommentsPage subject={subject} />;
}
