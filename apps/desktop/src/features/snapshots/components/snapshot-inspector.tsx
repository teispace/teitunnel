import { ExternalLink, RotateCcw, Trash2, Upload } from "lucide-react";
import { toast } from "sonner";
import { ConfirmDialog } from "@/components/patterns/confirm-dialog";
import { CopyField } from "@/components/patterns/copy-field";
import { Inspector, InspectorSection } from "@/components/patterns/inspector";
import { KeyValueGrid } from "@/components/patterns/key-value-grid";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { relativeTime } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { SnapshotVersionView, SnapshotView } from "@/lib/ipc/bindings";
import { openUrl } from "@/lib/open-url";
import { formatBytes, protectionLabel, sourceLabel } from "../format";
import { useSnapshotChange, useSnapshotVersions } from "../queries";

function VersionRow({
  snapshot,
  version,
}: {
  snapshot: SnapshotView;
  version: SnapshotVersionView;
}) {
  const change = useSnapshotChange();
  return (
    <li className="flex min-h-8 items-center gap-2 border-inset border-b-hairline py-1.5 last:border-b-0">
      <div className="min-w-0 flex-1">
        <div className="text-body">
          {t("snapshots.version.label", { number: version.number })}
          {version.live ? (
            <span className="ml-1.5 text-callout text-healthy">{t("snapshots.version.live")}</span>
          ) : null}
        </div>
        <div className="text-callout text-secondary tabular">
          {t("snapshots.version.meta", {
            files: t("snapshots.files", { count: version.files ?? 0 }),
            size: formatBytes(version.bytes ?? 0),
            when: relativeTime(version.createdAt),
          })}
        </div>
      </div>
      {version.live ? null : (
        <ConfirmDialog
          trigger={
            <Button size="sm">
              <RotateCcw />
              {t("snapshots.rollback.action")}
            </Button>
          }
          title={t("snapshots.rollback.title", { number: version.number })}
          description={t("snapshots.rollback.description", { number: version.number })}
          confirmLabel={t("snapshots.rollback.confirm")}
          onConfirm={async () => {
            await change.mutateAsync({
              accountId: snapshot.accountId,
              change: { type: "rollback", snapshot: snapshot.id, version: version.number },
            });
            toast.success(t("snapshots.rollback.done", { number: version.number }));
          }}
        />
      )}
    </li>
  );
}

interface SnapshotInspectorProps {
  snapshot: SnapshotView;
  onUpdate: () => void;
}

/** A Snapshot's address, details and versions, with its actions. */
export function SnapshotInspector({ snapshot, onUpdate }: SnapshotInspectorProps) {
  const versions = useSnapshotVersions(snapshot.id);
  const remove = useSnapshotChange();
  const expires = snapshot.expiresAt
    ? new Date(snapshot.expiresAt).toLocaleString(undefined, { dateStyle: "medium" })
    : t("snapshots.detail.never");

  return (
    <Inspector
      title={snapshot.name}
      subtitle={
        snapshot.liveVersion === null
          ? t("snapshots.status.incomplete")
          : t("snapshots.status.live")
      }
      actions={
        <>
          {snapshot.url ? (
            <Button variant="primary" onClick={() => void openUrl(snapshot.url)}>
              <ExternalLink />
              {t("snapshots.open")}
            </Button>
          ) : null}
          {snapshot.liveVersion === null ? null : (
            <Button onClick={onUpdate}>
              <Upload />
              {t("snapshots.update")}
            </Button>
          )}
          <ConfirmDialog
            trigger={
              <Button variant="destructive">
                <Trash2 />
                {t("snapshots.delete.action")}
              </Button>
            }
            variant="destructive"
            title={t("snapshots.delete.title", { name: snapshot.name })}
            description={t("snapshots.delete.description")}
            confirmLabel={t("snapshots.delete.confirm")}
            onConfirm={async () => {
              await remove.mutateAsync({
                accountId: snapshot.accountId,
                change: { type: "delete", snapshot: snapshot.id },
              });
              toast.success(t("snapshots.delete.done", { name: snapshot.name }));
            }}
          />
        </>
      }
    >
      {snapshot.url ? (
        <InspectorSection title={t("snapshots.detail.address")}>
          <CopyField label={t("common.url")} value={snapshot.url} />
        </InspectorSection>
      ) : null}
      <InspectorSection title={t("snapshots.detail.title")}>
        <KeyValueGrid
          items={[
            { label: t("snapshots.detail.source"), value: sourceLabel(snapshot.source) },
            {
              label: t("snapshots.detail.size"),
              value: t("snapshots.detail.sizeValue", {
                files: t("snapshots.files", { count: snapshot.files ?? 0 }),
                size: formatBytes(snapshot.bytes ?? 0),
              }),
            },
            { label: t("snapshots.detail.protection"), value: protectionLabel(snapshot) },
            {
              label: t("snapshots.detail.pages"),
              value: snapshot.spa ? t("snapshots.detail.spa") : t("snapshots.detail.static"),
            },
            { label: t("snapshots.detail.published"), value: relativeTime(snapshot.updatedAt) },
            { label: t("snapshots.detail.expires"), value: expires },
            { label: t("snapshots.detail.worker"), value: snapshot.script, mono: true },
          ]}
        />
      </InspectorSection>
      <InspectorSection title={t("snapshots.detail.versions")}>
        {versions.isPending ? (
          <Skeleton className="h-8" />
        ) : (
          <ol className="flex flex-col rounded-card bg-surface-inset px-3 py-0.5">
            {(versions.data ?? []).map((version) => (
              <VersionRow key={version.number} snapshot={snapshot} version={version} />
            ))}
          </ol>
        )}
        <p className="text-footnote text-secondary">{t("snapshots.version.kept")}</p>
      </InspectorSection>
    </Inspector>
  );
}
