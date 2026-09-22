import { ExternalLink, FileInput, Pencil, Plus, RefreshCw, Trash2, Waypoints } from "lucide-react";
import { useEffect, useState } from "react";
import { useUiStore } from "@/app/ui-store";
import { CopyField } from "@/components/patterns/copy-field";
import { EmptyState } from "@/components/patterns/empty-state";
import { ErrorState } from "@/components/patterns/error-state";
import { Inspector, InspectorSection } from "@/components/patterns/inspector";
import { KeyValueGrid } from "@/components/patterns/key-value-grid";
import { ListPane, ListRow } from "@/components/patterns/list-pane";
import { SplitView } from "@/components/patterns/split-view";
import { TitlebarToolbar } from "@/components/patterns/titlebar-toolbar";
import { Button } from "@/components/ui/button";
import { IconButton } from "@/components/ui/icon-button";
import { Select } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { type Status, StatusDot } from "@/components/ui/status-dot";
import { ConnectSheet, useAccounts, useActiveAccount } from "@/features/accounts";
import type { ConnectorState, RouteView, TunnelView, Verification } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { openUrl } from "@/lib/open-url";
import { DriftBanner } from "./components/drift-banner";
import { ImportSheet } from "./components/import-sheet";
import { RouteSheet, type SheetMode } from "./components/route-sheet";
import { useActivity, useLocalSetups, useRoutesOverview, useVerify } from "./queries";

const routeKey = (route: RouteView) => `${route.hostname}${route.path ?? ""}`;

function connectorStatus(state: ConnectorState | null): { dot: Status; label: string } {
  switch (state?.state) {
    case "healthy":
      return { dot: "healthy", label: "Connected" };
    case "starting":
    case "connecting":
      return { dot: "connecting", label: "Connecting" };
    case "degraded":
      return { dot: "warning", label: "Connection lost" };
    case "crashed":
      return { dot: "connecting", label: "Restarting" };
    case "crashLoop":
      return { dot: "error", label: "Keeps stopping" };
    default:
      return { dot: "idle", label: "Connector stopped" };
  }
}

/** One dot and label per route: the worst of its DNS and this Mac's connector. */
export function routeStatus(
  route: RouteView,
  tunnel: TunnelView | null,
): { dot: Status; label: string } {
  if (route.dns.state === "missing") return { dot: "warning", label: "No DNS record" };
  if (route.dns.state === "elsewhere") return { dot: "warning", label: "DNS points elsewhere" };
  const connector = connectorStatus(tunnel?.connector ?? null);
  return connector.dot === "healthy" ? { dot: "healthy", label: "Live" } : connector;
}

function displayOrigin(origin: string) {
  return origin.replace(/^http:\/\/localhost:/, "localhost:");
}

function relativeTime(at: number | null) {
  if (at === null) return "";
  const minutes = Math.round((Date.now() - at) / 60_000);
  if (minutes < 1) return "Just now";
  if (minutes < 60) return `${minutes} min ago`;
  return new Date(at).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

function TestResult({ result }: { result: Verification }) {
  return result.failure ? (
    <p role="status" className="text-callout text-warning">
      {result.message}
    </p>
  ) : (
    <p role="status" className="text-callout text-healthy">
      Works{result.status ? ` · HTTP ${result.status}` : ""}
    </p>
  );
}

function RouteInspector({
  route,
  tunnel,
  accountId,
  onEdit,
  onRemove,
}: {
  route: RouteView;
  tunnel: TunnelView | null;
  accountId: string;
  onEdit: () => void;
  onRemove: () => void;
}) {
  const status = routeStatus(route, tunnel);
  const url = `https://${route.hostname}`;
  const test = useVerify(accountId);
  const activity = useActivity(accountId);
  // A test result belongs to the route it was run for.
  // biome-ignore lint/correctness/useExhaustiveDependencies: reset when the route changes
  useEffect(() => test.reset(), [route.hostname]);
  const history = (activity.data ?? []).filter((e) => e.summary.includes(route.hostname));

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
          <Button onClick={() => void openUrl(url)}>
            Open <ExternalLink />
          </Button>
          <Button
            disabled={test.isPending}
            onClick={() => test.mutate({ hostname: route.hostname, wait: false })}
          >
            {test.isPending ? "Testing…" : "Test"}
          </Button>
          <IconButton icon={Pencil} label="Edit route" variant="secondary" onClick={onEdit} />
          <IconButton icon={Trash2} label="Remove route" variant="secondary" onClick={onRemove} />
        </>
      }
    >
      {test.data ? <TestResult result={test.data} /> : null}
      {test.error ? (
        <p role="alert" className="text-callout text-error">
          {toIpcError(test.error).message}
        </p>
      ) : null}
      <InspectorSection title="Address">
        <CopyField label="URL" value={url} />
      </InspectorSection>
      <InspectorSection title="Details">
        <KeyValueGrid
          items={[
            { label: "Service", value: displayOrigin(route.origin), mono: true },
            ...(route.path ? [{ label: "Path", value: route.path, mono: true }] : []),
            { label: "Domain", value: route.zone ?? "—" },
            {
              label: "DNS",
              value:
                route.dns.state === "ok"
                  ? "Points to this Mac's tunnel"
                  : route.dns.state === "missing"
                    ? "No record"
                    : `Points to ${route.dns.content}`,
            },
            { label: "Connector", value: connectorStatus(tunnel?.connector ?? null).label },
            ...(route.local ? [] : [{ label: "Note", value: "The service isn't on this Mac" }]),
          ]}
        />
      </InspectorSection>
      {history.length > 0 ? (
        <InspectorSection title="Activity">
          <ul className="flex flex-col gap-1.5">
            {history.slice(0, 5).map((entry) => (
              <li key={entry.id} className="flex flex-col text-callout">
                <span className={entry.outcome === "applied" ? "" : "text-warning"}>
                  {entry.summary}
                </span>
                <span className="text-secondary">
                  {relativeTime(entry.at)}
                  {entry.outcome === "applied" ? "" : " · undone after an error"}
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
export function RoutesPage({ adding = false }: { adding?: boolean }) {
  const { isSuccess } = useAccounts();
  const { data: accounts = [] } = useAccounts();
  const active = useActiveAccount();
  const setActive = useUiStore((state) => state.setActiveAccountId);
  const overview = useRoutesOverview(active?.id ?? null);
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [sheet, setSheet] = useState<SheetMode | null>(null);
  const [importing, setImporting] = useState(false);
  const setups = useLocalSetups(active !== null);
  const importable = (setups.data ?? []).some((s) => s.routes.some((r) => !r.unsupported));
  const routes = overview.data?.routes ?? [];
  const tunnel = overview.data?.tunnel ?? null;
  const zones = overview.data?.zones ?? [];
  const selected = routes.find((r) => routeKey(r) === selectedKey) ?? routes[0] ?? null;

  // ⌘N / "New route" opens the sheet once the account's domains are known.
  useEffect(() => {
    if (adding && overview.isSuccess) setSheet({ kind: "add" });
  }, [adding, overview.isSuccess]);

  const toolbar = (
    <TitlebarToolbar title="Routes">
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
          label="Refresh routes"
          onClick={() => void overview.refetch()}
          disabled={overview.isFetching}
        />
      ) : null}
      {active && overview.isSuccess && importable ? (
        <IconButton
          icon={FileInput}
          label="Import from cloudflared"
          onClick={() => setImporting(true)}
        />
      ) : null}
      {active && overview.isSuccess ? (
        <IconButton
          icon={Plus}
          label="New route"
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
          title="Connect Cloudflare"
          description="Routes send a hostname on your domain, like app.example.com, to a service on this Mac."
          action={<ConnectSheet trigger={<Button variant="primary">Connect Cloudflare</Button>} />}
        />
      </>
    );
  }

  const body = (() => {
    if (overview.error) {
      const error = toIpcError(overview.error);
      return (
        <ErrorState
          title="Couldn't load routes"
          message={error.message}
          hint={error.hint}
          action={<Button onClick={() => void overview.refetch()}>Try again</Button>}
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
          title="No routes yet"
          description={
            zones.length === 0
              ? "Add a domain to your Cloudflare account first. Routes use hostnames on your domains."
              : "A route sends a hostname like app.example.com to a service on this Mac, such as localhost:3000."
          }
          action={
            zones.length > 0 ? (
              <Button variant="primary" onClick={() => setSheet({ kind: "add" })}>
                Add Route
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
            label="Routes"
            items={routes}
            getId={routeKey}
            groupOf={(route) => route.zone ?? "Other"}
            selectedId={selected ? routeKey(selected) : null}
            onSelect={setSelectedKey}
            renderRow={(route) => {
              const status = routeStatus(route, tunnel);
              return (
                <ListRow
                  title={route.path ? `${route.hostname} ${route.path}` : route.hostname}
                  subtitle={`→ ${displayOrigin(route.origin)}`}
                  leading={<StatusDot status={status.dot} label={status.label} />}
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
              tunnel={tunnel}
              accountId={active.id}
              onEdit={() => setSheet({ kind: "edit", route: selected })}
              onRemove={() => setSheet({ kind: "remove", route: selected })}
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
          <DriftBanner accountId={active.id} onRestore={() => setSheet({ kind: "restore" })} />
        ) : null}
        {body}
      </div>
      {active ? (
        <RouteSheet
          accountId={active.id}
          zones={zones}
          mode={sheet}
          onClose={() => setSheet(null)}
        />
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
            label: "Import Routes",
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
