import { GroupedRow, GroupedSection } from "@/components/patterns/grouped-list";
import { Badge } from "@/components/ui/badge";
import { relativeTime } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { ConnectedAgent } from "@/lib/ipc/bindings";
import { useAiAgents } from "./queries";

function modeLabel(mode: string): string {
  switch (mode) {
    case "read-only":
      return t("settings.aiAgents.mode.readOnly");
    case "full":
      return t("settings.aiAgents.mode.full");
    default:
      return t("settings.aiAgents.mode.ask");
  }
}

function agentName(agent: ConnectedAgent): string {
  return agent.version ? `${agent.name} ${agent.version}` : agent.name;
}

/**
 * Settings ▸ General ▸ AI Tools, continued: agents connected through `teitunnel mcp`
 * right now, and their changes waiting for an answer (asked in a dialog with the plan).
 */
export function AiAgents() {
  const { data } = useAiAgents();
  if (!data) return null;
  return (
    <GroupedSection title={t("settings.aiAgents.title")} footer={t("settings.aiAgents.footer")}>
      {data.agents.length === 0 ? (
        <p className="py-2 text-callout text-secondary">{t("settings.aiAgents.none")}</p>
      ) : (
        data.agents.map((agent) => (
          <GroupedRow
            key={`${agent.name}-${agent.connectedAt ?? 0}`}
            label={agentName(agent)}
            description={t("settings.aiAgents.since", {
              when: relativeTime(agent.connectedAt ?? null),
            })}
          >
            <Badge tone={agent.mode === "full" ? "warning" : "neutral"}>
              {modeLabel(agent.mode)}
            </Badge>
          </GroupedRow>
        ))
      )}
      {data.approvals.map((approval) => (
        <GroupedRow
          key={`${approval.agent}-${approval.askedAt ?? 0}`}
          label={approval.title}
          description={t("settings.aiAgents.waiting", { agent: approval.agent })}
        >
          <Badge tone="accent">{t("settings.aiAgents.pending")}</Badge>
        </GroupedRow>
      ))}
    </GroupedSection>
  );
}
