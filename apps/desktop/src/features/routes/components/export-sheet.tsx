import { useMutation, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { Sheet, SheetClose, SheetContent } from "@/components/ui/sheet";
import { Skeleton } from "@/components/ui/skeleton";
import { type MessageKey, t } from "@/lib/i18n";
import { commands, type ExportFormat } from "@/lib/ipc/bindings";
import { call, toIpcError } from "@/lib/ipc/client";

const FORMATS = [
  { value: "configYaml", label: "config.yml" },
  { value: "dockerCompose", label: "Docker Compose" },
  { value: "terraform", label: "Terraform" },
] as const;

const HINTS: Record<ExportFormat, MessageKey> = {
  configYaml: "export.hint.configYaml",
  dockerCompose: "export.hint.dockerCompose",
  terraform: "export.hint.terraform",
};

interface ExportSheetProps {
  accountId: string;
  /** One of this Mac's tunnels; the default one when absent. */
  tunnelId?: string | null;
  open: boolean;
  onClose: () => void;
}

/** Export this Mac's tunnel and routes as configuration for other tools. */
export function ExportSheet({ accountId, tunnelId = null, open, onClose }: ExportSheetProps) {
  const [format, setFormat] = useState<ExportFormat>("configYaml");
  const file = useQuery({
    queryKey: ["routes", "export", accountId, tunnelId, format],
    queryFn: () => call(commands.routesExport(accountId, tunnelId, format)),
    enabled: open,
    staleTime: 0,
  });
  const save = useMutation({
    mutationFn: () => call(commands.routesExportSave(accountId, tunnelId, format)),
    onSuccess: (path) => toast.success(t("export.saved"), { description: path }),
    onError: (error) => toast.error(toIpcError(error).message),
  });
  const contents = file.data?.contents ?? null;

  return (
    <Sheet open={open} onOpenChange={(next) => !next && onClose()}>
      <SheetContent
        title={t("export.title")}
        description={t(HINTS[format])}
        width="lg"
        footer={
          <>
            <SheetClose asChild>
              <Button>{t("common.done")}</Button>
            </SheetClose>
            <Button
              disabled={contents === null}
              onClick={() =>
                contents !== null &&
                void navigator.clipboard
                  .writeText(contents)
                  .then(() => toast.success(t("export.copied")))
              }
            >
              {t("export.copy")}
            </Button>
            <Button
              variant="primary"
              disabled={contents === null}
              pending={save.isPending}
              onClick={() => save.mutate()}
            >
              {t("export.save")}
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-3">
          <SegmentedControl
            label={t("export.format")}
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
            <p className="text-callout text-secondary">{t("export.empty")}</p>
          ) : (
            <textarea
              readOnly
              value={contents}
              aria-label={t("export.contents", { file: file.data?.fileName ?? t("export.title") })}
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
