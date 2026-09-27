// The tabs of AI & Integrations, apart from the page: the route checks its search with
// them, and anything the route imports statically is in the app's first load.
export const integrationTabs = ["apps", "agents", "servers", "browser", "more"] as const;
export type IntegrationsTab = (typeof integrationTabs)[number];

export function isIntegrationsTab(value: unknown): value is IntegrationsTab {
  return integrationTabs.includes(value as IntegrationsTab);
}
