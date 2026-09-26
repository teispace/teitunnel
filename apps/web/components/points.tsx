"use client";

import { type ReactNode, useEffect, useRef, useState } from "react";

/** How long each point is highlighted, in milliseconds (the progress line's length). */
const DWELL = 4200;

/**
 * A feature's key points: while the card is on screen one is highlighted at a time, with a
 * line that fills as it's read, then the next; hovering or focusing one holds it. Everyone
 * gets the full text; without motion all are shown alike.
 */
export function Points({ items }: { items: ReactNode[] }) {
  const ref = useRef<HTMLOListElement>(null);
  const [active, setActive] = useState<number | null>(null);
  const [held, setHeld] = useState(false);

  useEffect(() => {
    const element = ref.current;
    if (
      !element ||
      !document.documentElement.classList.contains("tt-motion") ||
      !("IntersectionObserver" in window)
    )
      return;
    const observer = new IntersectionObserver(
      ([entry]) => setActive((now) => (entry?.isIntersecting ? (now ?? 0) : null)),
      { threshold: 0.6 },
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (active === null || held) return;
    const timer = window.setTimeout(() => setActive((active + 1) % items.length), DWELL);
    return () => window.clearTimeout(timer);
  }, [active, held, items.length]);

  return (
    <ol
      ref={ref}
      className="tt-points flex flex-col"
      data-running={active === null ? undefined : ""}
      onPointerLeave={() => setHeld(false)}
    >
      {items.map((item, index) => (
        <li
          // biome-ignore lint/suspicious/noArrayIndexKey: a fixed list
          key={index}
          data-active={active === index ? "" : undefined}
          data-held={active === index && held ? "" : undefined}
          className="tt-point"
          style={{ "--dwell": `${DWELL}ms` } as React.CSSProperties}
          onPointerEnter={() => {
            setActive(index);
            setHeld(true);
          }}
          onFocus={() => {
            setActive(index);
            setHeld(true);
          }}
          onBlur={() => setHeld(false)}
        >
          <span className="text-[15px] text-fd-muted-foreground [&_strong]:font-medium [&_strong]:text-fd-foreground">
            {item}
          </span>
          <span className="tt-point-bar" aria-hidden />
        </li>
      ))}
    </ol>
  );
}
