import { GroupedRow, GroupedSection } from "@/components/patterns/grouped-list";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { t } from "@/lib/i18n";
import { toIpcError } from "@/lib/ipc/client";
import { useBrowserHost, useSetBrowserHost } from "./queries";

/**
 * Settings ▸ Integrations ▸ Browser Extension: lets the Teitunnel extension talk to the
 * app (the bundled command line tool as its native messaging host) in every installed
 * browser.
 */
export function BrowserExtensionSection() {
  const { data } = useBrowserHost();
  const change = useSetBrowserHost();
  if (!data) return null;
  const detected = data.browsers.filter((b) => b.detected);
  const ready = detected.some((b) => b.installed);
  const missing = detected.some((b) => !b.installed);
  return (
    <GroupedSection
      title={t("integrations.browser.title")}
      footer={t("integrations.browser.footer")}
    >
      {!data.available ? (
        <p className="py-2 text-callout text-secondary">{t("integrations.browser.unavailable")}</p>
      ) : detected.length === 0 ? (
        <p className="py-2 text-callout text-secondary">{t("integrations.browser.none")}</p>
      ) : (
        <>
          {detected.map((browser) => (
            <GroupedRow key={browser.browser} label={browser.name}>
              <Badge tone={browser.installed ? "healthy" : "neutral"}>
                {browser.installed
                  ? t("integrations.browser.ready")
                  : t("integrations.browser.notSetUp")}
              </Badge>
            </GroupedRow>
          ))}
          <div className="flex justify-end gap-2 py-2">
            {ready ? (
              <Button
                size="sm"
                pending={change.isPending && change.variables === false}
                disabled={change.isPending}
                onClick={() => change.mutate(false)}
              >
                {t("integrations.browser.remove")}
              </Button>
            ) : null}
            {missing ? (
              <Button
                size="sm"
                variant="primary"
                pending={change.isPending && change.variables === true}
                disabled={change.isPending}
                onClick={() => change.mutate(true)}
              >
                {t("integrations.browser.setUp")}
              </Button>
            ) : null}
          </div>
        </>
      )}
      {change.error ? (
        <p role="alert" className="text-callout text-error">
          {toIpcError(change.error).message}
        </p>
      ) : null}
    </GroupedSection>
  );
}
