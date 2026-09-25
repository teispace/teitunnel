import { Camera, Plus, RefreshCw } from "lucide-react";
import { useState } from "react";
import { EmptyState } from "@/components/patterns/empty-state";
import { ErrorState } from "@/components/patterns/error-state";
import { ListPane, ListRow } from "@/components/patterns/list-pane";
import { SplitView } from "@/components/patterns/split-view";
import { TitlebarToolbar } from "@/components/patterns/titlebar-toolbar";
import { Button } from "@/components/ui/button";
import { IconButton } from "@/components/ui/icon-button";
import { Skeleton } from "@/components/ui/skeleton";
import { type Status, StatusDot } from "@/components/ui/status-dot";
import { ConnectSheet, useActiveAccount, useDomains } from "@/features/accounts";
import { stripScheme } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { SnapshotView } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { useManualRefetch } from "@/lib/use-manual-refetch";
import { type PublishMode, PublishSheet } from "./components/publish-sheet";
import { SnapshotInspector } from "./components/snapshot-inspector";
import { formatBytes } from "./format";
import { useSnapshots } from "./queries";

function statusOf(snapshot: SnapshotView): { dot: Status; label: string } {
  return snapshot.liveVersion === null
    ? { dot: "warning", label: t("snapshots.status.incomplete") }
    : { dot: "healthy", label: t("snapshots.status.live") };
}

interface SnapshotsPageProps {
  /** Open the publish sheet for this running site (from a Quick Share). */
  capture?: string | undefined;
  /** Open the publish sheet. */
  publish?: boolean;
}

/** Static copies on the user's Cloudflare account that stay online while this computer sleeps. */
export function SnapshotsPage({ capture, publish = false }: SnapshotsPageProps) {
  const active = useActiveAccount();
  const snapshots = useSnapshots();
  const domains = useDomains(active?.id ?? null);
  const reload = useManualRefetch(snapshots.refetch);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [mode, setMode] = useState<PublishMode | null>(null);
  // Opened from a Quick Share (or ⌘-menu): open the sheet once the account is known.
  const [requested, setRequested] = useState(Boolean(capture) || publish);
  if (requested && active) {
    setRequested(false);
    setMode({ kind: "new", accountId: active.id, ...(capture ? { siteUrl: capture } : {}) });
  }
  const list = snapshots.data ?? [];
  const selected = list.find((s) => s.id === selectedId) ?? list[0] ?? null;
  const zones = (domains.data ?? []).map((d) => ({ id: d.id, name: d.name }));
  const zonesOf = (accountId: string) => (accountId === active?.id ? zones : []);
  const openPublish = () => active && setMode({ kind: "new", accountId: active.id });

  const toolbar = (
    <TitlebarToolbar title={t("snapshots.title")}>
      <IconButton
        icon={RefreshCw}
        label={t("snapshots.refresh")}
        onClick={reload.refresh}
        pending={reload.refreshing}
      />
      {active ? (
        <IconButton icon={Plus} label={t("snapshots.publish")} onClick={openPublish} />
      ) : null}
    </TitlebarToolbar>
  );

  const sheet = (
    <PublishSheet
      mode={mode}
      zones={mode?.kind === "update" ? zonesOf(mode.snapshot.accountId) : zones}
      onClose={() => setMode(null)}
      onPublished={(name) => {
        const found = (snapshots.data ?? []).find((s) => s.name === name);
        if (found) setSelectedId(found.id);
      }}
    />
  );

  if (!active && !snapshots.isPending && list.length === 0) {
    return (
      <>
        {toolbar}
        <EmptyState
          icon={Camera}
          title={t("routes.connectCloudflare.title")}
          description={t("snapshots.connectDescription")}
          action={
            <ConnectSheet
              trigger={<Button variant="primary">{t("routes.connectCloudflare.title")}</Button>}
            />
          }
        />
      </>
    );
  }

  if (snapshots.error) {
    const error = toIpcError(snapshots.error);
    return (
      <>
        {toolbar}
        <ErrorState
          title={t("snapshots.loadFailed")}
          message={error.message}
          hint={error.hint}
          action={<Button onClick={() => void snapshots.refetch()}>{t("common.tryAgain")}</Button>}
        />
      </>
    );
  }

  if (!snapshots.isPending && list.length === 0) {
    return (
      <>
        {toolbar}
        <EmptyState
          icon={Camera}
          title={t("snapshots.empty.title")}
          description={t("snapshots.empty.description")}
          action={
            <Button variant="primary" onClick={openPublish}>
              {t("snapshots.publish")}
            </Button>
          }
        />
        {sheet}
      </>
    );
  }

  return (
    <>
      {toolbar}
      <SplitView
        id="snapshots"
        list={
          snapshots.isPending ? (
            <div className="flex flex-col gap-2 p-3">
              <Skeleton className="h-11" />
              <Skeleton className="h-11" />
            </div>
          ) : (
            <ListPane
              label={t("snapshots.list")}
              items={list}
              getId={(snapshot) => snapshot.id}
              selectedId={selected?.id ?? null}
              onSelect={setSelectedId}
              renderRow={(snapshot) => (
                <ListRow
                  title={snapshot.name}
                  subtitle={
                    snapshot.url
                      ? `${stripScheme(snapshot.url)} · ${formatBytes(snapshot.bytes ?? 0)}`
                      : statusOf(snapshot).label
                  }
                  leading={
                    <StatusDot status={statusOf(snapshot).dot} label={statusOf(snapshot).label} />
                  }
                />
              )}
            />
          )
        }
      >
        {selected ? (
          <div className="flex min-h-0 flex-1 flex-col">
            <SnapshotInspector
              snapshot={selected}
              onUpdate={() => setMode({ kind: "update", snapshot: selected })}
            />
          </div>
        ) : (
          <EmptyState title={t("snapshots.noSelection")} description="" />
        )}
      </SplitView>
      {sheet}
    </>
  );
}
