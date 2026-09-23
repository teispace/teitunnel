import { QrCode } from "lucide-react";
import { useState } from "react";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Tooltip } from "@/components/ui/tooltip";
import { t } from "@/lib/i18n";
import { useQrCode } from "../queries";

/** Shows the URL as a QR code, for opening it on a phone. */
export function QrButton({ url }: { url: string }) {
  const [open, setOpen] = useState(false);
  const { data: svg } = useQrCode(url, open);
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <Tooltip content={t("quickShare.showQr")}>
        <PopoverTrigger asChild>
          <button
            type="button"
            aria-label={t("quickShare.showQr")}
            className="flex size-7 items-center justify-center rounded-full bg-surface-control text-secondary outline-offset-1 active:bg-surface-control-pressed"
          >
            <QrCode aria-hidden className="size-4" strokeWidth={1.75} />
          </button>
        </PopoverTrigger>
      </Tooltip>
      <PopoverContent className="w-60 p-4 text-center">
        {svg ? (
          <div
            className="mx-auto size-48 text-primary [&_svg]:size-full"
            // The SVG is generated in Rust from the URL only (no user-controlled markup).
            // biome-ignore lint/security/noDangerouslySetInnerHtml: trusted, locally generated SVG
            dangerouslySetInnerHTML={{ __html: svg }}
          />
        ) : (
          <div className="mx-auto size-48" />
        )}
        <p className="mt-3 text-callout text-secondary">Scan to open on your phone.</p>
      </PopoverContent>
    </Popover>
  );
}
