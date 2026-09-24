import { TriangleAlert } from "lucide-react";
import { useState } from "react";
import { CopyField } from "@/components/patterns/copy-field";
import { Button } from "@/components/ui/button";
import { t } from "@/lib/i18n";
import type { HostRejection } from "@/lib/ipc/bindings";
import { devServerName } from "../names";

interface HostRejectionFixProps {
  rejection: HostRejection;
  /** What sending the Host header changes: a Quick Share restarts with a new address. */
  via: "share" | "route";
  /** Sends the Host header the server expects; left out, the button isn't offered. */
  onSendHost?: ((host: string) => void) | undefined;
  sending?: boolean;
  /** Checks again, e.g. after the dev server's config changed. */
  onCheck?: (() => void) | undefined;
  checking?: boolean;
}

/**
 * A dev server refused the public address. Fixed in place, one of two ways: send the
 * Host header it expects (recommended where only its rebinding check reads Host), or
 * allow the address with the exact config line for the framework. Where the header
 * would break origin checks (Next.js, Rails, Django, SvelteKit, Astro), the config line
 * is shown first and the header comes with a warning.
 */
export function HostRejectionFix({
  rejection,
  via,
  onSendHost,
  sending = false,
  onCheck,
  checking = false,
}: HostRejectionFixProps) {
  const server = devServerName(rejection.server);
  const host = rejection.hostHeader;
  const recommendHost = host !== null && rejection.hostHeaderSafe && onSendHost !== undefined;
  const [showConfig, setShowConfig] = useState(!recommendHost);
  const title = t("devServer.rejected");

  return (
    <section aria-label={title} className="flex gap-3 rounded-card bg-surface-inset p-4 text-left">
      <TriangleAlert aria-hidden className="mt-0.5 size-4 shrink-0 text-warning" />
      <div className="flex min-w-0 flex-1 flex-col gap-2">
        <h3 className="text-headline">{title}</h3>
        <p className="text-callout text-secondary">
          {t("devServer.rejectedDetail", { server, host: rejection.host })}
        </p>
        {rejection.server === "next" ? (
          <p className="text-callout text-secondary">{t("devServer.nextOnly")}</p>
        ) : host !== null && onSendHost ? (
          <p className="text-callout text-secondary">
            {recommendHost
              ? via === "share"
                ? t("devServer.sendHostShare", { host, server })
                : t("devServer.sendHostRoute", { host, server })
              : t("devServer.unsafe", { server })}
          </p>
        ) : null}
        {showConfig ? (
          <div className="flex flex-col gap-1">
            <p className="text-callout">
              {t("devServer.configHelp", { file: rejection.configFile })}
            </p>
            <CopyField multiline label={t("devServer.configLabel")} value={rejection.configLine} />
          </div>
        ) : null}
        <div className="flex flex-wrap items-center gap-2">
          {host !== null && onSendHost ? (
            <Button
              size="sm"
              variant={recommendHost ? "primary" : "secondary"}
              pending={sending}
              onClick={() => onSendHost(host)}
            >
              {t("devServer.sendHost", { host })}
            </Button>
          ) : null}
          {showConfig ? null : (
            <Button size="sm" onClick={() => setShowConfig(true)}>
              {t("devServer.showConfig")}
            </Button>
          )}
          {showConfig && onCheck ? (
            <Button size="sm" pending={checking} onClick={onCheck}>
              {checking ? t("devServer.checking") : t("devServer.checkAgain")}
            </Button>
          ) : null}
        </div>
      </div>
    </section>
  );
}
