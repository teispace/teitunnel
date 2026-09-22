import { AnimatePresence, LazyMotion, m } from "motion/react";
import { useState } from "react";
import { TitlebarToolbar } from "@/components/patterns/titlebar-toolbar";
import { ScrollArea } from "@/components/ui/scroll-area";
import { spring } from "@/lib/motion-tokens";
import { BinaryNotice } from "./components/binary-notice";
import { ShareCard } from "./components/share-card";
import { ShareComposer } from "./components/share-composer";
import { useBinaryStatus, useQuickShares } from "./queries";

const loadFeatures = () => import("motion/react").then((mod) => mod.domMax);

/** Quick Share: a public URL for a local service, no account needed. */
export function QuickSharePage() {
  const [scrolled, setScrolled] = useState(false);
  const { data: shares = [] } = useQuickShares();
  const binary = useBinaryStatus();
  const missing = binary.isSuccess && binary.data === null;

  return (
    <LazyMotion features={loadFeatures} strict>
      <TitlebarToolbar title="Quick Share" separator={scrolled} />
      <ScrollArea onScrolledChange={setScrolled}>
        <div className="mx-auto flex max-w-[680px] flex-col gap-4 px-5 pt-2 pb-8">
          <section className="flex flex-col gap-3 rounded-card bg-surface-inset p-4">
            <div>
              <h2 className="text-headline">Share a local service</h2>
              <p className="mt-0.5 text-callout text-secondary">
                Get a temporary public URL. Anyone with the link can open it until you stop sharing.
              </p>
            </div>
            <ShareComposer disabled={missing} />
          </section>

          {missing ? <BinaryNotice /> : null}

          <AnimatePresence initial={false}>
            {shares.map((share) => (
              <m.div
                key={share.id}
                layout
                initial={{ opacity: 0, scale: 0.98 }}
                animate={{ opacity: 1, scale: 1 }}
                exit={{ opacity: 0, scale: 0.98 }}
                transition={spring("smooth")}
              >
                <ShareCard share={share} />
              </m.div>
            ))}
          </AnimatePresence>

          {shares.length === 0 && !missing ? (
            <p className="px-1 text-center text-callout text-tertiary">
              Running shares appear here. They stop when you quit Teitunnel.
            </p>
          ) : null}
        </div>
      </ScrollArea>
    </LazyMotion>
  );
}
