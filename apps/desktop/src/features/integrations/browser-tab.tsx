import { CopyField } from "@/components/patterns/copy-field";
import { GroupedRow, GroupedSection, SkeletonSection } from "@/components/patterns/grouped-list";
import { Button } from "@/components/ui/button";
import { Disclosure } from "@/components/ui/disclosure";
import { relativeTime } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { BrowserHostStatus } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { openUrl } from "@/lib/open-url";
import { useBrowserHost, useSetBrowserHost } from "./queries";

/** Where the extension and its setup are explained (it isn't in the stores yet). */
export const EXTENSION_DOCS = "https://teitunnel.teispace.com/docs/guides/browser-extension";

/** "Set up · extension connected 5 min ago". */
export function describeBrowser(browser: BrowserHostStatus): string {
  if (!browser.installed) return t("ai.browser.notSetUp");
  const seen =
    browser.extensionSeenAt === null
      ? t("ai.browser.notSeen")
      : t("ai.browser.seen", { when: relativeTime(browser.extensionSeenAt) });
  return `${t("ai.browser.ready")} · ${seen}`;
}

function BrowserRow({ browser }: { browser: BrowserHostStatus }) {
  const change = useSetBrowserHost();
  const busy = change.isPending && change.variables?.browser === browser.browser;
  return (
    <div className="flex flex-col py-1">
      <GroupedRow label={browser.name} description={describeBrowser(browser)}>
        <Button
          size="sm"
          variant={browser.installed ? "secondary" : "primary"}
          pending={busy}
          disabled={change.isPending && !busy}
          onClick={() => change.mutate({ install: !browser.installed, browser: browser.browser })}
        >
          {browser.installed ? t("ai.browser.remove") : t("ai.browser.setUp")}
        </Button>
      </GroupedRow>
      {change.error && change.variables?.browser === browser.browser ? (
        <p role="alert" className="pb-2 text-callout text-error">
          {toIpcError(change.error).message}
        </p>
      ) : null}
      <Disclosure title={t("ai.apps.details")} className="pb-1.5">
        <dl className="grid grid-cols-[auto_1fr] items-center gap-x-3 gap-y-1.5 pt-1 text-callout">
          <dt className="text-secondary">{t("ai.browser.helper")}</dt>
          <dd className="min-w-0">
            <CopyField value={browser.manifest} label={t("ai.browser.helper")} />
          </dd>
          {browser.app ? (
            <>
              <dt className="text-secondary">{t("ai.apps.foundAt")}</dt>
              <dd className="selectable min-w-0 truncate font-mono text-mono">{browser.app}</dd>
            </>
          ) : null}
        </dl>
      </Disclosure>
    </div>
  );
}

/**
 * AI & Integrations ▸ Browser: for each browser found on this computer, whether it can
 * start the extension's helper (Set Up) and whether the extension has connected
 * through it, the only proof it's installed and working.
 */
export function BrowserTab() {
  const { data } = useBrowserHost();
  const all = useSetBrowserHost();
  if (!data) return <SkeletonSection rows={3} />;
  const found = data.browsers.filter((b) => b.detected);
  const missing = data.browsers.filter((b) => !b.detected);
  return (
    <>
      <p className="px-2.5 text-callout text-secondary">{t("ai.browser.intro")}</p>
      <p className="px-2.5 text-callout text-secondary">{t("ai.browser.steps")}</p>
      {!data.available ? (
        <GroupedSection>
          <p className="py-2 text-callout text-secondary">{t("ai.browser.unavailable")}</p>
        </GroupedSection>
      ) : (
        <GroupedSection title={t("ai.browser.browsers")}>
          {found.length === 0 ? (
            <p className="py-2 text-callout text-secondary">{t("ai.browser.none")}</p>
          ) : (
            found.map((browser) => <BrowserRow key={browser.browser} browser={browser} />)
          )}
        </GroupedSection>
      )}
      <div className="flex justify-end gap-2 px-2.5">
        <Button onClick={() => void openUrl(EXTENSION_DOCS)}>{t("ai.browser.extension")}</Button>
        {found.some((b) => !b.installed) ? (
          <Button
            variant="primary"
            pending={all.isPending}
            onClick={() => all.mutate({ install: true, browser: null })}
          >
            {t("ai.browser.setUpAll")}
          </Button>
        ) : null}
      </div>
      {missing.length > 0 ? (
        <Disclosure title={t("ai.browser.notFound", { count: missing.length })} className="px-2.5">
          <p className="pt-1 text-callout text-secondary">
            {missing.map((b) => b.name).join(", ")}
          </p>
        </Disclosure>
      ) : null}
    </>
  );
}
