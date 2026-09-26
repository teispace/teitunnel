import { GroupedRow, GroupedSection, SkeletonSection } from "@/components/patterns/grouped-list";
import { Badge } from "@/components/ui/badge";
import { relativeTime } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { ConnectedAgent } from "@/lib/ipc/bindings";
import { useAiAgents } from "./queries";

function modeLabel(mode: string): string {
  switch (mode) {
    case "read-only":
      return t("ai.agents.mode.readOnly");
    case "full":
      return t("ai.agents.mode.full");
    default:
      return t("ai.agents.mode.ask");
  }
}

function agentName(agent: ConnectedAgent): string {
  return agent.version ? `${agent.name} ${agent.version}` : agent.name;
}

/**
 * AI & Integrations ▸ Agents: AI agents connected through `teitunnel mcp` right now,
 * what each may do, and their changes waiting for an answer (asked in a dialog with
 * the plan).
 */
export function AgentsTab() {
  const { data } = useAiAgents();
  if (!data) return <SkeletonSection rows={2} />;
  return (
    <>
      <p className="px-2.5 text-callout text-secondary">{t("ai.agents.intro")}</p>
      {data.approvals.length > 0 ? (
        <GroupedSection title={t("ai.agents.waiting")}>
          {data.approvals.map((approval) => (
            <GroupedRow
              key={`${approval.agent}-${approval.askedAt}`}
              label={approval.title}
              description={t("ai.agents.waitingDetail", { agent: approval.agent })}
            >
              <span className="text-callout text-secondary">{relativeTime(approval.askedAt)}</span>
            </GroupedRow>
          ))}
        </GroupedSection>
      ) : null}
      <GroupedSection title={t("ai.agents.connected")}>
        {data.agents.length === 0 ? (
          <p className="py-2 text-callout text-secondary">{t("ai.agents.none")}</p>
        ) : (
          data.agents.map((agent) => (
            <GroupedRow
              key={`${agent.name}-${agent.connectedAt}`}
              label={agentName(agent)}
              description={t("ai.agents.since", { when: relativeTime(agent.connectedAt) })}
            >
              <Badge tone={agent.mode === "full" ? "warning" : "neutral"}>
                {modeLabel(agent.mode)}
              </Badge>
            </GroupedRow>
          ))
        )}
      </GroupedSection>
    </>
  );
}
