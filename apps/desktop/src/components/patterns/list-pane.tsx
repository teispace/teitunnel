import { Fragment, type KeyboardEvent, type ReactNode, useId, useRef } from "react";
import { cn } from "@/lib/cn";

interface ListPaneProps<T> {
  items: readonly T[];
  getId: (item: T) => string;
  selectedId: string | null;
  onSelect: (id: string) => void;
  renderRow: (item: T) => ReactNode;
  label: string;
  /** Shown when there are no items. */
  empty?: ReactNode;
  /** Section title of an item; a header is shown where it changes (items must be sorted). */
  groupOf?: (item: T) => string;
  className?: string;
}

/**
 * A selectable list (NSTableView source list). The list is one tab stop; arrow keys,
 * Home and End move the selection, which follows focus. Selection is accent-coloured
 * only while the list has focus, grey otherwise, as in AppKit.
 */
export function ListPane<T>({
  items,
  getId,
  selectedId,
  onSelect,
  renderRow,
  label,
  empty,
  groupOf,
  className,
}: ListPaneProps<T>) {
  const baseId = useId();
  const listRef = useRef<HTMLDivElement>(null);
  const optionId = (id: string) => `${baseId}-${id}`;
  const index = items.findIndex((item) => getId(item) === selectedId);

  const select = (next: number) => {
    const item = items[Math.min(items.length - 1, Math.max(0, next))];
    if (!item) return;
    const id = getId(item);
    onSelect(id);
    listRef.current
      ?.querySelector(`[id="${CSS.escape(optionId(id))}"]`)
      ?.scrollIntoView({ block: "nearest" });
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const moves: Record<string, number> = {
      ArrowDown: index + 1,
      ArrowUp: index < 0 ? 0 : index - 1,
      Home: 0,
      End: items.length - 1,
    };
    const next = moves[event.key];
    if (next === undefined) return;
    event.preventDefault();
    select(next);
  };

  if (items.length === 0 && empty) return <>{empty}</>;

  return (
    <div
      ref={listRef}
      role="listbox"
      aria-label={label}
      tabIndex={0}
      {...(selectedId ? { "aria-activedescendant": optionId(selectedId) } : {})}
      onKeyDown={onKeyDown}
      className={cn(
        "group/list min-h-0 flex-1 overflow-y-auto overscroll-contain px-2.5 py-1 outline-none",
        className,
      )}
    >
      {items.map((item, position) => {
        const id = getId(item);
        const selected = id === selectedId;
        const group = groupOf?.(item);
        const previous = position > 0 ? items[position - 1] : undefined;
        const header =
          group !== undefined && (previous === undefined || groupOf?.(previous) !== group);
        return (
          <Fragment key={id}>
            {header ? (
              <div
                role="presentation"
                className="truncate px-2 pt-2.5 pb-1 text-footnote font-semibold text-tertiary first:pt-1"
              >
                {group}
              </div>
            ) : null}
            <div
              id={optionId(id)}
              role="option"
              aria-selected={selected}
              tabIndex={-1}
              onMouseDown={(event) => {
                if (event.button !== 0) return;
                onSelect(id);
                listRef.current?.focus();
                event.preventDefault();
              }}
              className={cn(
                "rounded-row",
                selected && "bg-surface-selected-inactive",
                selected &&
                  "group-focus-within/list:bg-surface-selected group-focus-within/list:text-on-accent group-focus-within/list:[--text-secondary:color-mix(in_srgb,var(--text-on-accent)_75%,transparent)]",
              )}
            >
              {renderRow(item)}
            </div>
          </Fragment>
        );
      })}
    </div>
  );
}

interface ListRowProps {
  title: ReactNode;
  subtitle?: ReactNode;
  leading?: ReactNode;
  trailing?: ReactNode;
}

/** Row content: 28 px single-line or 44 px two-line (DESIGN §5). */
export function ListRow({ title, subtitle, leading, trailing }: ListRowProps) {
  return (
    <div className={cn("flex items-center gap-2 px-2", subtitle ? "h-11" : "h-7")}>
      {leading ? <div className="flex shrink-0 items-center">{leading}</div> : null}
      <div className="min-w-0 flex-1">
        <div className="truncate text-body">{title}</div>
        {subtitle ? <div className="truncate text-callout text-secondary">{subtitle}</div> : null}
      </div>
      {trailing ? (
        <div className="flex shrink-0 items-center gap-1.5 text-callout text-secondary">
          {trailing}
        </div>
      ) : null}
    </div>
  );
}
