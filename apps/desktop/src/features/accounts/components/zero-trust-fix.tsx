import { ExternalLink, KeyRound } from "lucide-react";
import { useEffect } from "react";
import { Button } from "@/components/ui/button";
import { t } from "@/lib/i18n";
import { openUrl } from "@/lib/open-url";

/** Cloudflare Zero Trust's dashboard, which asks for a team name on the first visit. */
const ZERO_TRUST = "https://one.dash.cloudflare.com/";

/**
 * Logins need a Zero Trust organization, which only the dashboard can create (it asks for
 * a team name and a plan). Opens it, then tries again when the window regains focus.
 */
export function ZeroTrustFix({ onRetry, retrying }: { onRetry: () => void; retrying: boolean }) {
  useEffect(() => {
    window.addEventListener("focus", onRetry);
    return () => window.removeEventListener("focus", onRetry);
  }, [onRetry]);

  return (
    <section
      aria-label={t("zeroTrustFix.title")}
      className="flex gap-3 rounded-card bg-surface-inset p-4"
    >
      <KeyRound aria-hidden className="mt-0.5 size-4 shrink-0 text-warning" />
      <div className="flex min-w-0 flex-1 flex-col gap-3">
        <h3 className="text-headline">{t("zeroTrustFix.title")}</h3>
        <p className="text-callout text-secondary">{t("zeroTrustFix.detail")}</p>
        <ol className="flex list-decimal flex-col gap-1 pl-5 text-callout">
          <li>{t("zeroTrustFix.step1")}</li>
          <li>{t("zeroTrustFix.step2")}</li>
        </ol>
        <div className="flex flex-wrap items-center gap-2">
          <Button size="sm" variant="primary" onClick={() => void openUrl(ZERO_TRUST)}>
            {t("zeroTrustFix.open")} <ExternalLink />
          </Button>
          <Button size="sm" disabled={retrying} onClick={onRetry}>
            {t("zeroTrustFix.tryAgain")}
          </Button>
        </div>
        <p className="text-footnote text-secondary">{t("permissionFix.autoCheck")}</p>
      </div>
    </section>
  );
}
