import { useMutation, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { Sheet, SheetClose, SheetContent } from "@/components/ui/sheet";
import { Skeleton } from "@/components/ui/skeleton";
import { commands, type ExportFormat } from "@/lib/ipc/bindings";
import { call, toIpcError } from "@/lib/ipc/client";

const FORMATS = [
  { value: "configYaml", label: "config.yml" },
  { value: "dockerCompose", label: "Docker Compose" },
  { value: "terraform", label: "Terraform" },
] as const;

const HINTS: Record<ExportFormat, string> = {
  configYaml:
    "The routes as a cloudflared configuration file, to run them as a locally-managed tunnel or keep as a record.",
  dockerCompose:
    "A service that runs this tunnel in Docker. The run token isn't included; the file says how to get it.",
  terraform:
    "Terraform for the Cloudflare provider, with import blocks that adopt the existing tunnel and records instead of recreating them.",
};

interface ExportSheetProps {
  accountId: string;
  open: boolean;
  onClose: () => void;
}

/** Export this Mac's tunnel and routes as configuration for other tools. */
export function ExportSheet({ accountId, open, onClose }: ExportSheetProps) {
  const [format, setFormat] = useState<ExportFormat>("configYaml");
  const file = useQuery({
    queryKey: ["routes", "export", accountId, format],
    queryFn: () => call(commands.routesExport(accountId, format)),
    enabled: open,
    staleTime: 0,
  });
  const save = useMutation({
    mutationFn: () => call(commands.routesExportSave(accountId, format)),
    onSuccess: (path) => toast.success("Saved to Downloads", { description: path }),
    onError: (error) => toast.error(toIpcError(error).message),
  });
  const contents = file.data?.contents ?? null;

  return (
    <Sheet open={open} onOpenChange={(next) => !next && onClose()}>
      <SheetContent
        title="Export"
        description={HINTS[format]}
        width="lg"
        footer={
          <>
            <SheetClose asChild>
              <Button>Done</Button>
            </SheetClose>
            <Button
              disabled={contents === null}
              onClick={() =>
                contents !== null &&
                void navigator.clipboard.writeText(contents).then(() => toast.success("Copied"))
              }
            >
              Copy
            </Button>
            <Button
              variant="primary"
              disabled={contents === null || save.isPending}
              onClick={() => save.mutate()}
            >
              Save to Downloads
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-3">
          <SegmentedControl
            label="Format"
            segments={FORMATS}
            value={format}
            onValueChange={setFormat}
          />
          {file.isPending ? (
            <Skeleton className="h-64" />
          ) : file.error ? (
            <p role="alert" className="text-callout text-error">
              {toIpcError(file.error).message}
            </p>
          ) : contents === null ? (
            <p className="text-callout text-secondary">
              This Mac has no routes in this account yet, so there's nothing to export.
            </p>
          ) : (
            <textarea
              readOnly
              value={contents}
              aria-label={`${file.data?.fileName ?? "Export"} contents`}
              spellCheck={false}
              rows={16}
              className="selectable w-full resize-none rounded-control bg-surface-inset px-3 py-2 font-mono text-[11px] leading-4 outline-offset-0"
            />
          )}
        </div>
      </SheetContent>
    </Sheet>
  );
}
