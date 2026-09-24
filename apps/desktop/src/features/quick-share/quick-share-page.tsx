import { AnimatePresence, LazyMotion, m } from "motion/react";
import { useState } from "react";
import { TitlebarToolbar } from "@/components/patterns/titlebar-toolbar";
import { ScrollArea } from "@/components/ui/scroll-area";
import { BinaryNotice, binaryReady, useBinaryStatus } from "@/features/binary";
import { t } from "@/lib/i18n";
import { spring } from "@/lib/motion-tokens";
import { usePageVisible } from "@/lib/use-page-visible";
import { DomainShareCard } from "./components/domain-share-card";
import { ShareCard } from "./components/share-card";
import { ShareComposer } from "./components/share-composer";
import { TerminalShareCard } from "./components/terminal-share-card";
import { useDomainShares, useQuickShares, useTerminalShares } from "./queries";

const loadFeatures = () => import("motion/react").then((mod) => mod.domMax);

/** Quick Share: a public URL for a local service, no account needed. */
export function QuickSharePage({ compose = false }: { compose?: boolean }) {
  const [scrolled, setScrolled] = useState(false);
  const sharesQuery = useQuickShares();
  const domainQuery = useDomainShares();
  const terminalQuery = useTerminalShares();
  const shares = sharesQuery.data ?? [];
  const domainShares = domainQuery.data ?? [];
  const terminalShares = terminalQuery.data ?? [];
  const loaded = sharesQuery.isSuccess && domainQuery.isSuccess && terminalQuery.isSuccess;
  const binary = useBinaryStatus();
  const visible = usePageVisible();
  const missing = binary.isSuccess && !binaryReady(binary.data);

  return (
    <LazyMotion features={loadFeatures} strict>
      <TitlebarToolbar title={t("quickShare.title")} separator={scrolled} />
      <ScrollArea onScrolledChange={setScrolled}>
        <div className="mx-auto flex max-w-[680px] flex-col gap-4 px-5 pt-2 pb-8">
          <section className="flex flex-col gap-3 rounded-card bg-surface-inset p-4">
            <div>
              <h2 className="text-headline">{t("quickShare.heading")}</h2>
              <p className="mt-0.5 text-callout text-secondary">{t("quickShare.headingDetail")}</p>
            </div>
            <ShareComposer disabled={missing} autoFocus={compose} />
          </section>

          {missing ? <BinaryNotice binary={binary.data ?? null} /> : null}

          <AnimatePresence initial={false}>
            {domainShares.map((share) => (
              <m.div
                key={`${share.accountId}/${share.hostname}`}
                layout
                initial={{ opacity: 0, scale: 0.98 }}
                animate={{ opacity: 1, scale: 1 }}
                {...(visible ? { exit: { opacity: 0, scale: 0.98 } } : {})}
                transition={spring("smooth")}
              >
                <DomainShareCard share={share} />
              </m.div>
            ))}
            {terminalShares.map((share) => (
              <m.div
                key={`${share.owner}/${share.url}`}
                layout
                initial={{ opacity: 0, scale: 0.98 }}
                animate={{ opacity: 1, scale: 1 }}
                {...(visible ? { exit: { opacity: 0, scale: 0.98 } } : {})}
                transition={spring("smooth")}
              >
                <TerminalShareCard share={share} />
              </m.div>
            ))}
            {shares.map((share) => (
              <m.div
                key={share.id}
                layout
                initial={{ opacity: 0, scale: 0.98 }}
                animate={{ opacity: 1, scale: 1 }}
                {...(visible ? { exit: { opacity: 0, scale: 0.98 } } : {})}
                transition={spring("smooth")}
              >
                <ShareCard share={share} />
              </m.div>
            ))}
          </AnimatePresence>

          {loaded &&
          shares.length === 0 &&
          domainShares.length === 0 &&
          terminalShares.length === 0 &&
          !missing ? (
            <p className="px-1 text-center text-callout text-tertiary">
              {t("quickShare.emptyHint")}
            </p>
          ) : null}
        </div>
      </ScrollArea>
    </LazyMotion>
  );
}
