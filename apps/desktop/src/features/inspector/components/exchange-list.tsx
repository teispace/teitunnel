import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowUp, Repeat, Webhook } from "lucide-react";
import {
  type KeyboardEvent,
  type MouseEvent,
  memo,
  type ReactNode,
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { cn } from "@/lib/cn";
import { t } from "@/lib/i18n";
import type { ExchangeRow } from "@/lib/ipc/bindings";
import { formatClock, formatMs, rowSize, statusClass, statusText } from "../model";

/** Row height: one 12 px line in a 24 px row, like Xcode's and Proxyman's tables. */
export const ROW = 24;

/** The columns, shared by the header and the rows. */
const columns =
  "grid grid-cols-[3.5rem_minmax(0,1fr)_3.25rem_4.25rem_4.5rem_4.75rem] items-center gap-x-2 px-3";

interface ExchangeListProps {
  rows: readonly ExchangeRow[];
  selectedId: string | null;
  /** Requests picked with ⌘-click (to compare or export several). */
  marked: ReadonlySet<string>;
  onSelect: (id: string, event: { toggle: boolean }) => void;
  empty: string;
  /** Shown after the last row (Load earlier). */
  footer?: ReactNode;
}

interface RowProps {
  row: ExchangeRow;
  id: string;
  /** Offset from the top, in pixels. */
  start: number;
  odd: boolean;
  selected: boolean;
  picked: boolean;
  onMouseDown: (event: MouseEvent, id: string) => void;
}

/** A row's classes, merged once for each look rather than on every render. */
const rowClass = (odd: boolean, selected: boolean) =>
  cn(
    columns,
    "absolute inset-x-0 top-0 h-6 font-mono text-mono",
    odd && "bg-surface-inset/60",
    selected && "bg-surface-selected-inactive",
    selected &&
      "group-focus/list:bg-surface-selected group-focus/list:text-on-accent group-focus/list:[--text-secondary:color-mix(in_srgb,var(--text-on-accent)_75%,transparent)] group-focus/list:[--color-healthy:var(--text-on-accent)] group-focus/list:[--color-warning:var(--text-on-accent)] group-focus/list:[--color-error:var(--text-on-accent)]",
  );
const ROW_CLASSES = [
  [rowClass(false, false), rowClass(false, true)],
  [rowClass(true, false), rowClass(true, true)],
] as const;

/** What a row says: depends on the request alone, so it survives rows moving. */
const Cells = memo(function Cells({ row }: { row: ExchangeRow }) {
  return (
    <>
      <span className="truncate font-medium">{row.method}</span>
      <span className="flex min-w-0 items-center gap-1">
        {row.replayOf ? (
          <Repeat
            aria-label={t("inspector.list.replay")}
            className="size-3 shrink-0 text-secondary"
            strokeWidth={2}
          />
        ) : null}
        {row.webhook ? (
          <Webhook
            aria-label={t("inspector.list.webhook")}
            className="size-3 shrink-0 text-secondary"
            strokeWidth={2}
          />
        ) : null}
        <span className="truncate">{row.path}</span>
      </span>
      <span className={cn("truncate tabular", statusClass(row))}>{statusText(row)}</span>
      <span className="truncate text-right text-secondary tabular">
        {row.durationMs === null ? "" : formatMs(row.durationMs)}
      </span>
      <span className="truncate text-right text-secondary tabular">{rowSize(row)}</span>
      <span className="truncate text-right text-secondary tabular">
        {formatClock(row.startedAt)}
      </span>
    </>
  );
});

interface RowProps {
  row: ExchangeRow;
  id: string;
  /** Offset from the top, in pixels. */
  start: number;
  odd: boolean;
  selected: boolean;
  picked: boolean;
  onMouseDown: (event: MouseEvent, id: string) => void;
}

/**
 * One request. Memoised: scrolling renders only the rows coming into view, and new
 * requests above only move the others (their cells stay as they are).
 */
const Row = memo(function Row({ row, id, start, odd, selected, picked, onMouseDown }: RowProps) {
  const chosen = selected || picked;
  return (
    <div
      id={id}
      role="option"
      aria-selected={chosen}
      tabIndex={-1}
      onMouseDown={(event) => onMouseDown(event, row.id)}
      title={`${row.method} ${row.host}${row.path}`}
      className={ROW_CLASSES[odd ? 1 : 0][chosen ? 1 : 0]}
      style={{ transform: `translateY(${start}px)` }}
    >
      <Cells row={row} />
    </div>
  );
});

/**
 * The live request list: newest first, only the rows in view rendered (virtualised like
 * the log viewer, D-051), so 10,000 requests scroll at 60 fps. While scrolled down, new
 * requests arriving at the top don't move what you're reading; a pill jumps back up.
 */
export function ExchangeList({
  rows,
  selectedId,
  marked,
  onSelect,
  empty,
  footer,
}: ExchangeListProps) {
  const baseId = useId();
  const scroller = useRef<HTMLDivElement>(null);
  const [above, setAbove] = useState(0);
  const firstId = useRef<string | null>(null);
  const list = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scroller.current,
    estimateSize: () => ROW,
    overscan: 20,
    initialRect: { width: 640, height: 480 },
    getItemKey: (index) => rows[index]?.id ?? index,
  });

  // Requests added above the rows in view: keep those rows where they are.
  useLayoutEffect(() => {
    const el = scroller.current;
    const previous = firstId.current;
    firstId.current = rows[0]?.id ?? null;
    if (!el || !previous || el.scrollTop <= 0) return;
    const added = rows.findIndex((row) => row.id === previous);
    if (added > 0) {
      el.scrollTop += added * ROW;
      setAbove((count) => count + added);
    }
  }, [rows]);

  const onScroll = () => {
    if ((scroller.current?.scrollTop ?? 0) <= 0) setAbove(0);
  };

  const index = rows.findIndex((row) => row.id === selectedId);
  // Keep the selection in view when it moves by keyboard.
  const moved = useRef(false);
  useEffect(() => {
    if (moved.current && index >= 0) list.scrollToIndex(index, { align: "auto" });
    moved.current = false;
  }, [index, list]);

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const page = Math.max(1, Math.floor((scroller.current?.clientHeight ?? 480) / ROW) - 1);
    const moves: Record<string, number> = {
      ArrowDown: index + 1,
      ArrowUp: index < 0 ? 0 : index - 1,
      PageDown: index + page,
      PageUp: index - page,
      Home: 0,
      End: rows.length - 1,
    };
    const next = moves[event.key];
    if (next === undefined || rows.length === 0) return;
    event.preventDefault();
    const row = rows[Math.min(rows.length - 1, Math.max(0, next))];
    if (!row) return;
    moved.current = true;
    onSelect(row.id, { toggle: false });
  };

  const optionId = (id: string) => `${baseId}-${id}`;
  // Stable for the memoised rows, calling the latest `onSelect`.
  const select = useRef(onSelect);
  select.current = onSelect;
  const onMouseDown = useCallback((event: MouseEvent, id: string) => {
    if (event.button !== 0) return;
    event.preventDefault();
    scroller.current?.focus();
    select.current(id, { toggle: event.metaKey || event.ctrlKey });
  }, []);

  return (
    <div className="relative flex min-h-0 flex-1 flex-col">
      <div
        aria-hidden
        className={cn(
          columns,
          "h-[22px] shrink-0 border-separator border-b-hairline text-footnote text-secondary",
        )}
      >
        <span>{t("inspector.list.method")}</span>
        <span>{t("inspector.list.path")}</span>
        <span>{t("inspector.list.status")}</span>
        <span className="text-right">{t("inspector.list.duration")}</span>
        <span className="text-right">{t("inspector.list.size")}</span>
        <span className="text-right">{t("inspector.list.time")}</span>
      </div>
      <div
        ref={scroller}
        role="listbox"
        aria-label={t("inspector.list.label")}
        aria-multiselectable
        tabIndex={0}
        {...(selectedId && index >= 0 ? { "aria-activedescendant": optionId(selectedId) } : {})}
        onKeyDown={onKeyDown}
        onScroll={onScroll}
        className="group/list min-h-0 flex-1 overflow-y-auto overscroll-contain outline-none"
      >
        {rows.length === 0 ? (
          <p className="px-3 py-3 text-callout text-secondary">{empty}</p>
        ) : (
          <div className="relative" style={{ height: list.getTotalSize() }}>
            {list.getVirtualItems().map((item) => {
              const row = rows[item.index];
              if (!row) return null;
              return (
                <Row
                  key={item.key}
                  row={row}
                  id={optionId(row.id)}
                  start={item.start}
                  odd={item.index % 2 === 1}
                  selected={row.id === selectedId}
                  picked={marked.has(row.id)}
                  onMouseDown={onMouseDown}
                />
              );
            })}
          </div>
        )}
        {footer}
      </div>
      {above > 0 ? (
        <button
          type="button"
          onClick={() => {
            scroller.current?.scrollTo({ top: 0 });
            setAbove(0);
          }}
          className="absolute top-8 left-1/2 flex h-6 -translate-x-1/2 items-center gap-1 rounded-full bg-accent-fill px-2.5 text-callout text-on-accent shadow-raised"
        >
          <ArrowUp aria-hidden className="size-3" strokeWidth={2.25} />
          {t("inspector.list.newAbove", { count: above })}
        </button>
      ) : null}
    </div>
  );
}
