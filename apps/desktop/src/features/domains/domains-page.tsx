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
import { t } from "@/lib/i18n";
import type { Domain, DomainStatus } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";

const dots: Record<DomainStatus, Status> = {
  active: "healthy",
  pending: "warning",
  moved: "error",
  other: "idle",
};

const statusOf = (status: DomainStatus) => ({
  dot: dots[status],
  label: t(`domains.status.${status}`),
});

function DomainInspector({ domain, accountId }: { domain: Domain; accountId: string }) {
  const status = statusOf(domain.status);
  return (
    <Inspector title={domain.name} subtitle={status.label}>
      {domain.status === "pending" ? (
        <InspectorSection title={t("domains.finishSetup")}>
          <p className="text-callout text-secondary">{t("domains.finishSetupDetail")}</p>
          {domain.nameServers.map((ns) => (
            <CopyField key={ns} label={t("domains.nameserver")} value={ns} />
          ))}
        </InspectorSection>
      ) : null}
      <InspectorSection title={t("domains.details")}>
        <KeyValueGrid
          items={[
            { label: t("domains.detail.status"), value: status.label },
            { label: t("domains.detail.plan"), value: domain.plan ?? "—" },
            { label: t("domains.detail.zoneId"), value: domain.id, mono: true },
            ...(domain.originalNameServers.length > 0 && domain.status !== "active"
              ? [
                  {
                    label: t("domains.detail.currentNs"),
                    value: domain.originalNameServers.join(", "),
                    mono: true,
                  },
                ]
              : []),
          ]}
        />
      </InspectorSection>
      <InspectorSection title={t("domains.permissions")}>
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
    <TitlebarToolbar title={t("domains.title")}>
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
          label={t("domains.refresh")}
          onClick={() => void domains.refetch()}
          disabled={domains.isFetching}
        />
      ) : null}
      {active ? (
        <ConnectSheet trigger={<IconButton icon={Plus} label={t("domains.connectAnother")} />} />
      ) : null}
    </TitlebarToolbar>
  );

  if (isSuccess && !active) {
    return (
      <>
        {toolbar}
        <EmptyState
          icon={Globe}
          title={t("routes.connectCloudflare.title")}
          description={t("domains.connectDescription")}
          action={
            <ConnectSheet
              trigger={<Button variant="primary">{t("routes.connectCloudflare.title")}</Button>}
            />
          }
        />
      </>
    );
  }

  return (
    <>
      {toolbar}
      {domains.error ? (
        <ErrorState
          title={t("domains.loadFailed")}
          message={toIpcError(domains.error).message}
          hint={toIpcError(domains.error).hint}
          action={<Button onClick={() => void domains.refetch()}>{t("common.tryAgain")}</Button>}
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
                      aria-label={t("domains.filter")}
                      placeholder={t("domains.filterPlaceholder")}
                      value={query}
                      onChange={(event) => setQuery(event.target.value)}
                      className="rounded-full"
                    />
                  </div>
                ) : null}
                <ListPane
                  label={t("domains.list")}
                  items={list}
                  getId={(domain) => domain.id}
                  selectedId={selected?.id ?? null}
                  onSelect={setSelectedId}
                  renderRow={(domain) => (
                    <ListRow
                      title={domain.name}
                      subtitle={statusOf(domain.status).label}
                      leading={
                        <StatusDot
                          status={statusOf(domain.status).dot}
                          label={statusOf(domain.status).label}
                        />
                      }
                    />
                  )}
                  empty={
                    <EmptyState
                      title={query ? t("domains.noMatches") : t("domains.none")}
                      description={
                        query ? t("domains.noMatchesDetail", { query }) : t("domains.noneDetail")
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
              title={t("domains.noSelection")}
              description={active ? credentialLabel(active) : ""}
            />
          )}
        </SplitView>
      )}
    </>
  );
}
