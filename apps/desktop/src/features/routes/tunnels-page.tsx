import { Network, Plus, RefreshCw } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { detectPlatform } from "@/app/platform";
import { useUiStore } from "@/app/ui-store";
import { EmptyState } from "@/components/patterns/empty-state";
import { ErrorState } from "@/components/patterns/error-state";
import { Inspector, InspectorSection } from "@/components/patterns/inspector";
import { KeyValueGrid } from "@/components/patterns/key-value-grid";
import { ListPane, ListRow } from "@/components/patterns/list-pane";
import { LogViewer } from "@/components/patterns/log-viewer";
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
import { type MessageKey, t } from "@/lib/i18n";
import type { ConnectorView, ForeignConnector, TunnelSummary } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { NetworksSection } from "./components/networks-section";
import { RemoteLogsSheet } from "./components/remote-logs-sheet";
import { RouteSheet, type SheetMode } from "./components/route-sheet";
import {
  useAlwaysOn,
  useForeignConnectors,
  useSaveLog,
  useSetAlwaysOn,
  useStopForeign,
  useTunnelAction,
  useTunnelLogs,
  useTunnels,
} from "./queries";
import { TunnelTraffic } from "./tunnel-traffic";

type Entry =
  | { kind: "tunnel"; tunnel: TunnelSummary }
  | { kind: "foreign"; process: ForeignConnector };

const entryId = (e: Entry) => (e.kind === "tunnel" ? e.tunnel.id : `pid-${e.process.pid}`);

function foreignTitle(process: ForeignConnector) {
  switch (process.mode.type) {
    case "quickTunnel":
      return t("tunnels.foreign.quick", {
        origin: process.mode.origin.replace(/^https?:\/\//, ""),
      });
    case "named":
      return process.mode.tunnel
        ? t("tunnels.foreign.named", { name: process.mode.tunnel })
        : t("tunnels.foreign.namedUnknown");
    default:
      return "cloudflared";
  }
}

function foreignStatus(process: ForeignConnector): { dot: Status; label: string } {
  if (process.connections === null) return { dot: "idle", label: t("tunnels.foreign.running") };
  return process.connections > 0
    ? { dot: "healthy", label: t("tunnels.foreign.connections", { count: process.connections }) }
    : { dot: "warning", label: t("tunnels.foreign.notConnected") };
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
            <Button variant="destructive">{t("tunnels.foreign.stop")}</Button>
          </DialogTrigger>
          <DialogContent
            title={t("tunnels.foreign.stopTitle")}
            description={
              process.service ? t("tunnels.foreign.stopService") : t("tunnels.foreign.stopProcess")
            }
            footer={
              <>
                <DialogClose asChild>
                  <Button>{t("common.cancel")}</Button>
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
                    {t("tunnels.foreign.stopConfirm")}
                  </Button>
                </DialogClose>
              </>
            }
          />
        </Dialog>
      }
    >
      <p className="text-callout text-secondary">{t("tunnels.foreign.onlyShown")}</p>
      <InspectorSection title={t("tunnels.details")}>
        <KeyValueGrid
          items={[
            { label: t("tunnels.foreign.process"), value: String(process.pid), mono: true },
            {
              label: t("tunnels.foreign.startedBy"),
              value: process.service
                ? t("tunnels.foreign.byService")
                : t("tunnels.foreign.byTerminal"),
            },
            ...(process.metrics
              ? [{ label: t("tunnels.foreign.metrics"), value: process.metrics, mono: true }]
              : []),
          ]}
        />
      </InspectorSection>
      <InspectorSection title={t("tunnels.foreign.command")}>
        <p className="selectable break-all font-mono text-mono text-secondary">{process.command}</p>
      </InspectorSection>
    </Inspector>
  );
}

const cloudStatus: Record<string, { dot: Status; label: MessageKey }> = {
  healthy: { dot: "healthy", label: "tunnels.status.healthy" },
  degraded: { dot: "warning", label: "tunnels.status.degraded" },
  down: { dot: "error", label: "tunnels.status.down" },
  inactive: { dot: "idle", label: "tunnels.status.inactive" },
};

function statusOf(tunnel: TunnelSummary) {
  const status = cloudStatus[tunnel.status];
  return status
    ? { dot: status.dot, label: t(status.label) }
    : { dot: "idle" as const, label: tunnel.status };
}

function formatDate(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.getTime())
    ? "—"
    : date.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

function subtitle(tunnel: TunnelSummary) {
  const parts = [tunnel.thisMac ? t("tunnels.thisMac") : statusOf(tunnel).label];
  if (tunnel.routes !== null) parts.push(t("tunnels.routes", { count: tunnel.routes }));
  return parts.join(" · ");
}

/** One machine running the tunnel: where it connects from and to. */
function ConnectorRow({
  connector,
  onLogs,
}: {
  connector: ConnectorView;
  onLogs: (() => void) | null;
}) {
  const colos = connector.connections.map((c) => c.colo.toUpperCase()).join(", ");
  return (
    <li className="flex items-center gap-3">
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-1.5 text-body">
          <span className="selectable truncate">
            {connector.originIp || t("tunnels.unknownAddress")}
          </span>
          {connector.thisMac ? <Badge>{t("tunnels.thisMac")}</Badge> : null}
        </div>
        <div className="truncate text-callout text-secondary">
          cloudflared {connector.version} · {colos}
        </div>
      </div>
      {onLogs ? <Button onClick={onLogs}>{t("tunnels.logs")}</Button> : null}
    </li>
  );
}

function TunnelInspector({
  tunnel,
  accountId,
  onDelete,
  onConnectorLogs,
  onSheet,
}: {
  tunnel: TunnelSummary;
  accountId: string;
  onDelete: () => void;
  onConnectorLogs: (connector: ConnectorView) => void;
  onSheet: (mode: SheetMode) => void;
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
          {tunnel.thisMac ? <Badge>{t("tunnels.thisMac")}</Badge> : null}
        </span>
      }
      actions={
        <>
          {tunnel.thisMac ? (
            <Button disabled={action.isPending} onClick={() => run(running ? "stop" : "start")}>
              {running ? t("tunnels.stopHere") : t("tunnels.startHere")}
            </Button>
          ) : null}
          {tunnel.connectors.length > 0 ? (
            <Button disabled={action.isPending} onClick={() => run("clean")}>
              {t("tunnels.clean")}
            </Button>
          ) : null}
          {tunnel.thisMac ? (
            <Button variant="destructive" onClick={onDelete}>
              {t("tunnels.delete")}
            </Button>
          ) : null}
        </>
      }
    >
      <InspectorSection title={t("tunnels.details")}>
        <KeyValueGrid
          items={[
            {
              label: t("tunnels.detail.routes"),
              value: tunnel.routes === null ? t("tunnels.detail.local") : String(tunnel.routes),
            },
            { label: t("tunnels.detail.created"), value: formatDate(tunnel.createdAt) },
            { label: t("tunnels.detail.id"), value: tunnel.id, mono: true },
          ]}
        />
      </InspectorSection>
      <InspectorSection title={t("tunnels.connectors")}>
        {tunnel.connectors.length === 0 ? (
          <p className="text-callout text-secondary">{t("tunnels.noConnector")}</p>
        ) : (
          <ul className="flex flex-col gap-2.5">
            {tunnel.connectors.map((connector) => (
              <ConnectorRow
                key={connector.id}
                connector={connector}
                // This Mac's own logs are shown below; others stream through Cloudflare.
                onLogs={connector.thisMac ? null : () => onConnectorLogs(connector)}
              />
            ))}
          </ul>
        )}
      </InspectorSection>
      {tunnel.isDefault ? (
        <NetworksSection
          accountId={accountId}
          onAdd={() => onSheet({ kind: "addNetwork" })}
          onRemove={(network) => onSheet({ kind: "removeNetwork", network })}
        />
      ) : null}
      {tunnel.thisMac ? <AlwaysOnRow accountId={accountId} tunnelId={tunnel.id} /> : null}
      {tunnel.thisMac ? <TunnelTraffic tunnelId={tunnel.id} /> : null}
      {tunnel.thisMac ? <TunnelLogs tunnelId={tunnel.id} /> : null}
      {!tunnel.thisMac ? (
        <p className="text-callout text-secondary">{t("tunnels.notOurs")}</p>
      ) : null}
    </Inspector>
  );
}

/** "Keep running when Teitunnel quits": moves the connector to a launchd agent. */
function AlwaysOnRow({ accountId, tunnelId }: { accountId: string; tunnelId: string }) {
  const mode = useAlwaysOn(accountId, tunnelId);
  const change = useSetAlwaysOn(accountId, tunnelId);
  if (!mode.data?.supported) return null;
  const enabled = change.isPending ? change.variables : mode.data.enabled;
  return (
    <InspectorSection title={t("tunnels.running.title")}>
      <div className="flex items-start gap-3">
        <div className="min-w-0 flex-1">
          <label htmlFor="always-on" className="text-body">
            {t("tunnels.running.keep")}
          </label>
          <p className="text-callout text-secondary">
            {change.isPending
              ? t("tunnels.running.switching")
              : detectPlatform() === "linux"
                ? t("tunnels.running.linux")
                : t("tunnels.running.detail")}
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

function TunnelLogs({ tunnelId }: { tunnelId: string }) {
  const lines = useTunnelLogs(tunnelId, true).data ?? [];
  const save = useSaveLog();
  return (
    <InspectorSection title={t("tunnels.logs")}>
      <LogViewer lines={lines} empty={t("tunnels.logsEmpty")} onSave={save} />
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
  const [remote, setRemote] = useState<{ tunnelId: string; connector: ConnectorView } | null>(null);
  const foreign = useForeignConnectors(true);
  const list: Entry[] = [
    ...(tunnels.data ?? []).map((tunnel): Entry => ({ kind: "tunnel", tunnel })),
    ...(foreign.data ?? []).map((process): Entry => ({ kind: "foreign", process })),
  ];
  const selected = list.find((e) => entryId(e) === selectedId) ?? list[0] ?? null;

  const toolbar = (
    <TitlebarToolbar title={t("tunnels.title")}>
      {accounts.length > 1 && active ? (
        <Select
          label={t("common.account")}
          options={accounts.map((a) => ({ value: a.id, label: a.name }))}
          value={active.id}
          onValueChange={setActive}
        />
      ) : null}
      {active ? (
        <IconButton
          icon={Plus}
          label={t("tunnels.new")}
          onClick={() => setSheet({ kind: "createTunnel" })}
        />
      ) : null}
      {active ? (
        <IconButton
          icon={RefreshCw}
          label={t("tunnels.refresh")}
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
          title={t("routes.connectCloudflare.title")}
          description={t("tunnels.connectDescription")}
          action={
            <ConnectSheet
              trigger={<Button variant="primary">{t("routes.connectCloudflare.title")}</Button>}
            />
          }
        />
      </>
    );
  }

  const body = tunnels.error ? (
    <ErrorState
      title={t("tunnels.loadFailed")}
      message={toIpcError(tunnels.error).message}
      hint={toIpcError(tunnels.error).hint}
      action={<Button onClick={() => void tunnels.refetch()}>{t("common.tryAgain")}</Button>}
    />
  ) : tunnels.isPending ? (
    <div className="flex flex-col gap-2 p-3">
      <Skeleton className="h-11" />
    </div>
  ) : list.length === 0 ? (
    <EmptyState
      icon={Network}
      title={t("tunnels.empty.title")}
      description={t("tunnels.empty.description")}
      action={
        active ? (
          <Button onClick={() => setSheet({ kind: "addNetwork" })}>
            {t("tunnels.empty.share")}
          </Button>
        ) : undefined
      }
    />
  ) : (
    <SplitView
      id="tunnels"
      list={
        <ListPane
          label={t("tunnels.list")}
          items={list}
          getId={entryId}
          groupOf={(e) =>
            e.kind === "tunnel" ? t("tunnels.group.cloud") : t("tunnels.group.local")
          }
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
                subtitle={t("tunnels.foreign.notManaged", { pid: String(entry.process.pid) })}
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
            onDelete={() => setSheet({ kind: "removeTunnel", tunnelId: selected.tunnel.id })}
            onSheet={setSheet}
            onConnectorLogs={(connector) => setRemote({ tunnelId: selected.tunnel.id, connector })}
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
        <>
          <RouteSheet
            accountId={active.id}
            zones={[]}
            mode={sheet}
            onClose={() => setSheet(null)}
          />
          <RemoteLogsSheet
            target={
              remote
                ? {
                    accountId: active.id,
                    tunnelId: remote.tunnelId,
                    connectorId: remote.connector.id,
                    connector: remote.connector,
                  }
                : null
            }
            onClose={() => setRemote(null)}
          />
        </>
      ) : null}
    </>
  );
}
