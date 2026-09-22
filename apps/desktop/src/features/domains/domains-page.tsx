import { Globe, Plus, RefreshCw } from "lucide-react";
import { useState } from "react";
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
import { Input } from "@/components/ui/input";
import { Select } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { type Status, StatusDot } from "@/components/ui/status-dot";
import {
  CapabilityList,
  ConnectSheet,
  credentialLabel,
  useAccounts,
  useActiveAccount,
  useDomains,
} from "@/features/accounts";
import type { Domain, DomainStatus } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";

const statuses: Record<DomainStatus, { dot: Status; label: string }> = {
  active: { dot: "healthy", label: "Active" },
  pending: { dot: "warning", label: "Waiting for nameservers" },
  moved: { dot: "error", label: "Nameservers moved away" },
  other: { dot: "idle", label: "Setting up" },
};

function DomainInspector({ domain, accountId }: { domain: Domain; accountId: string }) {
  const status = statuses[domain.status];
  return (
    <Inspector title={domain.name} subtitle={status.label}>
      {domain.status === "pending" ? (
        <InspectorSection title="Finish setup">
          <p className="text-callout text-secondary">
            At your registrar, replace the nameservers with these. Changes can take up to a day to
            be picked up.
          </p>
          {domain.nameServers.map((ns) => (
            <CopyField key={ns} label="nameserver" value={ns} />
          ))}
        </InspectorSection>
      ) : null}
      <InspectorSection title="Details">
        <KeyValueGrid
          items={[
            { label: "Status", value: status.label },
            { label: "Plan", value: domain.plan ?? "—" },
            { label: "Zone ID", value: domain.id, mono: true },
            ...(domain.originalNameServers.length > 0 && domain.status !== "active"
              ? [{ label: "Current NS", value: domain.originalNameServers.join(", "), mono: true }]
              : []),
          ]}
        />
      </InspectorSection>
      <InspectorSection title="Permissions">
        <CapabilityList accountId={accountId} zoneId={domain.id} />
      </InspectorSection>
    </Inspector>
  );
}

/** Domains in the active Cloudflare account. */
export function DomainsPage() {
  const { data: accounts = [], isSuccess } = useAccounts();
  const active = useActiveAccount();
  const setActive = useUiStore((state) => state.setActiveAccountId);
  const domains = useDomains(active?.id ?? null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const all = domains.data ?? [];
  const list = query.trim() ? all.filter((d) => d.name.includes(query.trim().toLowerCase())) : all;
  const selected = list.find((d) => d.id === selectedId) ?? list[0] ?? null;

  const toolbar = (
    <TitlebarToolbar title="Domains">
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
          label="Refresh domains"
          onClick={() => void domains.refetch()}
          disabled={domains.isFetching}
        />
      ) : null}
      {active ? (
        <ConnectSheet trigger={<IconButton icon={Plus} label="Connect another account" />} />
      ) : null}
    </TitlebarToolbar>
  );

  if (isSuccess && !active) {
    return (
      <>
        {toolbar}
        <EmptyState
          icon={Globe}
          title="Connect Cloudflare"
          description="Use your own domains, like app.example.com, for services on this Mac."
          action={<ConnectSheet trigger={<Button variant="primary">Connect Cloudflare</Button>} />}
        />
      </>
    );
  }

  return (
    <>
      {toolbar}
      {domains.error ? (
        <ErrorState
          title="Couldn't load domains"
          message={toIpcError(domains.error).message}
          hint={toIpcError(domains.error).hint}
          action={<Button onClick={() => void domains.refetch()}>Try again</Button>}
        />
      ) : (
        <SplitView
          id="domains"
          list={
            domains.isPending ? (
              <div className="flex flex-col gap-2 p-3">
                <Skeleton className="h-9" />
                <Skeleton className="h-9" />
              </div>
            ) : (
              <div className="flex min-h-0 flex-1 flex-col">
                {all.length > 8 ? (
                  <div className="px-2.5 pt-1 pb-1.5">
                    <Input
                      type="search"
                      aria-label="Filter domains"
                      placeholder="Filter"
                      value={query}
                      onChange={(event) => setQuery(event.target.value)}
                      className="rounded-full"
                    />
                  </div>
                ) : null}
                <ListPane
                  label="Domains"
                  items={list}
                  getId={(domain) => domain.id}
                  selectedId={selected?.id ?? null}
                  onSelect={setSelectedId}
                  renderRow={(domain) => (
                    <ListRow
                      title={domain.name}
                      subtitle={statuses[domain.status].label}
                      leading={
                        <StatusDot
                          status={statuses[domain.status].dot}
                          label={statuses[domain.status].label}
                        />
                      }
                    />
                  )}
                  empty={
                    <EmptyState
                      title={query ? "No matches" : "No domains"}
                      description={
                        query
                          ? `No domain contains “${query}”.`
                          : "Add a domain in your Cloudflare dashboard, then refresh."
                      }
                    />
                  }
                />
              </div>
            )
          }
        >
          {selected && active ? (
            <div className="flex min-h-0 flex-1 flex-col">
              <DomainInspector domain={selected} accountId={active.id} />
            </div>
          ) : (
            <EmptyState
              title="No domain selected"
              description={active ? credentialLabel(active) : ""}
            />
          )}
        </SplitView>
      )}
    </>
  );
}
