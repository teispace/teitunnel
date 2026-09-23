import { useState } from "react";
import { CopyField } from "@/components/patterns/copy-field";
import { GroupedRow, GroupedSection } from "@/components/patterns/grouped-list";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ProgressBar } from "@/components/ui/progress-bar";
import { Spinner } from "@/components/ui/spinner";
import { toIpcError } from "@/lib/ipc/client";
import { useBinaryStatus, useCheckUpdate, useInstallBinary, useRevealBinary } from "../queries";
import { BinaryNotice, describeProgress } from "./binary-notice";

const sources: Record<string, string> = {
  managed: "Installed by Teitunnel",
  system: "Your installation (e.g. Homebrew)",
  override: "Set by TEITUNNEL_CLOUDFLARED",
};

/** Settings → cloudflared: which binary runs, and keeping it current. */
export function CloudflaredPane() {
  const { data: binary, isSuccess } = useBinaryStatus();
  const [checkRequested, setCheckRequested] = useState(false);
  const update = useCheckUpdate(checkRequested && binary?.source === "managed");
  const install = useInstallBinary();
  const reveal = useRevealBinary();
  const progress = describeProgress(install.progress);

  if (!isSuccess) return null;
  if (!binary?.supported) return <BinaryNotice binary={binary ?? null} />;

  const managed = binary.source === "managed";
  return (
    <div className="flex flex-col gap-5">
      <GroupedSection
        title="cloudflared"
        footer="Teitunnel runs cloudflared, Cloudflare's connector, for every Quick Share and route."
      >
        <GroupedRow label="Version">
          <span className="selectable font-mono text-mono">{binary.version ?? "Unknown"}</span>
          <Badge tone="healthy">Supported</Badge>
        </GroupedRow>
        <GroupedRow label="Source">
          <span className="text-body text-secondary">
            {sources[binary.source] ?? binary.source}
          </span>
        </GroupedRow>
        <GroupedRow label="Location">
          <CopyField label="path" value={binary.path} className="w-72" />
          <Button size="sm" onClick={() => reveal.mutate()}>
            Show in Finder
          </Button>
        </GroupedRow>
      </GroupedSection>

      <GroupedSection title="Updates">
        {managed ? (
          <GroupedRow
            label={
              update.data?.available
                ? `Version ${update.data.latest} is available`
                : update.data
                  ? "cloudflared is up to date"
                  : "Check for a newer version"
            }
            description={
              install.isPending
                ? progress.label
                : update.error
                  ? toIpcError(update.error).message
                  : "Running Quick Shares keep their current version until they restart."
            }
          >
            {install.isPending ? (
              <ProgressBar
                label="Updating cloudflared"
                className="w-32"
                {...(progress.value === undefined ? {} : { value: progress.value })}
              />
            ) : update.data?.available ? (
              <Button variant="primary" size="sm" onClick={() => install.mutate()}>
                Update
              </Button>
            ) : update.isFetching ? (
              <Spinner />
            ) : (
              <Button
                size="sm"
                onClick={() => (checkRequested ? void update.refetch() : setCheckRequested(true))}
              >
                Check Now
              </Button>
            )}
          </GroupedRow>
        ) : (
          <GroupedRow
            label="Updated by its installer"
            description="Teitunnel doesn't modify a cloudflared it didn't install. With Homebrew, run brew upgrade cloudflared."
          >
            <Button size="sm" onClick={() => install.mutate()} disabled={install.isPending}>
              Use Teitunnel's Copy
            </Button>
          </GroupedRow>
        )}
      </GroupedSection>
    </div>
  );
}
