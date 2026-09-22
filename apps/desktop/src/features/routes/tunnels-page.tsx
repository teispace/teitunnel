import { Network, RefreshCw } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { useUiStore } from "@/app/ui-store";
import { EmptyState } from "@/components/patterns/empty-state";
import { ErrorState } from "@/components/patterns/error-state";
import { Inspector, InspectorSection } from "@/components/patterns/inspector";
import { KeyValueGrid } from "@/components/patterns/key-value-grid";
import { ListPane, ListRow } from "@/components/patterns/list-pane";
import { LogViewer } from "@/components/patterns/log-viewer";
import { Sparkline } from "@/components/patterns/sparkline";
import { SplitView } from "@/components/patterns/split-view";
import { TitlebarToolbar } from "@/components/patterns/titlebar-toolbar";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogTrigger } from "@/components/ui/dialog";
import { IconButton } from "@/components/ui/icon-button";
import { Select } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { type Status, StatusDot } from "@/components/ui/status-dot";
import { Switch } from "@/components/ui/switch";
import { ConnectSheet, useAccounts, useActiveAccount } from "@/features/accounts";
import type { ForeignConnector, TunnelSummary } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { RouteSheet, type SheetMode } from "./components/route-sheet";
import {
  useAlwaysOn,
  useForeignConnectors,
  useSetAlwaysOn,
  useStopForeign,
  useTraffic,
  useTunnelAction,
  useTunnelLogs,
  useTunnels,
} from "./queries";

type Entry =
  | { kind: "tunnel"; tunnel: TunnelSummary }
  | { kind: "foreign"; process: ForeignConnector };

const entryId = (e: Entry) => (e.kind === "tunnel" ? e.tunnel.id : `pid-${e.process.pid}`);

function foreignTitle(process: ForeignConnector) {
  switch (process.mode.type) {
    case "quickTunnel":
      return `Quick Tunnel · ${process.mode.origin.replace(/^https?:\/\//, "")}`;
    case "named":
      return process.mode.tunnel ? `Tunnel ${process.mode.tunnel}` : "Named tunnel";
    default:
      return "cloudflared";
  }
}

function foreignStatus(process: ForeignConnector): { dot: Status; label: string } {
  if (process.connections === null) return { dot: "idle", label: "Running" };
  return process.connections > 0
    ? { dot: "healthy", label: `${process.connections} connections` }
    : { dot: "warning", label: "Not connected" };
}

function ForeignInspector({ process }: { process: ForeignConnector }) {
  const stop = useStopForeign();
  const status = foreignStatus(process);
  return (
    <Inspector
      title={foreignTitle(process)}
      subtitle={
        <span className="flex items-center gap-1.5">
          <StatusDot status={status.dot} label={status.label} /> {status.label}
        </span>
      }
      actions={
        <Dialog>
          <DialogTrigger asChild>
            <Button variant="destructive">Stop…</Button>
          </DialogTrigger>
          <DialogContent
            title="Stop this cloudflared?"
            description={
              process.service
                ? "It runs as a background service, which may start it again. Whatever it serves stops answering."
                : "Whatever it serves stops answering. Teitunnel didn't start it, so check nothing else relies on it."
            }
            footer={
              <>
                <DialogClose asChild>
                  <Button>Cancel</Button>
                </DialogClose>
                <DialogClose asChild>
                  <Button
                    variant="destructive"
                    onClick={() =>
                      stop.mutate(process.pid, {
                        onError: (error) => toast.error(toIpcError(error).message),
                      })
                    }
                  >
                    Stop
                  </Button>
                </DialogClose>
              </>
            }
          />
        </Dialog>
      }
    >
      <p className="text-callout text-secondary">
        Teitunnel didn't start this cloudflared, so it only shows it.
      </p>
      <InspectorSection title="Details">
        <KeyValueGrid
          items={[
            { label: "Process", value: String(process.pid), mono: true },
            {
              label: "Started by",
              value: process.service ? "A background service" : "A terminal or app",
            },
            ...(process.metrics ? [{ label: "Metrics", value: process.metrics, mono: true }] : []),
          ]}
        />
      </InspectorSection>
      <InspectorSection title="Command">
        <p className="selectable break-all font-mono text-mono text-secondary">{process.command}</p>
      </InspectorSection>
    </Inspector>
  );
}

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
      {tunnel.thisMac ? <AlwaysOnRow accountId={accountId} /> : null}
      {tunnel.thisMac ? <TunnelTraffic tunnelId={tunnel.id} /> : null}
      {tunnel.thisMac ? <TunnelLogs tunnelId={tunnel.id} /> : null}
      {!tunnel.thisMac ? (
        <p className="text-callout text-secondary">
          Teitunnel didn't create this tunnel, so it only shows it. Manage it where it was set up.
        </p>
      ) : null}
    </Inspector>
  );
}

/** "Keep running when Teitunnel quits": moves the connector to a launchd agent. */
function AlwaysOnRow({ accountId }: { accountId: string }) {
  const mode = useAlwaysOn(accountId);
  const change = useSetAlwaysOn(accountId);
  if (!mode.data?.supported) return null;
  const enabled = change.isPending ? change.variables : mode.data.enabled;
  return (
    <InspectorSection title="Running">
      <div className="flex items-start gap-3">
        <div className="min-w-0 flex-1">
          <label htmlFor="always-on" className="text-body">
            Keep running when Teitunnel quits
          </label>
          <p className="text-callout text-secondary">
            {change.isPending
              ? "Switching without dropping connections…"
              : "Routes stay up after you quit the app and start again when you log in."}
          </p>
        </div>
        <Switch
          id="always-on"
          checked={enabled}
          disabled={change.isPending}
          onCheckedChange={(next) =>
            change.mutate(next, { onError: (error) => toast.error(toIpcError(error).message) })
          }
        />
      </div>
    </InspectorSection>
  );
}

function TunnelTraffic({ tunnelId }: { tunnelId: string }) {
  const { data: traffic } = useTraffic(tunnelId, true);
  if (!traffic) return null;
  const recent = traffic.points.slice(-6).reduce((sum, p) => sum + p.requests, 0);
  return (
    <InspectorSection title="Traffic">
      <Sparkline values={traffic.points.map((p) => p.requests)} label="Requests in the last hour" />
      <KeyValueGrid
        items={[
          {
            label: "Last minute",
            value: `${recent.toLocaleString()} request${recent === 1 ? "" : "s"}`,
          },
          {
            label: "Since start",
            value: `${traffic.totalRequests.toLocaleString()} requests, ${traffic.totalErrors.toLocaleString()} failed`,
          },
          { label: "Connections", value: String(traffic.connections) },
          ...(traffic.rttMs !== null
            ? [{ label: "Round trip", value: `${Math.round(traffic.rttMs)} ms` }]
            : []),
          ...(traffic.locations.length > 0
            ? [{ label: "Edge", value: traffic.locations.join(", ").toUpperCase() }]
            : []),
        ]}
      />
    </InspectorSection>
  );
}

function TunnelLogs({ tunnelId }: { tunnelId: string }) {
  const lines = useTunnelLogs(tunnelId, true).data ?? [];
  return (
    <InspectorSection title="Logs">
      <LogViewer lines={lines} empty="The connector hasn't logged anything yet." />
    </InspectorSection>
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
  const foreign = useForeignConnectors(true);
  const list: Entry[] = [
    ...(tunnels.data ?? []).map((tunnel): Entry => ({ kind: "tunnel", tunnel })),
    ...(foreign.data ?? []).map((process): Entry => ({ kind: "foreign", process })),
  ];
  const selected = list.find((e) => entryId(e) === selectedId) ?? list[0] ?? null;

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
          getId={entryId}
          groupOf={(e) => (e.kind === "tunnel" ? "In Cloudflare" : "Also on this Mac")}
          selectedId={selected ? entryId(selected) : null}
          onSelect={setSelectedId}
          renderRow={(entry) =>
            entry.kind === "tunnel" ? (
              <ListRow
                title={entry.tunnel.name}
                subtitle={subtitle(entry.tunnel)}
                leading={
                  <StatusDot
                    status={statusOf(entry.tunnel).dot}
                    label={statusOf(entry.tunnel).label}
                  />
                }
              />
            ) : (
              <ListRow
                title={foreignTitle(entry.process)}
                subtitle={`Not managed · pid ${entry.process.pid}`}
                leading={
                  <StatusDot
                    status={foreignStatus(entry.process).dot}
                    label={foreignStatus(entry.process).label}
                  />
                }
              />
            )
          }
        />
      }
    >
      {selected?.kind === "foreign" ? (
        <div className="flex min-h-0 flex-1 flex-col">
          <ForeignInspector process={selected.process} />
        </div>
      ) : selected && active ? (
        <div className="flex min-h-0 flex-1 flex-col">
          <TunnelInspector
            tunnel={selected.tunnel}
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
