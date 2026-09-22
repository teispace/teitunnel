import { Network, RefreshCw } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { useUiStore } from "@/app/ui-store";
import { EmptyState } from "@/components/patterns/empty-state";
import { ErrorState } from "@/components/patterns/error-state";
import { Inspector, InspectorSection } from "@/components/patterns/inspector";
import { KeyValueGrid } from "@/components/patterns/key-value-grid";
import { ListPane, ListRow } from "@/components/patterns/list-pane";
import { SplitView } from "@/components/patterns/split-view";
import { TitlebarToolbar } from "@/components/patterns/titlebar-toolbar";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { IconButton } from "@/components/ui/icon-button";
import { Select } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { type Status, StatusDot } from "@/components/ui/status-dot";
import { ConnectSheet, useAccounts, useActiveAccount } from "@/features/accounts";
import type { TunnelSummary } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { RouteSheet, type SheetMode } from "./components/route-sheet";
import { useTunnelAction, useTunnels } from "./queries";

const cloudStatus: Record<string, { dot: Status; label: string }> = {
  healthy: { dot: "healthy", label: "Connected" },
  degraded: { dot: "warning", label: "Degraded" },
  down: { dot: "error", label: "Down" },
  inactive: { dot: "idle", label: "No connectors" },
};

function statusOf(tunnel: TunnelSummary) {
  return cloudStatus[tunnel.status] ?? { dot: "idle" as const, label: tunnel.status };
}

function formatDate(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.getTime())
    ? "—"
    : date.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

function subtitle(tunnel: TunnelSummary) {
  const parts = [tunnel.thisMac ? "This Mac" : statusOf(tunnel).label];
  if (tunnel.routes !== null) parts.push(`${tunnel.routes} route${tunnel.routes === 1 ? "" : "s"}`);
  return parts.join(" · ");
}

function TunnelInspector({
  tunnel,
  accountId,
  onDelete,
}: {
  tunnel: TunnelSummary;
  accountId: string;
  onDelete: () => void;
}) {
  const action = useTunnelAction(accountId);
  const running = tunnel.connector !== null && tunnel.connector.state !== "stopped";
  const run = (kind: "start" | "stop" | "clean") =>
    action.mutate(
      { action: kind, tunnelId: tunnel.id },
      { onError: (error) => toast.error(toIpcError(error).message) },
    );
  return (
    <Inspector
      title={tunnel.name}
      subtitle={
        <span className="flex items-center gap-1.5">
          <StatusDot status={statusOf(tunnel).dot} label={statusOf(tunnel).label} />
          {statusOf(tunnel).label}
          {tunnel.thisMac ? <Badge>This Mac</Badge> : null}
        </span>
      }
      actions={
        <>
          {tunnel.thisMac ? (
            <Button disabled={action.isPending} onClick={() => run(running ? "stop" : "start")}>
              {running ? "Stop on This Mac" : "Start on This Mac"}
            </Button>
          ) : null}
          {tunnel.connections.length > 0 ? (
            <Button disabled={action.isPending} onClick={() => run("clean")}>
              Clean Up Connections
            </Button>
          ) : null}
          {tunnel.thisMac ? (
            <Button variant="destructive" onClick={onDelete}>
              Delete…
            </Button>
          ) : null}
        </>
      }
    >
      <InspectorSection title="Details">
        <KeyValueGrid
          items={[
            {
              label: "Routes",
              value: tunnel.routes === null ? "Configured locally" : String(tunnel.routes),
            },
            { label: "Created", value: formatDate(tunnel.createdAt) },
            { label: "Tunnel ID", value: tunnel.id, mono: true },
          ]}
        />
      </InspectorSection>
      <InspectorSection title="Connectors">
        {tunnel.connections.length === 0 ? (
          <p className="text-callout text-secondary">No connector is connected.</p>
        ) : (
          <ul className="flex flex-col gap-1.5">
            {tunnel.connections.map((c, index) => (
              <li
                // biome-ignore lint/suspicious/noArrayIndexKey: connections have no stable id here
                key={index}
                className="flex items-baseline gap-2 text-callout"
              >
                <span className="font-mono text-mono uppercase">{c.colo}</span>
                <span className="text-secondary">
                  {c.originIp} · cloudflared {c.version}
                </span>
              </li>
            ))}
          </ul>
        )}
      </InspectorSection>
      {!tunnel.thisMac ? (
        <p className="text-callout text-secondary">
          Teitunnel didn't create this tunnel, so it only shows it. Manage it where it was set up.
        </p>
      ) : null}
    </Inspector>
  );
}

/** Every tunnel in the active account; this Mac's first. */
export function TunnelsPage() {
  const { isSuccess, data: accounts = [] } = useAccounts();
  const active = useActiveAccount();
  const setActive = useUiStore((state) => state.setActiveAccountId);
  const tunnels = useTunnels(active?.id ?? null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [sheet, setSheet] = useState<SheetMode | null>(null);
  const list = tunnels.data ?? [];
  const selected = list.find((t) => t.id === selectedId) ?? list[0] ?? null;

  const toolbar = (
    <TitlebarToolbar title="Tunnels">
      {accounts.length > 1 && active ? (
        <Select
          label="Account"
          options={accounts.map((a) => ({ value: a.id, label: a.name }))}
          value={active.id}
          onValueChange={setActive}
        />
      ) : null}
      {active ? (
        <IconButton
          icon={RefreshCw}
          label="Refresh tunnels"
          onClick={() => void tunnels.refetch()}
          disabled={tunnels.isFetching}
        />
      ) : null}
    </TitlebarToolbar>
  );

  if (isSuccess && !active) {
    return (
      <>
        {toolbar}
        <EmptyState
          icon={Network}
          title="Connect Cloudflare"
          description="Tunnels connect this Mac to Cloudflare so your routes can reach it."
          action={<ConnectSheet trigger={<Button variant="primary">Connect Cloudflare</Button>} />}
        />
      </>
    );
  }

  const body = tunnels.error ? (
    <ErrorState
      title="Couldn't load tunnels"
      message={toIpcError(tunnels.error).message}
      hint={toIpcError(tunnels.error).hint}
      action={<Button onClick={() => void tunnels.refetch()}>Try again</Button>}
    />
  ) : tunnels.isPending ? (
    <div className="flex flex-col gap-2 p-3">
      <Skeleton className="h-11" />
    </div>
  ) : list.length === 0 ? (
    <EmptyState
      icon={Network}
      title="No tunnels"
      description="Teitunnel creates a tunnel for this Mac when you add your first route."
    />
  ) : (
    <SplitView
      id="tunnels"
      list={
        <ListPane
          label="Tunnels"
          items={list}
          getId={(t) => t.id}
          selectedId={selected?.id ?? null}
          onSelect={setSelectedId}
          renderRow={(tunnel) => (
            <ListRow
              title={tunnel.name}
              subtitle={subtitle(tunnel)}
              leading={<StatusDot status={statusOf(tunnel).dot} label={statusOf(tunnel).label} />}
            />
          )}
        />
      }
    >
      {selected && active ? (
        <div className="flex min-h-0 flex-1 flex-col">
          <TunnelInspector
            tunnel={selected}
            accountId={active.id}
            onDelete={() => setSheet({ kind: "removeTunnel" })}
          />
        </div>
      ) : null}
    </SplitView>
  );

  return (
    <>
      {toolbar}
      {body}
      {active ? (
        <RouteSheet accountId={active.id} zones={[]} mode={sheet} onClose={() => setSheet(null)} />
      ) : null}
    </>
  );
}
