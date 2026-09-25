import { toast } from "sonner";
import { ConfirmDialog } from "@/components/patterns/confirm-dialog";
import { GroupedRow, GroupedSection } from "@/components/patterns/grouped-list";
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
      description={t("settings.mcpConnections.detail", {
        host: connection.host,
        approved: relativeTime(connection.createdAt ?? null),
        used: relativeTime(connection.lastUsedAt ?? null),
      })}
    >
      <ConfirmDialog
        trigger={
          <Button size="sm" variant="destructive">
            {t("settings.mcpConnections.disconnect")}
          </Button>
        }
        title={t("settings.mcpConnections.confirmTitle", names)}
        description={t("settings.mcpConnections.confirmDetail")}
        confirmLabel={t("settings.mcpConnections.disconnect")}
        variant="destructive"
        onConfirm={async () => {
          await disconnect.mutateAsync(connection.id);
          toast.success(t("settings.mcpConnections.disconnected", names));
        }}
      />
    </GroupedRow>
  );
}

/**
 * Settings ▸ General ▸ AI Tools, continued: apps signed in with OAuth to MCP servers
 * shared on the person's domain (claude.ai, ChatGPT…), each approved once in a dialog.
 * Shown only once there's one.
 */
export function McpConnections() {
  const { data } = useMcpConnections();
  if (!data || data.length === 0) return null;
  return (
    <GroupedSection
      title={t("settings.mcpConnections.title")}
      footer={t("settings.mcpConnections.footer")}
    >
      {data.map((connection) => (
        <Connection key={connection.id} connection={connection} />
      ))}
    </GroupedSection>
  );
}
