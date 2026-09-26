import { ProgressBar } from "@/components/ui/progress-bar";
import { t, translate } from "@/lib/i18n";
import type { Outcome, StepState } from "@/lib/ipc/bindings";
import { formatBytes } from "../format";

type Transfer = Extract<StepState, { state: "transferring" }>;

/** The step sending the files, while it does. */
export function transferring(steps: Record<number, StepState>): Transfer | undefined {
  return Object.values(steps).find((s): s is Transfer => s.state === "transferring");
}

/** How far the upload is. */
export function UploadProgress({ transfer }: { transfer: Transfer }) {
  const total = transfer.totalBytes ?? 0;
  return (
    <div className="flex flex-col gap-1">
      <ProgressBar
        label={t("snapshots.sheet.uploading")}
        value={total === 0 ? 1 : (transfer.bytes ?? 0) / total}
      />
      <p className="text-callout text-secondary tabular">
        {t("snapshots.sheet.uploadProgress", {
          files: transfer.files ?? 0,
          total: transfer.totalFiles ?? 0,
          size: formatBytes(total),
        })}
      </p>
    </div>
  );
}

/** Why publishing didn't finish, and what was put back or left. */
export function FailedOutcome({ outcome }: { outcome: Exclude<Outcome, { type: "applied" }> }) {
  return (
    <p role="alert" className="text-callout text-error">
      {outcome.type === "rolledBack"
        ? t("snapshots.sheet.rolledBack", { error: translate(outcome.error) })
        : t("snapshots.sheet.partial", {
            error: translate(outcome.error),
            leftovers: outcome.leftovers.map(translate).join("; "),
          })}
    </p>
  );
}
