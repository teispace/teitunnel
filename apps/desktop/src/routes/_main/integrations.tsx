import { createFileRoute } from "@tanstack/react-router";
import { IntegrationsPage, isIntegrationsTab } from "@/features/integrations";

interface IntegrationsSearch {
  /** The tab to show (AI Apps when absent). */
  tab?: string;
}

export const Route = createFileRoute("/_main/integrations")({
  validateSearch: (search: Record<string, unknown>): IntegrationsSearch =>
    isIntegrationsTab(search["tab"]) ? { tab: search["tab"] } : {},
  component: IntegrationsRoute,
});

function IntegrationsRoute() {
  const { tab } = Route.useSearch();
  const navigate = Route.useNavigate();
  return (
    <IntegrationsPage
      tab={isIntegrationsTab(tab) ? tab : "apps"}
      onTabChange={(next) => void navigate({ search: next === "apps" ? {} : { tab: next } })}
    />
  );
}
