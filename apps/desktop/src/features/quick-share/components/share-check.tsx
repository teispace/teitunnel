import { CircleCheck } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { CheckNotes, devServerName, HostRejectionFix } from "@/features/dev-server";
import { t } from "@/lib/i18n";
import type { DevServer, HostHeader, Verification } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";

/** "Sends Host: localhost:5173 so Vite accepts the public address." */
export function HostHeaderNote({ header }: { header: HostHeader }) {
  return (
    <p className="text-callout text-secondary">
      {header.autoFor
        ? t("devServer.sendsHostAuto", {
            host: header.value,
            server: devServerName(header.autoFor),
          })
        : t("devServer.sendsHost", { host: header.value })}
    </p>
  );
}

interface ShareCheckProps {
  /** The check through Cloudflare (`null` while there's none yet). */
  check: Verification | null;
  /** What sending the Host header does: restart a Quick Share, or change a route. */
  via: "share" | "route";
  /** Sends the Host header the dev server expects. */
  onSendHost?: ((host: string) => Promise<unknown>) | undefined;
  sending: boolean;
  onCheck: () => Promise<unknown>;
  checking: boolean;
  /** Cloudflare is still connecting the address: say so rather than show an error. */
  settling?: boolean | undefined;
}

/**
 * What the check after a share went live found, fixed in place: a dev server refusing
 * the address, an edge error, a stream a Quick Share can't carry. After a fix, the next
 * check that passes says so.
 */
export function ShareCheck({
  check,
  via,
  onSendHost,
  sending,
  onCheck,
  checking,
  settling = false,
}: ShareCheckProps) {
  /** The server a fix was tried for, to confirm when it answers. */
  const [fixing, setFixing] = useState<DevServer | null>(null);
  const [checked, setChecked] = useState(false);
  const rejection = check?.failure?.type === "hostRejected" ? check.failure.rejection : null;

  const sendHost = (host: string) => {
    if (!rejection || !onSendHost) return;
    setFixing(rejection.server);
    setChecked(false);
    onSendHost(host).catch((error: unknown) =>
      toast.error(t("devServer.fixFailed"), { description: toIpcError(error).message }),
    );
  };
  const checkAgain = () => {
    if (rejection) setFixing(rejection.server);
    void onCheck().then(() => setChecked(true));
  };

  // Nothing to say: take no room in the card.
  if (!check || (!check.failure && !check.eventStream && !check.links && !fixing)) return null;
  if (settling) {
    return (
      <p role="status" className="text-callout text-secondary">
        {t("quickShare.settling")}
      </p>
    );
  }
  return (
    <div className="flex flex-col gap-2" aria-live="polite">
      {rejection ? (
        <>
          <HostRejectionFix
            rejection={rejection}
            via={via}
            onSendHost={onSendHost ? sendHost : undefined}
            sending={sending}
            onCheck={checkAgain}
            checking={checking}
          />
          {checked && fixing && !checking ? (
            <p role="status" className="text-callout text-warning">
              {t("devServer.stillRejected", { server: devServerName(fixing) })}
            </p>
          ) : null}
        </>
      ) : null}
      {fixing && check && !check.failure ? (
        <p role="status" className="flex items-center gap-1.5 text-callout text-healthy">
          <CircleCheck aria-hidden className="size-3.5" />
          {t("devServer.fixed", { server: devServerName(fixing) })}
        </p>
      ) : null}
      {check ? (
        <CheckNotes
          check={check}
          quickShare={via === "share"}
          onCheck={checkAgain}
          checking={checking}
        />
      ) : null}
    </div>
  );
}
