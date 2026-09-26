import type { ReactNode } from "react";
import { TitlebarToolbar } from "@/components/patterns/titlebar-toolbar";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { t } from "@/lib/i18n";
import { AgentsTab } from "./agents-tab";
import { AiAppsTab } from "./ai-apps-tab";
import { BrowserTab } from "./browser-tab";
import { MoreTab } from "./more-tab";
import { ServersTab } from "./servers-tab";

export const integrationTabs = ["apps", "agents", "servers", "browser", "more"] as const;
export type IntegrationsTab = (typeof integrationTabs)[number];

export function isIntegrationsTab(value: unknown): value is IntegrationsTab {
  return integrationTabs.includes(value as IntegrationsTab);
}

const content: Record<IntegrationsTab, () => ReactNode> = {
  apps: () => <AiAppsTab />,
  agents: () => <AgentsTab />,
  servers: () => <ServersTab />,
  browser: () => <BrowserTab />,
  more: () => <MoreTab />,
};

/**
 * AI & Integrations: everything that works with Teitunnel from outside it, AI apps
 * through its MCP server, agents and their approvals, MCP servers shared with remote
 * apps, the browser extension, and other programs, in one place with a tab each.
 */
export function IntegrationsPage({
  tab,
  onTabChange,
}: {
  tab: IntegrationsTab;
  onTabChange: (tab: IntegrationsTab) => void;
}) {
  const segments = integrationTabs.map((value) => ({ value, label: t(`ai.tab.${value}`) }));
  return (
    <>
      <TitlebarToolbar title={t("ai.title")}>
        <SegmentedControl
          label={t("ai.tabs")}
          segments={segments}
          value={tab}
          onValueChange={onTabChange}
        />
      </TitlebarToolbar>
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="mx-auto flex max-w-3xl flex-col gap-5 px-5 pt-4 pb-8">{content[tab]()}</div>
      </div>
    </>
  );
}
