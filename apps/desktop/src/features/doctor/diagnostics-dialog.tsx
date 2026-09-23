import { useMutation, useQuery } from "@tanstack/react-query";
import { FileArchive } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { useUiStore } from "@/app/ui-store";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogClose, DialogContent, DialogTrigger } from "@/components/ui/dialog";
import { IconButton } from "@/components/ui/icon-button";
import { Spinner } from "@/components/ui/spinner";
import { t } from "@/lib/i18n";
import { commands } from "@/lib/ipc/bindings";
import { call, toIpcError } from "@/lib/ipc/client";

function size(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/**
 * Help ▸ Export diagnostics: shows what goes into the bundle (redacted: no tokens or
 * keys), then saves it to Downloads.
 */
export function DiagnosticsDialog() {
  const open = useUiStore((state) => state.diagnosticsOpen);
  const setOpen = useUiStore((state) => state.setDiagnosticsOpen);
  const preview = useQuery({
    queryKey: ["diagnostics", "preview"],
    queryFn: () => call(commands.diagnosticsPreview()),
    enabled: open,
    staleTime: 0,
    gcTime: 0,
  });
  const [withCloudflared, setWithCloudflared] = useState(false);
  const save = useMutation({
    mutationFn: () => call(commands.diagnosticsExport(withCloudflared)),
    onSuccess: (path) => {
      setOpen(false);
      toast.success(t("diagnostics.saved"), { description: path });
    },
    onError: (error) => toast.error(toIpcError(error).message),
  });
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <IconButton icon={FileArchive} label={t("diagnostics.export")} />
      </DialogTrigger>
      <DialogContent
        title={t("diagnostics.title")}
        description={t("diagnostics.description")}
        footer={
          <>
            <DialogClose asChild>
              <Button>{t("common.cancel")}</Button>
            </DialogClose>
            <Button
              variant="primary"
              disabled={!preview.isSuccess || save.isPending}
              onClick={() => save.mutate()}
            >
              {save.isPending
                ? withCloudflared
                  ? t("diagnostics.savingCloudflared")
                  : t("diagnostics.saving")
                : t("export.save")}
            </Button>
          </>
        }
      >
        {preview.isPending ? (
          <div className="flex items-center gap-2 text-body text-secondary">
            <Spinner className="size-3.5" /> {t("diagnostics.collecting")}
          </div>
        ) : preview.error ? (
          <p role="alert" className="text-callout text-error">
            {toIpcError(preview.error).message}
          </p>
        ) : (
          <ul
            aria-label={t("diagnostics.files")}
            className="flex flex-col rounded-card bg-surface-inset px-3 py-1"
          >
            {(preview.data ?? []).map((file) => (
              <li
                key={file.name}
                title={file.excerpt}
                className="flex h-7 items-center gap-2 border-inset border-b-hairline last:border-b-0"
              >
                <span className="min-w-0 flex-1 truncate font-mono text-mono">{file.name}</span>
                <span className="text-callout text-secondary tabular">{size(file.size)}</span>
              </li>
            ))}
          </ul>
        )}
        <div className="mt-4 flex flex-col gap-1">
          <label htmlFor="diag-cloudflared" className="flex items-center gap-2 text-body">
            <Checkbox
              id="diag-cloudflared"
              checked={withCloudflared}
              disabled={save.isPending}
              onCheckedChange={(value) => setWithCloudflared(value === true)}
            />
            {t("diagnostics.cloudflared")}
          </label>
          <p className="pl-6 text-callout text-secondary">{t("diagnostics.cloudflaredHelp")}</p>
        </div>
      </DialogContent>
    </Dialog>
  );
}
