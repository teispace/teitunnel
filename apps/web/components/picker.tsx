"use client";

import { type CSSProperties, type ReactNode, useEffect, useId, useRef, useState } from "react";
import { onSceneProgress } from "./scenes";

export interface PickerItem {
  id: string;
  /** Its icon, as an element (a server component can't pass a component to this one). */
  icon: ReactNode;
  name: string;
  note: string;
  /** What it shows when picked; its `.tt-pick-result` parts appear after a moment. */
  panel: ReactNode;
}

/** How long an item takes to "do" its job, and how long each is shown by default. */
const WORK = 1100;
const DWELL = 5200;

/**
 * A list beside a stage: pick an item and the stage shows it at work, or let the items take
 * turns while the list is on screen, with a line filling under the current one. Once
 * someone picks, the turns stop. Without motion the stage shows each result at once.
 *
 * `pinned`: on wide screens with room for it, the whole section (its `header`, the list and
 * stage, and its `footer`) holds still while the page scrolls, and the scroll steps through
 * the items instead of a timer; picking one scrolls to it.
 */
export function Picker({
  items,
  label,
  dwell = DWELL,
  pinned = false,
  header,
  footer,
}: {
  items: PickerItem[];
  label: string;
  /** Milliseconds each item is shown before the next. */
  dwell?: number;
  /** Stepped through by scrolling on wide screens. */
  pinned?: boolean;
  /** Above and below the list, held still with it when pinned. */
  header?: ReactNode;
  footer?: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const pinRef = useRef<HTMLDivElement>(null);
  const [scrolled, setScrolled] = useState(false);
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

  // Pinned: the scroll through the section picks the item, on wide screens with motion.
  useEffect(() => {
    const element = pinRef.current;
    if (!pinned || !element) return;
    // Wide and tall enough for the whole section to fit on screen.
    const wide = matchMedia("(min-width: 1024px) and (min-height: 680px)");
    let off = () => {};
    const update = () => {
      off();
      off = () => {};
      const on = wide.matches && document.documentElement.classList.contains("tt-motion");
      setScrolled(on);
      if (!on) return;
      off = onSceneProgress(element, (progress) => {
        const at = progress * items.length;
        const index = Math.min(items.length - 1, Math.floor(at));
        setActive(index);
        element.style.setProperty("--fill", Math.min(1, at - index).toFixed(3));
      });
    };
    update();
    wide.addEventListener("change", update);
    return () => {
      off();
      wide.removeEventListener("change", update);
    };
  }, [pinned, items.length]);

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
    if (!cycling || picked || scrolled) return;
    const timer = window.setTimeout(() => setActive((now) => (now + 1) % items.length), dwell);
    return () => window.clearTimeout(timer);
  }, [active, cycling, picked, scrolled, items.length, dwell]);

  const pick = (index: number) => {
    const pin = pinRef.current;
    if (scrolled && pin) {
      // The middle of the item's stretch of the scroll.
      const room = pin.offsetHeight - window.innerHeight;
      const top = pin.getBoundingClientRect().top + window.scrollY;
      window.scrollTo({ top: top + ((index + 0.5) / items.length) * room, behavior: "smooth" });
      return;
    }
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
  const picker = (
    <div
      ref={ref}
      className="tt-picker"
      data-running={cycling && !picked && !scrolled ? "" : undefined}
      data-scrolled={scrolled ? "" : undefined}
      style={{ "--dwell": `${dwell}ms` } as CSSProperties}
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
  if (!pinned) {
    return (
      <>
        {header}
        {picker}
        {footer}
      </>
    );
  }
  return (
    <div
      ref={pinRef}
      data-scene="pin"
      data-pinned={scrolled ? "" : undefined}
      className="tt-pin"
      style={{ "--n": items.length } as CSSProperties}
    >
      <div className="tt-pin-stage">
        {header}
        {picker}
        {footer}
      </div>
    </div>
  );
}
