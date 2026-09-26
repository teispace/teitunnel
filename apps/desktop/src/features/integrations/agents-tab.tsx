import { GroupedRow, GroupedSection, SkeletonSection } from "@/components/patterns/grouped-list";
import { Badge } from "@/components/ui/badge";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { Switch } from "@/components/ui/switch";
import { relativeTime } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { ConnectedAgent, McpMode } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { useAiAgents, useMcpSettings, useSaveMcpSettings } from "./queries";

const modeDetail: Record<McpMode, string> = {
  "read-only": "ai.agents.modeDetail.readOnly",
  ask: "ai.agents.modeDetail.ask",
  full: "ai.agents.modeDetail.full",
};

/** What agents may do: their access, approving changes yourself, and secrets. */
function Permissions() {
  const { data } = useMcpSettings();
  const save = useSaveMcpSettings();
  if (!data) return <SkeletonSection rows={3} />;
  // Show the value being saved while it's saved.
  const settings = save.isPending && save.variables ? save.variables : data;
  const modes: { value: McpMode; label: string }[] = [
    { value: "read-only", label: t("ai.agents.mode.readOnly") },
    { value: "ask", label: t("ai.agents.mode.ask") },
    { value: "full", label: t("ai.agents.mode.full") },
  ];
  return (
    <GroupedSection title={t("ai.agents.permissions")} footer={t("ai.agents.permissionsFooter")}>
      <GroupedRow
        label={t("ai.agents.modeLabel")}
        description={t(modeDetail[settings.mode] as Parameters<typeof t>[0])}
      >
        <SegmentedControl
          label={t("ai.agents.modeLabel")}
          segments={modes}
          value={settings.mode}
          onValueChange={(mode) => save.mutate({ ...settings, mode })}
        />
      </GroupedRow>
      <GroupedRow
        label={t("ai.agents.approveInApp")}
        description={t("ai.agents.approveInAppDetail")}
      >
        <Switch
          aria-label={t("ai.agents.approveInApp")}
          checked={settings.approveInApp}
          disabled={settings.mode !== "ask"}
          onCheckedChange={(approveInApp) => save.mutate({ ...settings, approveInApp })}
        />
      </GroupedRow>
      <GroupedRow
        label={t("ai.agents.allowSecrets")}
        description={t("ai.agents.allowSecretsDetail")}
      >
        <Switch
          aria-label={t("ai.agents.allowSecrets")}
          checked={settings.allowSecrets}
          onCheckedChange={(allowSecrets) => save.mutate({ ...settings, allowSecrets })}
        />
      </GroupedRow>
      {save.error ? (
        <p role="alert" className="py-2 text-callout text-error">
          {toIpcError(save.error).message}
        </p>
      ) : null}
    </GroupedSection>
  );
}

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
      <Permissions />
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
