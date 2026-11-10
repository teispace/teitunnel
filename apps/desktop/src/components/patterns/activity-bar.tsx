import { type Query, useIsFetching, useIsMutating } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { t } from "@/lib/i18n";

/** How long work runs before the bar shows: quick answers never flash it. */
const DELAY_MS = 400;

/**
 * Waiting: a first load, or a refresh someone may be looking at. Polls of data already
 * shown (live traffic every second; observers with an interval) aren't.
 */
const waiting = (query: Query) =>
  query.state.data === undefined ||
  !query.observers.some((observer) => Boolean(observer.options.refetchInterval));

/**
 * A hairline progress sweep along the bottom of the title bar while the app waits on
 * something slow (Cloudflare, mostly), so a refresh in the background never looks stuck.
 */
export function ActivityBar() {
  const busy = useIsFetching({ predicate: waiting }) + useIsMutating() > 0;
  const [shown, setShown] = useState(false);
  useEffect(() => {
    if (!busy) {
      setShown(false);
      return;
    }
    const timer = setTimeout(() => setShown(true), DELAY_MS);
    return () => clearTimeout(timer);
  }, [busy]);
  if (!shown) return null;
  return (
    <div
      role="progressbar"
      aria-label={t("shell.working")}
      className="pointer-events-none absolute inset-x-0 bottom-0 h-0.5 overflow-hidden"
    >
      <div className="h-full w-1/3 animate-indeterminate rounded-full bg-accent/70" />
    </div>
  );
}
