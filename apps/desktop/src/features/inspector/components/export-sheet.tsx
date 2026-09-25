import { Check, Copy, Download } from "lucide-react";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Select } from "@/components/ui/select";
import { Sheet, SheetClose, SheetContent } from "@/components/ui/sheet";
import { t } from "@/lib/i18n";
import type { ExchangeId, TrafficFormat } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { FORMATS, formatLabel } from "../model";
import { useExport, useExportSave } from "../queries";

/** Characters of the preview rendered at most (Copy and Save take everything). */
const PREVIEW = 20_000;

interface ExportSheetProps {
  /** Requests to export (empty: closed). */
  ids: readonly ExchangeId[];
  onClose: () => void;
}

/**
 * Requests as cURL, HTTPie, fetch, raw HTTP, HAR, JSON or Markdown, redacted unless
 * Redact is unticked; copied or saved to Downloads.
 */
export function ExportSheet({ ids, onClose }: ExportSheetProps) {
  const [format, setFormat] = useState<TrafficFormat>("curl");
  const [redact, setRedact] = useState(true);
  const [copied, setCopied] = useState(false);
  const open = ids.length > 0;
  const text = useExport(open ? ids : [], format, redact);
  const save = useExportSave();

  // Redacted again each time it opens.
  useEffect(() => {
    if (!open) return;
    setRedact(true);
    setCopied(false);
  }, [open]);

  const copy = async () => {
    if (!text.data) return;
    await navigator.clipboard.writeText(text.data);
    setCopied(true);
    setTimeout(() => setCopied(false), 1400);
  };

  const contents = text.data ?? "";
  return (
    <Sheet open={open} onOpenChange={(next) => !next && onClose()}>
      <SheetContent
        title={t("inspector.export.title")}
        description={t("inspector.export.description", { count: ids.length })}
        width="lg"
        footer={
          <>
            <SheetClose asChild>
              <Button className="mr-auto">{t("common.close")}</Button>
            </SheetClose>
            <Button disabled={!text.data} onClick={() => void copy()}>
              {copied ? <Check /> : <Copy />}
              {copied ? t("inspector.export.copied") : t("inspector.export.copy")}
            </Button>
            <Button
              variant="primary"
              disabled={!text.data}
              pending={save.isPending}
              onClick={() =>
                save.mutate(
                  { ids, format, redact },
                  {
                    onSuccess: (path) => toast.success(t("inspector.export.saved", { path })),
                    onError: (error) => toast.error(toIpcError(error).message),
                  },
                )
              }
            >
              <Download /> {t("inspector.export.save")}
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-3">
          <div className="flex flex-wrap items-center gap-x-6 gap-y-2">
            <label htmlFor="export-format" className="flex items-center gap-2 text-body">
              {t("inspector.export.format")}
              <Select
                id="export-format"
                label={t("inspector.export.format")}
                options={FORMATS.map((value) => ({ value, label: formatLabel(value) }))}
                value={format}
                onValueChange={setFormat}
                className="w-36"
              />
            </label>
            <label htmlFor="export-redact" className="flex items-center gap-2 text-body">
              <Checkbox
                id="export-redact"
                checked={redact}
                onCheckedChange={(on) => setRedact(on === true)}
              />
              {t("inspector.export.redact")}
            </label>
          </div>
          <p className={redact ? "text-callout text-secondary" : "text-callout text-warning"}>
            {redact ? t("inspector.export.redactHelp") : t("inspector.export.unredacted")}
          </p>
          <pre
            aria-busy={text.isFetching || undefined}
            className="selectable max-h-80 min-h-24 overflow-auto rounded-control bg-surface-inset px-2 py-1.5 font-mono text-mono whitespace-pre"
          >
            {contents.length > PREVIEW ? `${contents.slice(0, PREVIEW)}\n…` : contents}
          </pre>
          {text.error ? (
            <p role="alert" className="text-callout text-error">
              {toIpcError(text.error).message}
            </p>
          ) : null}
        </div>
      </SheetContent>
    </Sheet>
  );
}
