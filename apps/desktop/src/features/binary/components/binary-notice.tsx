import { useIsFetching, useQueryClient } from "@tanstack/react-query";
import { TriangleAlert } from "lucide-react";
import { detectPlatform } from "@/app/platform";
import { CopyField } from "@/components/patterns/copy-field";
import { Button } from "@/components/ui/button";
import { Disclosure } from "@/components/ui/disclosure";
import { ProgressBar } from "@/components/ui/progress-bar";
import { t } from "@/lib/i18n";
import type { BinaryInfo, InstallProgress } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { queryKeys } from "@/lib/ipc/query-keys";
import { useInstallBinary } from "../queries";

const MB = 1024 * 1024;

export function describeProgress(progress: InstallProgress | null): {
  label: string;
  value?: number;
} {
  if (!progress) return { label: t("binary.progress.contacting") };
  switch (progress.step) {
    case "downloading":
      return {
        label: t("binary.progress.downloading", {
          received: (progress.received / MB).toFixed(1),
          total: (progress.total / MB).toFixed(1),
        }),
        value: progress.total > 0 ? progress.received / progress.total : 0,
      };
    case "verifying":
      return { label: t("binary.progress.verifying") };
    case "installing":
      return { label: t("binary.progress.installing") };
  }
}

/** The package manager's command for cloudflared, where there's one everyone has. */
function packageCommand(outdated: boolean): string | null {
  switch (detectPlatform()) {
    case "macos":
      return outdated ? "brew upgrade cloudflared" : "brew install cloudflared";
    case "windows":
      return `winget ${outdated ? "upgrade" : "install"} --id Cloudflare.cloudflared`;
    default:
      // Linux: each distribution packages it differently (or not at all).
      return null;
  }
}

/** Whether `binary` can run everything Teitunnel needs. */
export function binaryReady(binary: BinaryInfo | null | undefined): boolean {
  return binary?.supported === true;
}

/**
 * Shown when cloudflared is missing or too old: a one-click verified install into
 * Teitunnel's own folder (a Homebrew install is never modified), or Homebrew.
 */
export function BinaryNotice({ binary }: { binary: BinaryInfo | null }) {
  const queryClient = useQueryClient();
  const checking = useIsFetching({ queryKey: queryKeys.binary.status() }) > 0;
  const install = useInstallBinary();
  const status = describeProgress(install.progress);
  const error = install.error ? toIpcError(install.error) : null;
  const outdated = binary !== null;
  const title = outdated
    ? binary.version
      ? t("binary.tooOld", { version: binary.version })
      : t("binary.tooOldUnknown")
    : t("binary.missing");

  return (
    <section aria-label={title} className="flex gap-3 rounded-card bg-surface-inset p-4">
      <TriangleAlert
        aria-hidden
        className="mt-0.5 size-4 shrink-0 text-warning"
        strokeWidth={1.75}
      />
      <div className="flex min-w-0 flex-1 flex-col gap-3">
        <div>
          <h2 className="text-headline">{title}</h2>
          <p className="mt-0.5 text-callout text-secondary">
            {outdated ? t("binary.tooOldDetail") : t("binary.missingDetail")}
          </p>
        </div>
        {install.isPending ? (
          <div className="flex flex-col gap-1.5" aria-live="polite">
            <ProgressBar
              label={t("binary.installing")}
              {...(status.value === undefined ? {} : { value: status.value })}
            />
            <span className="text-callout text-secondary tabular">{status.label}</span>
          </div>
        ) : (
          <div className="flex items-center gap-2">
            <Button variant="primary" onClick={() => install.mutate()}>
              {t("binary.install")}
            </Button>
            <Button
              variant="plain"
              pending={checking}
              onClick={() =>
                void queryClient.invalidateQueries({ queryKey: queryKeys.binary.status() })
              }
            >
              {t("binary.checkAgain")}
            </Button>
          </div>
        )}
        {error ? (
          <p role="alert" className="text-callout text-error">
            {error.message}
          </p>
        ) : null}
        {packageCommand(outdated) ? (
          <Disclosure
            title={
              <span className="text-callout font-normal text-secondary">
                {t("binary.homebrew")}
              </span>
            }
          >
            <CopyField
              label={t("binary.command")}
              value={packageCommand(outdated) ?? ""}
              className="max-w-80"
            />
          </Disclosure>
        ) : null}
      </div>
    </section>
  );
}
