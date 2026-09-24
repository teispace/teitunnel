import {
  ExternalLink,
  FileInput,
  FileOutput,
  LockKeyhole,
  Pencil,
  Plus,
  RefreshCw,
  Trash2,
  Waypoints,
} from "lucide-react";
import { useEffect, useState } from "react";
import { useUiStore } from "@/app/ui-store";
import { CopyField } from "@/components/patterns/copy-field";
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
import { IconButton } from "@/components/ui/icon-button";
import { Select } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { StatusDot } from "@/components/ui/status-dot";
import { ConnectSheet, useAccounts, useActiveAccount } from "@/features/accounts";
import { summaryOf } from "@/features/activity/model";
import { RouteAnalytics } from "@/features/analytics";
import { CheckNotes, HostRejectionFix, useSendHostOnRoute } from "@/features/dev-server";
import { IssueCallout, routeIssues } from "@/features/doctor";
import { useIssues } from "@/features/doctor/queries";
import {
  ProtectionSection,
  ProtectionSheet,
  ServiceTokens,
  useProtection,
} from "@/features/protection";
import { relativeTime } from "@/lib/format";
import { type MessageKey, t, translate } from "@/lib/i18n";
import type { ClientAccess, RouteView, TunnelView, Verification } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { openUrl } from "@/lib/open-url";
import { useManualRefetch } from "@/lib/use-manual-refetch";
import { describeAllowed } from "./access";
import { BalanceHealth } from "./components/balance-health";
import { DriftBanner } from "./components/drift-banner";
import { ExportSheet } from "./components/export-sheet";
import { ImportSheet } from "./components/import-sheet";
import { RouteSheet, type SheetMode } from "./components/route-sheet";
import {
  useActivity,
  useLocalSetups,
  useRouteLogs,
  useRoutesOverview,
  useSaveLog,
  useVerify,
} from "./queries";
import { connectorStatus, routeStatus } from "./status";

const routeKey = (route: RouteView) => `${route.hostname}${route.path ?? ""}`;

function displayOrigin(origin: string) {
  return origin.replace(/^http:\/\/localhost:/, "localhost:");
}

const clientApps: Record<ClientAccess["protocol"], MessageKey> = {
  ssh: "routes.client.app.ssh",
  rdp: "routes.client.app.rdp",
  smb: "routes.client.app.smb",
  tcp: "routes.client.app.tcp",
};

/** "Then point the app at `localhost:5432`.", with the address in monospace. */
function ThenConnect({ app, address }: { app: string; address: string }) {
  const [before, after] = t("routes.connect.then", { app, address: "\u0000" }).split("\u0000");
  return (
    <p className="text-callout text-secondary">
      {before}
      <span className="selectable font-mono text-mono text-primary">{address}</span>
      {after}
    </p>
  );
}

/** How visitors reach an SSH, RDP, SMB or TCP route: through cloudflared on their side. */
function ConnectSection({ client }: { client: ClientAccess }) {
  return (
    <InspectorSection title={t("routes.connect.title")}>
      <p className="text-callout text-secondary">
        {client.protocol === "ssh" ? t("routes.connect.ssh") : t("routes.connect.other")}
      </p>
      <CopyField label={t("routes.connect.command")} value={client.command} />
      {client.localAddress ? (
        <ThenConnect app={t(clientApps[client.protocol])} address={client.localAddress} />
      ) : null}
      {client.sshConfig ? (
        <>
          <p className="text-callout text-secondary">{t("routes.connect.sshConfigHint")}</p>
          <CopyField label={t("routes.connect.sshConfig")} value={client.sshConfig} multiline />
        </>
      ) : null}
    </InspectorSection>
  );
}

function TestResult({
  result,
  route,
  accountId,
  onTest,
  testing,
}: {
  result: Verification;
  route: RouteView;
  accountId: string;
  /** Tests again (waiting for the change to reach the connector). */
  onTest: () => void;
  testing: boolean;
}) {
  const sendHost = useSendHostOnRoute(accountId);
  if (result.failure?.type === "hostRejected") {
    return (
      <HostRejectionFix
        rejection={result.failure.rejection}
        via="route"
        onSendHost={(host) => sendHost.mutate({ route, host }, { onSuccess: onTest })}
        sending={sendHost.isPending}
        onCheck={onTest}
        checking={testing}
      />
    );
  }
  return result.failure ? (
    <>
      <p role="status" className="text-callout text-warning">
        {result.message ? translate(result.message) : null}
      </p>
      <CheckNotes check={result} showMessage={false} onCheck={onTest} checking={testing} />
    </>
  ) : (
    <p role="status" className="text-callout text-healthy">
      {result.protected
        ? t("routes.test.protected")
        : result.status
          ? t("routes.test.worksStatus", { status: String(result.status) })
          : t("routes.test.works")}
    </p>
  );
}

function RouteInspector({
  route,
  tunnel,
  accountId,
  onEdit,
  onRemove,
  onBalance,
  localTunnelIds,
}: {
  route: RouteView;
  tunnel: TunnelView | null;
  accountId: string;
  /** This machine's tunnels. */
  localTunnelIds: string[];
  onEdit: () => void;
  onRemove: () => void;
  /** Start or stop load balancing the route. */
  onBalance: () => void;
}) {
  const { issues } = useIssues();
  const status = routeStatus(route, tunnel, issues);
  const url = `https://${route.hostname}`;
  const test = useVerify(accountId);
  const activity = useActivity(accountId);
  // A test result belongs to the route it was run for.
  // biome-ignore lint/correctness/useExhaustiveDependencies: reset when the route changes
  useEffect(() => test.reset(), [route.hostname]);
  const history = (activity.data ?? []).filter((e) =>
    // Older entries only have a summary to go by.
    e.record ? e.record.hostnames.includes(route.hostname) : e.summary.includes(route.hostname),
  );

  return (
    <Inspector
      title={route.hostname}
      subtitle={
        <span className="flex items-center gap-1.5">
          <StatusDot status={status.dot} label={status.label} /> {status.label}
        </span>
      }
      actions={
        <>
          {route.client ? null : (
            <>
              <Button onClick={() => void openUrl(url)}>
                {t("common.open")} <ExternalLink />
              </Button>
              <Button
                disabled={test.isPending}
                onClick={() => test.mutate({ hostname: route.hostname, wait: false })}
              >
                {test.isPending ? t("routes.test.testing") : t("routes.test.test")}
              </Button>
            </>
          )}
          {route.client ? null : (
            <Button onClick={onBalance}>
              {route.balanced ? t("routes.balance.stop") : t("routes.balance.start")}
            </Button>
          )}
          <IconButton icon={Pencil} label={t("routes.edit")} variant="secondary" onClick={onEdit} />
          <IconButton
            icon={Trash2}
            label={t("routes.remove")}
            variant="secondary"
            onClick={onRemove}
          />
        </>
      }
    >
      {routeIssues(issues, route.hostname).map((issue) => (
        <IssueCallout key={issue.id} issue={issue} />
      ))}
      {test.data ? (
        <TestResult
          result={test.data}
          route={route}
          accountId={accountId}
          onTest={() => test.mutate({ hostname: route.hostname, wait: true })}
          testing={test.isPending}
        />
      ) : null}
      {test.error ? (
        <p role="alert" className="text-callout text-error">
          {toIpcError(test.error).message}
        </p>
      ) : null}
      {route.client ? (
        <ConnectSection client={route.client} />
      ) : (
        <InspectorSection title={t("routes.inspector.address")}>
          <CopyField label={t("common.url")} value={url} />
        </InspectorSection>
      )}
      <InspectorSection title={t("routes.inspector.details")}>
        <KeyValueGrid
          items={[
            { label: t("routes.detail.service"), value: displayOrigin(route.origin), mono: true },
            ...(route.path
              ? [{ label: t("routes.detail.path"), value: route.path, mono: true }]
              : []),
            { label: t("routes.detail.domain"), value: route.zone ?? "—" },
            ...(route.balanced
              ? [{ label: t("routes.detail.balancing"), value: t("routes.balance.on") }]
              : []),
            {
              label: t("routes.detail.login"),
              value: route.access ? describeAllowed(route.access) : t("routes.detail.noLogin"),
            },
            {
              label: t("routes.detail.dns"),
              value:
                route.dns.state === "ok"
                  ? t("routes.detail.dnsOk")
                  : route.dns.state === "missing"
                    ? t("routes.detail.dnsMissing")
                    : t("routes.detail.dnsElsewhere", { content: route.dns.content }),
            },
            {
              label: t("routes.detail.connector"),
              value: connectorStatus(tunnel?.connector ?? null).label,
            },
            ...(route.local
              ? []
              : [{ label: t("routes.detail.note"), value: t("routes.detail.remote") }]),
          ]}
        />
      </InspectorSection>
      {route.balanced ? (
        <BalanceHealth
          accountId={accountId}
          hostname={route.hostname}
          localTunnelIds={localTunnelIds}
        />
      ) : null}
      {route.client ? null : <ProtectionSection accountId={accountId} hostname={route.hostname} />}
      {route.client ? null : <ServiceTokens accountId={accountId} hostname={route.hostname} />}
      {route.client ? null : <RouteAnalytics accountId={accountId} route={route} />}
      {route.local && tunnel ? (
        <RouteLogs accountId={accountId} hostname={route.hostname} path={route.path} />
      ) : null}
      {history.length > 0 ? (
        <InspectorSection title={t("routes.inspector.activity")}>
          <ul className="flex flex-col gap-1.5">
            {history.slice(0, 5).map((entry) => (
              <li key={entry.id} className="flex flex-col text-callout">
                <span
                  className={
                    entry.outcome === "applied" || entry.outcome === "resolved"
                      ? ""
                      : "text-warning"
                  }
                >
                  {summaryOf(entry)}
                </span>
                <span className="text-secondary">
                  {relativeTime(entry.at)}
                  {entry.outcome === "applied" || entry.record?.kind === "alert"
                    ? ""
                    : t("routes.activity.undone")}
                </span>
              </li>
            ))}
          </ul>
        </InspectorSection>
      ) : null}
    </Inspector>
  );
}

/** Routes of this Mac's tunnel in the active Cloudflare account. */
export function RoutesPage({
  adding = false,
  focus,
}: {
  adding?: boolean;
  /** A hostname to select first (links and the control connection). */
  focus?: string | undefined;
}) {
  const { isSuccess } = useAccounts();
  const { data: accounts = [] } = useAccounts();
  const active = useActiveAccount();
  const setActive = useUiStore((state) => state.setActiveAccountId);
  const overview = useRoutesOverview(active?.id ?? null);
  const reload = useManualRefetch(overview.refetch);
  const { issues } = useIssues();
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [sheet, setSheet] = useState<SheetMode | null>(null);
  /** The hostname whose edge protection is being edited from the route sheet. */
  const [protecting, setProtecting] = useState<string | null>(null);
  const protection = useProtection(active?.id ?? "", protecting ?? "", protecting !== null);
  const [importing, setImporting] = useState(false);
  const [exporting, setExporting] = useState(false);
  const setups = useLocalSetups(active !== null);
  const importable = (setups.data ?? []).some((s) => s.routes.some((r) => !r.unsupported));
  const routes = overview.data?.routes ?? [];
  const tunnel = overview.data?.tunnel ?? null;
  const tunnels = overview.data?.tunnels ?? [];
  /** The tunnel carrying a route (its connector decides whether it's live). */
  const carrier = (route: RouteView) => tunnels.find((t) => t.id === route.tunnelId) ?? tunnel;
  const zones = overview.data?.zones ?? [];
  const selected =
    routes.find((r) => routeKey(r) === selectedKey) ??
    routes.find((r) => r.hostname === focus?.toLowerCase()) ??
    routes[0] ??
    null;

  // ⌘N / "New route" opens the sheet once the account's domains are known.
  useEffect(() => {
    if (adding && overview.isSuccess) setSheet({ kind: "add" });
  }, [adding, overview.isSuccess]);

  const toolbar = (
    <TitlebarToolbar title={t("routes.title")}>
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
          icon={RefreshCw}
          label={t("routes.refresh")}
          onClick={reload.refresh}
          pending={reload.refreshing}
        />
      ) : null}
      {active && overview.isSuccess && importable ? (
        <IconButton
          icon={FileInput}
          label={t("routes.import")}
          onClick={() => setImporting(true)}
        />
      ) : null}
      {active && overview.data?.tunnel ? (
        <IconButton
          icon={FileOutput}
          label={t("routes.export")}
          onClick={() => setExporting(true)}
        />
      ) : null}
      {active && overview.isSuccess ? (
        <IconButton
          icon={Plus}
          label={t("routes.new")}
          disabled={zones.length === 0}
          onClick={() => setSheet({ kind: "add" })}
        />
      ) : null}
    </TitlebarToolbar>
  );

  if (isSuccess && !active) {
    return (
      <>
        {toolbar}
        <EmptyState
          icon={Waypoints}
          title={t("routes.connectCloudflare.title")}
          description={t("routes.connectCloudflare.description")}
          action={
            <ConnectSheet
              trigger={<Button variant="primary">{t("routes.connectCloudflare.title")}</Button>}
            />
          }
        />
      </>
    );
  }

  const body = (() => {
    if (overview.error) {
      const error = toIpcError(overview.error);
      return (
        <ErrorState
          title={t("routes.loadFailed")}
          message={error.message}
          hint={error.hint}
          action={<Button onClick={() => void overview.refetch()}>{t("common.tryAgain")}</Button>}
        />
      );
    }
    if (overview.isPending) {
      return (
        <div className="flex flex-col gap-2 p-3">
          <Skeleton className="h-11" />
          <Skeleton className="h-11" />
        </div>
      );
    }
    if (routes.length === 0) {
      return (
        <EmptyState
          icon={Waypoints}
          title={t("routes.empty.title")}
          description={
            zones.length === 0 ? t("routes.empty.noDomains") : t("routes.empty.description")
          }
          action={
            zones.length > 0 ? (
              <Button variant="primary" onClick={() => setSheet({ kind: "add" })}>
                {t("routes.empty.add")}
              </Button>
            ) : undefined
          }
        />
      );
    }
    return (
      <SplitView
        id="routes"
        list={
          <ListPane
            label={t("routes.list")}
            items={routes}
            getId={routeKey}
            groupOf={(route) => route.zone ?? t("routes.otherZone")}
            selectedId={selected ? routeKey(selected) : null}
            onSelect={setSelectedKey}
            renderRow={(route) => {
              const via = carrier(route);
              const status = routeStatus(route, via, issues);
              return (
                <ListRow
                  title={route.path ? `${route.hostname} ${route.path}` : route.hostname}
                  subtitle={
                    tunnels.length > 1 && via
                      ? t("routes.viaTunnel", {
                          origin: displayOrigin(route.origin),
                          tunnel: via.name,
                        })
                      : `→ ${displayOrigin(route.origin)}`
                  }
                  leading={<StatusDot status={status.dot} label={status.label} />}
                  trailing={
                    route.access || route.temporary || route.balanced ? (
                      <span className="flex items-center gap-1.5">
                        {route.temporary ? <Badge>{t("routes.temporary")}</Badge> : null}
                        {route.balanced ? <Badge>{t("routes.balanced")}</Badge> : null}
                        {route.access ? (
                          <LockKeyhole
                            role="img"
                            aria-label={t("routes.requiresLogin")}
                            className="size-3.5"
                            strokeWidth={1.75}
                          />
                        ) : null}
                      </span>
                    ) : null
                  }
                />
              );
            }}
          />
        }
      >
        {selected && active ? (
          <div className="flex min-h-0 flex-1 flex-col">
            <RouteInspector
              route={selected}
              tunnel={carrier(selected)}
              accountId={active.id}
              localTunnelIds={tunnels.map((t) => t.id)}
              onEdit={() => setSheet({ kind: "edit", route: selected })}
              onRemove={() => setSheet({ kind: "remove", route: selected })}
              onBalance={() =>
                setSheet({ kind: selected.balanced ? "unbalance" : "balance", route: selected })
              }
            />
          </div>
        ) : null}
      </SplitView>
    );
  })();

  return (
    <>
      {toolbar}
      <div className="flex min-h-0 flex-1 flex-col">
        {active ? (
          <DriftBanner
            accountId={active.id}
            onRestore={(tunnelId) => setSheet({ kind: "restore", tunnelId })}
          />
        ) : null}
        {body}
      </div>
      {active ? (
        <RouteSheet
          accountId={active.id}
          zones={zones}
          tunnels={tunnels}
          mode={sheet}
          onClose={() => setSheet(null)}
          onEditProtection={(hostname) => {
            setSheet(null);
            setProtecting(hostname);
          }}
        />
      ) : null}
      {active && protecting ? (
        <ProtectionSheet
          accountId={active.id}
          hostname={protecting}
          current={protection.data}
          open={protection.isSuccess}
          onClose={() => setProtecting(null)}
        />
      ) : null}
      {active ? (
        <ExportSheet accountId={active.id} open={exporting} onClose={() => setExporting(false)} />
      ) : null}
      <ImportSheet
        open={importing}
        setups={setups.data ?? []}
        zones={zones}
        existing={routes}
        onClose={() => setImporting(false)}
        onReview={(chosen) => {
          setImporting(false);
          setSheet({
            kind: "fix",
            label: t("routes.importLabel"),
            change: {
              type: "importRoutes",
              routes: chosen.map((r) => ({
                hostname: r.hostname,
                path: r.path,
                origin: r.service,
              })),
            },
          });
        }}
      />
    </>
  );
}

function RouteLogs({
  accountId,
  hostname,
  path,
}: {
  accountId: string;
  hostname: string;
  path: string | null;
}) {
  const lines = useRouteLogs(accountId, hostname, path).data ?? [];
  const save = useSaveLog();
  return (
    <InspectorSection title={t("routes.inspector.logs")}>
      <LogViewer lines={lines} height={160} onSave={save} empty={t("routes.logs.empty")} />
    </InspectorSection>
  );
}
