import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { ExchangeList } from "@/features/inspector/components/exchange-list";
import { type Filters, matches, noFilters } from "@/features/inspector/model";
import { ExchangeStore, MAX_ROWS } from "@/features/inspector/store";
import type { ExchangeRow } from "@/lib/ipc/bindings";
import { mockRows } from "./mock-inspector";

const LIVE_BATCH = 20; // every 100 ms: 200 requests/s
const SCROLL_PX = 36; // per frame, like a fast trackpad fling
const NONE: ReadonlySet<string> = new Set();

/**
 * The Inspector list's budget: 10,000 requests, 200 more a second
 * arriving on top, a status filter applied, and the list scrolled every frame, while
 * every frame's interval is recorded for `scripts/perf.ts inspector`.
 */
export default function StressInspector() {
  const store = useMemo(() => new ExchangeStore(null), []);
  const rows = useSyncExternalStore(store.subscribe, store.rows);
  const [filters] = useState<Filters>({ ...noFilters, status: "2" });
  const visible = useMemo(() => rows.filter((row) => matches(row, filters)), [rows, filters]);
  const [selected, setSelected] = useState<string | null>(null);
  const holder = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const seed = mockRows(MAX_ROWS);
    store.reset(seed);
    const stats = { frames: [] as number[], logLines: 0, chartUpdates: 0 };
    window.__stress = stats;
    let last = performance.now();
    let down = true;
    let frame = requestAnimationFrame(function tick(now) {
      stats.frames.push(now - last);
      last = now;
      // The list's own scroller: fling down, then back up.
      const scroller = holder.current?.querySelector<HTMLElement>("[role=listbox]");
      if (scroller) {
        const max = scroller.scrollHeight - scroller.clientHeight;
        if (scroller.scrollTop >= max - 1) down = false;
        if (scroller.scrollTop <= 0) down = true;
        scroller.scrollTop += down ? SCROLL_PX : -SCROLL_PX;
      }
      frame = requestAnimationFrame(tick);
    });
    let n = 0;
    const live = setInterval(() => {
      const exchanges: ExchangeRow[] = Array.from({ length: LIVE_BATCH }, (_, i) => {
        n += 1;
        const template = seed[n % seed.length] as ExchangeRow;
        return { ...template, id: `live-${n}`, seq: MAX_ROWS + n, startedAtMs: Date.now() + i };
      });
      // The "log lines" counter doubles as requests received.
      stats.logLines += exchanges.length;
      store.apply({ exchanges, cleared: [], tapsChanged: false, lagged: false });
    }, 100);
    return () => {
      cancelAnimationFrame(frame);
      clearInterval(live);
    };
  }, [store]);

  return (
    <div className="flex h-full min-h-0 flex-col p-5">
      <h1 className="text-title2">Inspector stress</h1>
      <p className="text-callout text-secondary tabular">
        {visible.length} of {rows.length}
      </p>
      <div ref={holder} className="flex min-h-0 flex-1 flex-col">
        <ExchangeList
          rows={visible}
          selectedId={selected}
          marked={NONE}
          onSelect={(id) => setSelected(id)}
          empty="Waiting…"
        />
      </div>
    </div>
  );
}
