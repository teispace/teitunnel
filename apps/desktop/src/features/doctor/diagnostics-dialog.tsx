import { useMutation, useQuery } from "@tanstack/react-query";
import { FileArchive } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogTrigger } from "@/components/ui/dialog";
import { IconButton } from "@/components/ui/icon-button";
import { Spinner } from "@/components/ui/spinner";
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
  const [open, setOpen] = useState(false);
  const preview = useQuery({
    queryKey: ["diagnostics", "preview"],
    queryFn: () => call(commands.diagnosticsPreview()),
    enabled: open,
    staleTime: 0,
    gcTime: 0,
  });
  const save = useMutation({
    mutationFn: () => call(commands.diagnosticsExport()),
    onSuccess: (path) => {
      setOpen(false);
      toast.success("Diagnostics saved", { description: path });
    },
    onError: (error) => toast.error(toIpcError(error).message),
  });
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <IconButton icon={FileArchive} label="Export diagnostics" />
      </DialogTrigger>
      <DialogContent
        title="Export Diagnostics"
        description="A file to attach to a bug report. Tokens, keys and passwords are removed; hostnames and account ids stay so the problem can be understood."
        footer={
          <>
            <DialogClose asChild>
              <Button>Cancel</Button>
            </DialogClose>
            <Button
              variant="primary"
              disabled={!preview.isSuccess || save.isPending}
              onClick={() => save.mutate()}
            >
              {save.isPending ? "Saving…" : "Save to Downloads"}
            </Button>
          </>
        }
      >
        {preview.isPending ? (
          <div className="flex items-center gap-2 text-body text-secondary">
            <Spinner className="size-3.5" /> Collecting…
          </div>
        ) : preview.error ? (
          <p role="alert" className="text-callout text-error">
            {toIpcError(preview.error).message}
          </p>
        ) : (
          <ul aria-label="Files" className="flex flex-col rounded-card bg-surface-inset px-3 py-1">
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
      </DialogContent>
    </Dialog>
  );
}
