import { Shield, ShieldCheck } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { IconButton } from "@/components/ui/icon-button";
import { Sheet, SheetClose, SheetContent } from "@/components/ui/sheet";
import { Tooltip } from "@/components/ui/tooltip";
import { stripScheme } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { QuickShare } from "@/lib/ipc/bindings";
import { protectionSummary } from "../model";
import { useTaps } from "../queries";
import { TapProtection } from "./tap-protection";

/** The inspector's tap for a Quick Share (it has the share's id), while inspected. */
function useShareTap(share: QuickShare) {
  const taps = useTaps();
  return share.inspected ? (taps.data?.find((tap) => tap.id === share.id) ?? null) : null;
}

/**
 * Protects a Quick Share on this computer (password page, secret link, basic auth,
 * bearer tokens, networks, user agents): the inspector enforces it, so it's offered once
 * the share is inspected.
 */
export function ProtectShareButton({ share }: { share: QuickShare }) {
  const tap = useShareTap(share);
  const [open, setOpen] = useState(false);
  const on = tap ? protectionSummary(tap.protection).length > 0 : false;
  const label = tap ? t("inspector.share.protect") : t("inspector.share.protectNeedsInspect");
  return (
    <>
      <Tooltip content={label}>
        <IconButton
          icon={on ? ShieldCheck : Shield}
          label={label}
          variant="secondary"
          size="lg"
          disabled={!tap}
          aria-pressed={on}
          className={on ? "text-accent" : undefined}
          onClick={() => setOpen(true)}
        />
      </Tooltip>
      {tap ? (
        <Sheet open={open} onOpenChange={setOpen}>
          <SheetContent
            title={t("inspector.share.protectTitle", {
              name: share.url ? stripScheme(share.url) : tap.name,
            })}
            width="lg"
            footer={
              <SheetClose asChild>
                <Button variant="primary">{t("common.done")}</Button>
              </SheetClose>
            }
          >
            <TapProtection tap={tap} />
          </SheetContent>
        </Sheet>
      ) : null}
    </>
  );
}

/** What protects the share on this computer, when anything does. */
export function ShareProtectionNote({ share }: { share: QuickShare }) {
  const tap = useShareTap(share);
  const summary = tap ? protectionSummary(tap.protection) : [];
  if (summary.length === 0) return null;
  return (
    <p className="text-callout text-secondary">
      {t("inspector.share.protected", { list: summary.join(", ") })}
    </p>
  );
}
