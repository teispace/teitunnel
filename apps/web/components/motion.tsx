"use client";

import { usePathname } from "next/navigation";
import { useEffect } from "react";

declare global {
  interface Window {
    __ttReveal?: boolean;
  }
}

/** Reveals `[data-reveal]` elements as they scroll into view, once each. */
export function Motion() {
  const pathname = usePathname();
  // biome-ignore lint/correctness/useExhaustiveDependencies: a new page brings new elements to observe
  useEffect(() => {
    window.__ttReveal = true;
    const root = document.documentElement;
    const pending = document.querySelectorAll<HTMLElement>("[data-reveal]:not([data-shown])");
    if (!root.classList.contains("tt-motion") || !("IntersectionObserver" in window)) {
      for (const element of pending) element.dataset.shown = "";
      return;
    }
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (!entry.isIntersecting) continue;
          (entry.target as HTMLElement).dataset.shown = "";
          observer.unobserve(entry.target);
        }
      },
      { rootMargin: "0px 0px -8% 0px", threshold: 0.12 },
    );
    for (const element of pending) observer.observe(element);
    return () => observer.disconnect();
  }, [pathname]);
  return null;
}
