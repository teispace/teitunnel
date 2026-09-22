import { useQueryClient } from "@tanstack/react-query";
import { TriangleAlert } from "lucide-react";
import { CopyField } from "@/components/patterns/copy-field";
import { Button } from "@/components/ui/button";
import { Disclosure } from "@/components/ui/disclosure";
import { ProgressBar } from "@/components/ui/progress-bar";
import type { BinaryInfo, InstallProgress } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { queryKeys } from "@/lib/ipc/query-keys";
import { useInstallBinary } from "../queries";

const MB = 1024 * 1024;

export function describeProgress(progress: InstallProgress | null): {
  label: string;
  value?: number;
} {
  if (!progress) return { label: "Contacting GitHub…" };
  switch (progress.step) {
    case "downloading":
      return {
        label: `Downloading ${(progress.received / MB).toFixed(1)} of ${(progress.total / MB).toFixed(1)} MB`,
        value: progress.total > 0 ? progress.received / progress.total : 0,
      };
    case "verifying":
      return { label: "Checking the checksum and Cloudflare's signature…" };
    case "installing":
      return { label: "Installing…" };
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
  const install = useInstallBinary();
  const status = describeProgress(install.progress);
  const error = install.error ? toIpcError(install.error) : null;
  const outdated = binary !== null;
  const title = outdated
    ? `cloudflared ${binary.version ?? ""} is too old`.replace("  ", " ")
    : "cloudflared isn't installed";

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
            {outdated
              ? "Teitunnel needs cloudflared 2025.6.1 or later. Install a current copy for Teitunnel; your existing installation stays as it is."
              : "Teitunnel uses cloudflared, Cloudflare's connector. It's downloaded from Cloudflare's GitHub releases and checked against the published checksum and Cloudflare's code signature before it's used."}
          </p>
        </div>
        {install.isPending ? (
          <div className="flex flex-col gap-1.5" aria-live="polite">
            <ProgressBar
              label="Installing cloudflared"
              {...(status.value === undefined ? {} : { value: status.value })}
            />
            <span className="text-callout text-secondary tabular">{status.label}</span>
          </div>
        ) : (
          <div className="flex items-center gap-2">
            <Button variant="primary" onClick={() => install.mutate()}>
              Install cloudflared
            </Button>
            <Button
              variant="plain"
              onClick={() =>
                void queryClient.invalidateQueries({ queryKey: queryKeys.binary.status() })
              }
            >
              Check again
            </Button>
          </div>
        )}
        {error ? (
          <p role="alert" className="text-callout text-error">
            {error.message}
          </p>
        ) : null}
        <Disclosure
          title={<span className="text-callout font-normal text-secondary">Prefer Homebrew?</span>}
        >
          <CopyField
            label="command"
            value={outdated ? "brew upgrade cloudflared" : "brew install cloudflared"}
            className="max-w-80"
          />
        </Disclosure>
      </div>
    </section>
  );
}
