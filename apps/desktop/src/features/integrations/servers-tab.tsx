import { toast } from "sonner";
import { ConfirmDialog } from "@/components/patterns/confirm-dialog";
import { GroupedRow, GroupedSection, SkeletonSection } from "@/components/patterns/grouped-list";
import { Button } from "@/components/ui/button";
import { relativeTime } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { McpConnection } from "@/lib/ipc/bindings";
import { useMcpConnections, useMcpDisconnect } from "./queries";

function Connection({ connection }: { connection: McpConnection }) {
  const disconnect = useMcpDisconnect();
  const names = { client: connection.clientName, host: connection.host };
  return (
    <GroupedRow
      label={connection.clientName}
      description={t("ai.servers.detail", {
        host: connection.redirectHost,
        approved: relativeTime(connection.createdAt),
        used: relativeTime(connection.lastUsedAt),
      })}
    >
      <ConfirmDialog
        trigger={
          <Button size="sm" variant="destructive">
            {t("ai.servers.disconnect")}
          </Button>
        }
        title={t("ai.servers.confirmTitle", names)}
        description={t("ai.servers.confirmDetail")}
        confirmLabel={t("ai.servers.disconnect")}
        variant="destructive"
        onConfirm={async () => {
          await disconnect.mutateAsync(connection.id);
          toast.success(t("ai.servers.disconnected", names));
        }}
      />
    </GroupedRow>
  );
}

/** Connections grouped by the shared server they're for, keeping the newest first. */
function byHost(connections: readonly McpConnection[]): [string, McpConnection[]][] {
  const groups = new Map<string, McpConnection[]>();
  for (const connection of connections) {
    groups.set(connection.host, [...(groups.get(connection.host) ?? []), connection]);
  }
  return [...groups.entries()];
}

/**
 * AI & Integrations ▸ Shared Servers: MCP servers shared from this computer that
 * remote AI apps (claude.ai, ChatGPT…) sign in to with OAuth, each approved once in a
 * dialog; what Teitunnel keeps of their tokens, and disconnecting them.
 */
export function ServersTab() {
  const { data } = useMcpConnections();
  if (!data) return <SkeletonSection rows={2} />;
  return (
    <>
      <p className="px-2.5 text-callout text-secondary">{t("ai.servers.intro")}</p>
      <p className="px-2.5 text-callout text-secondary">{t("ai.servers.how")}</p>
      {data.length === 0 ? (
        <GroupedSection>
          <p className="py-2 text-callout text-secondary">{t("ai.servers.none")}</p>
        </GroupedSection>
      ) : (
        byHost(data).map(([host, connections]) => (
          <GroupedSection key={host} title={t("ai.servers.connections", { host })}>
            {connections.map((connection) => (
              <Connection key={connection.id} connection={connection} />
            ))}
          </GroupedSection>
        ))
      )}
      <p className="px-2.5 text-callout text-secondary">{t("ai.servers.storage")}</p>
    </>
  );
}
