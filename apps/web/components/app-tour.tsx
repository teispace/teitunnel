"use client";

import { type KeyboardEvent, useId, useRef, useState } from "react";
import { Shot } from "./landing";

export interface TourStop {
  name: string;
  label: string;
  caption: string;
  alt: string;
}

/**
 * A look around the app: tabs switch the screenshot. Only the chosen screenshot is in the
 * page, so the others cost nothing until someone asks for them.
 */
export function AppTour({ stops }: { stops: TourStop[] }) {
  const [index, setIndex] = useState(0);
  const id = useId();
  const tabs = useRef<(HTMLButtonElement | null)[]>([]);
  const current = stops[index] ?? stops[0];
  if (!current) return null;

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const move =
      event.key === "ArrowRight"
        ? 1
        : event.key === "ArrowLeft"
          ? -1
          : event.key === "Home"
            ? -index
            : event.key === "End"
              ? stops.length - 1 - index
              : 0;
    if (move === 0) return;
    event.preventDefault();
    const next = (index + move + stops.length) % stops.length;
    setIndex(next);
    tabs.current[next]?.focus();
  };

  return (
    <div>
      <div
        role="tablist"
        aria-label="Screens of the app"
        onKeyDown={onKeyDown}
        className="-mx-4 mb-6 flex gap-1 overflow-x-auto px-4 pb-1 [scrollbar-width:none] sm:mx-0 sm:flex-wrap sm:justify-center sm:px-0"
      >
        {stops.map((stop, i) => (
          <button
            key={stop.name}
            ref={(element) => {
              tabs.current[i] = element;
            }}
            type="button"
            role="tab"
            id={`${id}-tab-${i}`}
            aria-selected={i === index}
            aria-controls={`${id}-panel`}
            tabIndex={i === index ? 0 : -1}
            onClick={() => setIndex(i)}
            className={`h-8 shrink-0 rounded-full px-3.5 text-sm transition-colors ${
              i === index
                ? "bg-fd-foreground text-fd-background"
                : "text-fd-muted-foreground hover:bg-fd-accent hover:text-fd-foreground"
            }`}
          >
            {stop.label}
          </button>
        ))}
      </div>
      <div role="tabpanel" id={`${id}-panel`} aria-labelledby={`${id}-tab-${index}`}>
        <Shot key={current.name} name={current.name} alt={current.alt} className="tt-panel" />
        <p className="mx-auto mt-5 max-w-2xl text-center text-sm text-fd-muted-foreground">
          {current.caption}
        </p>
      </div>
    </div>
  );
}
