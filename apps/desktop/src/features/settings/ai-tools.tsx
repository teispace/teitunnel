import { CopyField } from "@/components/patterns/copy-field";
import { GroupedRow, GroupedSection } from "@/components/patterns/grouped-list";
import { Button } from "@/components/ui/button";
import { t } from "@/lib/i18n";
import type { AiClientView } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { useAiClients, useSetAiClientConnected } from "./queries";

function describe(client: AiClientView) {
  if (client.problem) return t("settings.aiTools.problem", { problem: client.problem });
  if (client.connected) return t("settings.aiTools.connected");
  return client.detected ? t("settings.aiTools.notConnected") : t("settings.aiTools.notFound");
}

/** Settings ▸ General ▸ AI Tools: connect AI clients to Teitunnel's MCP server. */
export function AiTools() {
  const { data } = useAiClients();
  const change = useSetAiClientConnected();
  if (!data) return null;
  if (!data.command) {
    return (
      <GroupedSection title={t("settings.aiTools.title")}>
        <p className="py-2 text-callout text-secondary">{t("settings.aiTools.noCli")}</p>
      </GroupedSection>
    );
  }
  // Installed tools first, keeping their order otherwise.
  const clients = [...data.clients].sort(
    (a, b) => Number(b.detected || b.connected) - Number(a.detected || a.connected),
  );
  return (
    <GroupedSection title={t("settings.aiTools.title")} footer={t("settings.aiTools.footer")}>
      {clients.map((client) => {
        const pending = change.isPending && change.variables?.id === client.id;
        return (
          <GroupedRow key={client.id} label={client.name} description={describe(client)}>
            <Button
              size="sm"
              pending={pending}
              disabled={
                (change.isPending && !pending) || (client.problem !== null && !client.connected)
              }
              onClick={() => change.mutate({ id: client.id, connect: !client.connected })}
            >
              {client.connected ? t("settings.aiTools.disconnect") : t("settings.aiTools.connect")}
            </Button>
          </GroupedRow>
        );
      })}
      {change.error ? (
        <p role="alert" className="pb-2 text-callout text-error">
          {toIpcError(change.error).message}
        </p>
      ) : null}
      <div className="pt-1 pb-2">
        <p className="pb-1 text-callout text-secondary">{t("settings.aiTools.others")}</p>
        <CopyField label={t("settings.aiTools.command")} value={`${data.command} mcp`} />
      </div>
    </GroupedSection>
  );
}
