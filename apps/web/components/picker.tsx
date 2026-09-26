"use client";

import { type CSSProperties, type ReactNode, useEffect, useId, useRef, useState } from "react";

export interface PickerItem {
  id: string;
  /** Its icon, as an element (a server component can't pass a component to this one). */
  icon: ReactNode;
  name: string;
  note: string;
  /** What it shows when picked; its `.tt-pick-result` parts appear after a moment. */
  panel: ReactNode;
}

/** How long each item is shown before the next, and how long it takes to "do" its job. */
const DWELL = 5200;
const WORK = 1100;

/**
 * A list beside a stage: pick an item and the stage shows it at work, or let the items take
 * turns while the list is on screen, with a line filling under the current one. Once
 * someone picks, the turns stop. Without motion the stage shows each result at once.
 */
export function Picker({ items, label }: { items: PickerItem[]; label: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const prefix = useId();
  const [active, setActive] = useState(0);
  const [done, setDone] = useState(true);
  const [cycling, setCycling] = useState(false);
  const [moving, setMoving] = useState(false);
  const [picked, setPicked] = useState(false);

  useEffect(() => {
    const element = ref.current;
    if (
      !element ||
      !document.documentElement.classList.contains("tt-motion") ||
      !("IntersectionObserver" in window)
    )
      return;
    setMoving(true);
    const observer = new IntersectionObserver(([entry]) => setCycling(!!entry?.isIntersecting), {
      threshold: 0.4,
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  // Each item starts, then shows what it did.
  // biome-ignore lint/correctness/useExhaustiveDependencies: replays whenever the item changes
  useEffect(() => {
    if (!moving) return;
    setDone(false);
    const timer = window.setTimeout(() => setDone(true), WORK);
    return () => window.clearTimeout(timer);
  }, [active, moving]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: each item gets its full turn
  useEffect(() => {
    if (!cycling || picked) return;
    const timer = window.setTimeout(() => setActive((now) => (now + 1) % items.length), DWELL);
    return () => window.clearTimeout(timer);
  }, [active, cycling, picked, items.length]);

  const pick = (index: number) => {
    setPicked(true);
    setActive(index);
  };

  // Arrow keys move between tabs, as tablists do.
  const onKeyDown = (event: React.KeyboardEvent) => {
    const step = { ArrowDown: 1, ArrowRight: 1, ArrowUp: -1, ArrowLeft: -1 }[event.key];
    if (!step) return;
    event.preventDefault();
    const next = (active + step + items.length) % items.length;
    pick(next);
    document.getElementById(`${prefix}-tab-${next}`)?.focus();
  };

  const item = items[active];
  return (
    <div
      ref={ref}
      className="tt-picker"
      data-running={cycling && !picked ? "" : undefined}
      style={{ "--dwell": `${DWELL}ms` } as CSSProperties}
    >
      <div role="tablist" aria-label={label} className="tt-pick-list" onKeyDown={onKeyDown}>
        {items.map(({ id, icon, name, note }, index) => (
          <button
            key={id}
            type="button"
            role="tab"
            id={`${prefix}-tab-${index}`}
            aria-selected={index === active}
            aria-controls={`${prefix}-panel`}
            tabIndex={index === active ? 0 : -1}
            onClick={() => pick(index)}
            className="tt-pick-tab"
          >
            <span className="tt-pick-icon" aria-hidden>
              {icon}
            </span>
            <span className="min-w-0 text-left">
              <span className="block text-sm font-medium">{name}</span>
              <span className="tt-pick-note block truncate text-xs text-fd-muted-foreground">
                {note}
              </span>
            </span>
            <span className="tt-pick-bar" aria-hidden />
          </button>
        ))}
      </div>
      {item ? (
        <div
          id={`${prefix}-panel`}
          role="tabpanel"
          aria-labelledby={`${prefix}-tab-${active}`}
          className="tt-pick-stage"
        >
          <div key={item.id} className="tt-pick-panel" data-done={done ? "" : undefined}>
            {item.panel}
          </div>
        </div>
      ) : null}
    </div>
  );
}
