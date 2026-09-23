import { createFileRoute } from "@tanstack/react-router";
import { DoctorPage } from "@/features/doctor";

interface DoctorSearch {
  /** Open with this issue selected (from a route's or tunnel's inspector). */
  issue?: string;
}

export const Route = createFileRoute("/_main/doctor")({
  validateSearch: (search: Record<string, unknown>): DoctorSearch =>
    typeof search["issue"] === "string" ? { issue: search["issue"] } : {},
  component: DoctorRoute,
});

function DoctorRoute() {
  const { issue } = Route.useSearch();
  return <DoctorPage initialIssue={issue ?? null} />;
}
