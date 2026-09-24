import { useRouter } from "@tanstack/react-router";
import { Info } from "lucide-react";
import { Button } from "@/components/ui/button";
import { t, translate } from "@/lib/i18n";
import type { Verification } from "@/lib/ipc/bindings";

interface CheckNotesProps {
  check: Verification;
  /** A Quick Share: mention a stream it can't carry. */
  quickShare?: boolean;
  /** Show the failure's sentence (off where the caller already shows it). */
  showMessage?: boolean;
  onCheck?: (() => void) | undefined;
  checking?: boolean;
}

/**
 * What a check through Cloudflare found, other than a dev server refusing the address
 * (that's `HostRejectionFix`): the failure with what to do next (the Doctor for a
 * connector that's down, a re-check for a server that isn't running), and a note when
 * a Quick Share's service streams events, which Quick Shares don't carry.
 */
export function CheckNotes({
  check,
  quickShare = false,
  showMessage = true,
  onCheck,
  checking = false,
}: CheckNotesProps) {
  // No router around it (isolated renders): the button just does nothing.
  const router = useRouter({ warn: false }) as ReturnType<typeof useRouter> | undefined;
  const failure = check.failure;
  const shown = failure !== null && failure.type !== "hostRejected";
  const tunnelDown = failure?.type === "noConnector" || failure?.type === "tunnelMismatch";
  const originDown = failure?.type === "originUnreachable" || failure?.type === "originTimeout";
  return (
    <>
      {shown && (tunnelDown || originDown || showMessage) ? (
        <div className="flex flex-wrap items-center gap-2">
          {showMessage && check.message ? (
            <p role="status" className="min-w-0 flex-1 text-callout text-warning">
              {translate(check.message)}
            </p>
          ) : null}
          {tunnelDown ? (
            <Button size="sm" onClick={() => void router?.navigate({ to: "/doctor" })}>
              {t("devServer.openDoctor")}
            </Button>
          ) : null}
          {originDown && onCheck ? (
            <Button size="sm" pending={checking} onClick={onCheck}>
              {t("devServer.checkAgain")}
            </Button>
          ) : null}
        </div>
      ) : null}
      {quickShare && check.eventStream ? (
        <p className="flex items-start gap-1.5 text-callout text-secondary">
          <Info aria-hidden className="mt-0.5 size-3.5 shrink-0" />
          {t("devServer.eventStream")}
        </p>
      ) : null}
    </>
  );
}
