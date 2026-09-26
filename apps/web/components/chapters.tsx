"use client";

import { type CSSProperties, useEffect, useRef, useState } from "react";
import { tourPosition } from "@/lib/chapters";

/**
 * The feature tour's chapters, in a rail that stays under the header while the cards are
 * scrolled: the card on top is marked, a line fills with the way through the tour, and a
 * chapter is one click away. Links to each card's `id` without JavaScript.
 */
export function Chapters({ chapters }: { chapters: [label: string, id: string][] }) {
  const ref = useRef<HTMLElement>(null);
  const [active, setActive] = useState(0);
  const [progress, setProgress] = useState(0);

  useEffect(() => {
    const rail = ref.current;
    if (!rail) return;
    const cards = chapters
      .map(([, id]) => document.getElementById(id))
      .filter((card): card is HTMLElement => card !== null);
    let frame = 0;
    const update = () => {
      frame = 0;
      // A card is on top once it has reached the rail (give or take a little).
      const { active, progress } = tourPosition(
        cards.map((card) => card.getBoundingClientRect().top),
        rail.getBoundingClientRect().bottom + 80,
        window.innerHeight,
      );
      setActive(active);
      setProgress(progress);
    };
    const schedule = () => {
      if (frame === 0) frame = requestAnimationFrame(update);
    };
    update();
    window.addEventListener("scroll", schedule, { passive: true });
    window.addEventListener("resize", schedule);
    return () => {
      cancelAnimationFrame(frame);
      window.removeEventListener("scroll", schedule);
      window.removeEventListener("resize", schedule);
    };
  }, [chapters]);

  // On narrow screens the rail scrolls sideways: keep the current chapter in it.
  useEffect(() => {
    const rail = ref.current?.querySelector<HTMLElement>(".tt-chapters-list");
    const item = rail?.children[active] as HTMLElement | undefined;
    if (!rail || !item || rail.scrollWidth <= rail.clientWidth) return;
    rail.scrollTo({
      left: item.offsetLeft - (rail.clientWidth - item.offsetWidth) / 2,
      behavior: "smooth",
    });
  }, [active]);

  return (
    <nav
      ref={ref}
      aria-label="Features"
      className="tt-chapters"
      style={{ "--progress": progress } as CSSProperties}
    >
      <ol className="tt-chapters-list">
        {chapters.map(([label, id], index) => (
          <li key={id}>
            <a
              href={`#${id}`}
              className="tt-chapter"
              aria-current={index === active ? "step" : undefined}
            >
              <span className="font-mono text-[10px] text-fd-muted-foreground tabular-nums">
                {String(index + 1).padStart(2, "0")}
              </span>
              {label}
            </a>
          </li>
        ))}
      </ol>
      <span className="tt-chapters-bar" aria-hidden />
    </nav>
  );
}
