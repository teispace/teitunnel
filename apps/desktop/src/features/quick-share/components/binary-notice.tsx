import { useQueryClient } from "@tanstack/react-query";
import { TriangleAlert } from "lucide-react";
import { CopyField } from "@/components/patterns/copy-field";
import { Button } from "@/components/ui/button";
import { Disclosure } from "@/components/ui/disclosure";
import { ProgressBar } from "@/components/ui/progress-bar";
import type { InstallProgress } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { queryKeys } from "@/lib/ipc/query-keys";
import { useInstallBinary } from "../queries";

const MB = 1024 * 1024;

function describeProgress(progress: InstallProgress | null): { label: string; value?: number } {
  if (!progress) return { label: "Contacting GitHub…" };
  switch (progress.step) {
    case "downloading": {
      const received = (progress.received / MB).toFixed(1);
      const total = (progress.total / MB).toFixed(1);
      return {
        label: `Downloading ${received} of ${total} MB`,
        value: progress.total > 0 ? progress.received / progress.total : 0,
      };
    }
    case "verifying":
      return { label: "Checking the checksum and Cloudflare's signature…" };
    case "installing":
      return { label: "Installing…" };
  }
}

/** Shown when cloudflared isn't installed: one-click verified install, or Homebrew. */
export function BinaryNotice() {
  const queryClient = useQueryClient();
  const install = useInstallBinary();
  const status = describeProgress(install.progress);
  const error = install.error ? toIpcError(install.error) : null;

  return (
    <section
      aria-label="cloudflared isn't installed"
      className="flex gap-3 rounded-card bg-surface-inset p-4"
    >
      <TriangleAlert
        aria-hidden
        className="mt-0.5 size-4 shrink-0 text-warning"
        strokeWidth={1.75}
      />
      <div className="flex min-w-0 flex-1 flex-col gap-3">
        <div>
          <h2 className="text-headline">cloudflared isn't installed</h2>
          <p className="mt-0.5 text-callout text-secondary">
            Teitunnel uses cloudflared, Cloudflare's connector. It's downloaded from Cloudflare's
            GitHub releases and checked against the published checksum and Cloudflare's code
            signature before it's used.
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
          <CopyField label="command" value="brew install cloudflared" className="max-w-80" />
        </Disclosure>
      </div>
    </section>
  );
}
