import { useSyncExternalStore } from "react";

const QUERIES = ["(prefers-color-scheme: dark)", "(prefers-contrast: more)"];

function subscribe(onChange: () => void) {
  const lists = QUERIES.map((q) => window.matchMedia?.(q)).filter((l) => l !== undefined);
  for (const list of lists) list.addEventListener("change", onChange);
  // The appearance override pins `data-theme` on <html> (app/theme.ts).
  const observer = new MutationObserver(onChange);
  observer.observe(document.documentElement, { attributeFilter: ["data-theme"] });
  return () => {
    for (const list of lists) list.removeEventListener("change", onChange);
    observer.disconnect();
  };
}

function snapshot() {
  const media = QUERIES.map((q) => (window.matchMedia?.(q).matches ? "1" : "0")).join("");
  return `${document.documentElement.dataset["theme"] ?? "system"}:${media}`;
}

/**
 * A key that changes whenever resolved colours may have changed (light/dark, increased
 * contrast, the appearance override), for things that paint colours themselves, such
 * as canvas charts.
 */
export function useAppearance(): string {
  return useSyncExternalStore(subscribe, snapshot, () => "system");
}
