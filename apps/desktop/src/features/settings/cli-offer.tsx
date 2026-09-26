import { SquareTerminal } from "lucide-react";
import { CopyField } from "@/components/patterns/copy-field";
import { Button } from "@/components/ui/button";
import { t } from "@/lib/i18n";
import { toIpcError } from "@/lib/ipc/client";
import { useCliStatus, useSetCliInstalled, useSettings, useUpdateSettings } from "./queries";

/**
 * A one-time offer to put the `teitunnel` command on the PATH. It only shows where
 * the installer didn't already do it: the Windows installer and the Linux packages put it
 * there, and Homebrew links it, so in practice the macOS disk image and the AppImage.
 * Answering either way hides it for good; Settings ▸ General ▸ Command line stays.
 */
export function CliOffer() {
  const { data: settings } = useSettings();
  const { data: cli } = useCliStatus();
  const install = useSetCliInstalled();
  const update = useUpdateSettings();
  if (!settings || settings.cliOfferDismissed || cli?.state !== "notInstalled") return null;

  const dismiss = () => update.mutate({ cliOfferDismissed: true });
  return (
    <section
      aria-labelledby="cli-offer-title"
      className="flex items-start gap-3 rounded-card bg-surface-inset px-3 py-2.5"
    >
      <SquareTerminal aria-hidden className="mt-0.5 size-4 text-secondary" strokeWidth={1.75} />
      <div className="flex min-w-0 flex-1 flex-col gap-1.5">
        <div>
          <h2 id="cli-offer-title" className="text-body">
            {t("cliOffer.title")}
          </h2>
          <p className="text-callout text-secondary">
            {cli.command ? t("cliOffer.manual") : t("cliOffer.detail")}
          </p>
        </div>
        {cli.command ? <CopyField label={t("settings.cli.command")} value={cli.command} /> : null}
        {install.error ? (
          <p role="alert" className="text-callout text-error">
            {toIpcError(install.error).message}
          </p>
        ) : null}
      </div>
      <div className="flex shrink-0 gap-1.5">
        <Button size="sm" variant="plain" disabled={update.isPending} onClick={dismiss}>
          {t("cliOffer.notNow")}
        </Button>
        {cli.command ? (
          <Button size="sm" disabled={update.isPending} onClick={dismiss}>
            {t("cliOffer.done")}
          </Button>
        ) : (
          <Button
            size="sm"
            variant="primary"
            disabled={update.isPending}
            pending={install.isPending}
            onClick={() => install.mutate(true, { onSuccess: dismiss })}
          >
            {t("cliOffer.install")}
          </Button>
        )}
      </div>
    </section>
  );
}
