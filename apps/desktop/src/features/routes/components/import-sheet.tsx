import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Sheet, SheetClose, SheetContent } from "@/components/ui/sheet";
import { t } from "@/lib/i18n";
import type { FoundRoute, LocalSetup, RouteView, ZoneRef } from "@/lib/ipc/bindings";

const key = (route: FoundRoute) => `${route.hostname}${route.path ?? ""}`;

function inZones(hostname: string, zones: readonly ZoneRef[]) {
  const host = hostname.replace(/^\*\./, "");
  return zones.some((z) => host === z.name || host.endsWith(`.${z.name}`));
}

/** Why a found route can't be imported into this account, if it can't. */
function blocker(route: FoundRoute, zones: readonly ZoneRef[], existing: readonly RouteView[]) {
  if (route.unsupported) return route.unsupported;
  if (!inZones(route.hostname, zones)) return t("import.notInAccount");
  if (existing.some((r) => r.hostname === route.hostname && r.path === route.path)) {
    return t("import.alreadyRouted");
  }
  return null;
}

interface ImportSheetProps {
  open: boolean;
  setups: readonly LocalSetup[];
  zones: readonly ZoneRef[];
  existing: readonly RouteView[];
  onClose: () => void;
  /** Continues to the plan review with the chosen routes. */
  onReview: (routes: FoundRoute[]) => void;
}

/**
 * Pick routes from existing cloudflared configs to move onto this Mac's tunnel. The
 * files are only read; nothing changes until the plan is applied.
 */
export function ImportSheet({
  open,
  setups,
  zones,
  existing,
  onClose,
  onReview,
}: ImportSheetProps) {
  const importable = setups.flatMap((s) => s.routes).filter((r) => !blocker(r, zones, existing));
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  // Everything importable starts selected each time the sheet opens.
  // biome-ignore lint/correctness/useExhaustiveDependencies: reset per opening
  useEffect(() => {
    if (open) setChosen(new Set(importable.map(key)));
  }, [open]);

  const toggle = (route: FoundRoute, on: boolean) =>
    setChosen((current) => {
      const next = new Set(current);
      if (on) next.add(key(route));
      else next.delete(key(route));
      return next;
    });
  const selected = importable.filter((r) => chosen.has(key(r)));

  return (
    <Sheet open={open} onOpenChange={(next) => !next && onClose()}>
      <SheetContent
        title={t("import.title")}
        description={t("import.description")}
        width="lg"
        footer={
          <>
            <SheetClose asChild>
              <Button>{t("common.cancel")}</Button>
            </SheetClose>
            <Button
              variant="primary"
              disabled={selected.length === 0}
              onClick={() => onReview(selected)}
            >
              {t("routeSheet.review")}
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-5">
          {setups.map((setup) => (
            <section key={setup.configPath} className="flex flex-col gap-2">
              <h3 className="text-headline">
                <span className="selectable font-mono text-mono">{setup.configPath}</span>
              </h3>
              {setup.problem ? (
                <p className="text-callout text-error">{setup.problem}</p>
              ) : setup.routes.length === 0 ? (
                <p className="text-callout text-secondary">{t("import.noRoutes")}</p>
              ) : (
                <ul className="flex flex-col rounded-card bg-surface-inset px-3 py-1">
                  {setup.routes.map((route) => {
                    const why = blocker(route, zones, existing);
                    const id = `import-${setup.configPath}-${key(route)}`;
                    return (
                      <li
                        key={key(route)}
                        className="flex min-h-9 items-center gap-2.5 border-inset border-b-hairline py-1.5 last:border-b-0"
                      >
                        <Checkbox
                          id={id}
                          disabled={why !== null}
                          checked={why === null && chosen.has(key(route))}
                          onCheckedChange={(value) => toggle(route, value === true)}
                        />
                        <label htmlFor={id} className="flex min-w-0 flex-1 flex-col">
                          <span className="truncate text-body">
                            {route.hostname}
                            {route.path ? ` ${route.path}` : ""}
                            <span className="text-secondary"> → {route.service}</span>
                          </span>
                          {why ? <span className="text-callout text-secondary">{why}</span> : null}
                        </label>
                      </li>
                    );
                  })}
                </ul>
              )}
              {setup.hasGlobalOptions ? (
                <p className="text-callout text-secondary">
                  This file has settings for all routes (originRequest). They aren't imported; check
                  that the routes work after importing.
                </p>
              ) : null}
            </section>
          ))}
        </div>
      </SheetContent>
    </Sheet>
  );
}
